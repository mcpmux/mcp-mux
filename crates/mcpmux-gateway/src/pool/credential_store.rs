//! Database-backed CredentialStore adapter for rmcp SDK integration.
//!
//! Bridges our split storage (OutboundOAuthRepository + CredentialRepository)
//! to rmcp's unified CredentialStore interface.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use mcpmux_core::{
    Credential, CredentialRepository, CredentialValue, OutboundOAuthRegistration,
    OutboundOAuthRepository,
};
use oauth2::{basic::BasicTokenType, AccessToken, RefreshToken, TokenResponse};
use rmcp::transport::auth::{AuthError, CredentialStore, OAuthTokenResponse, StoredCredentials};
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

/// Database-backed credential store for rmcp OAuth integration.
///
/// This adapter bridges our encrypted database storage to rmcp's CredentialStore trait,
/// allowing the SDK to handle token refresh automatically while we maintain persistent storage.
pub struct DatabaseCredentialStore {
    space_id: Uuid,
    server_id: String,
    server_url: String,
    credential_repo: Arc<dyn CredentialRepository>,
    backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
    /// Cached credentials for performance (SDK calls load() frequently)
    cache: RwLock<Option<StoredCredentials>>,
}

impl DatabaseCredentialStore {
    pub fn new(
        space_id: Uuid,
        server_id: impl Into<String>,
        server_url: impl Into<String>,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
    ) -> Self {
        Self {
            space_id,
            server_id: server_id.into(),
            server_url: server_url.into(),
            credential_repo,
            backend_oauth_repo,
            cache: RwLock::new(None),
        }
    }

    /// Convert our Credential to SDK's OAuthTokenResponse
    fn to_token_response(credential: &Credential) -> Option<OAuthTokenResponse> {
        match &credential.value {
            CredentialValue::OAuth {
                access_token,
                refresh_token,
                expires_at,
                ..
            } => {
                // Build a minimal token response
                // The SDK uses oauth2 crate's StandardTokenResponse internally
                let expires_in = expires_at.map(|exp| {
                    let duration = exp - Utc::now();
                    std::time::Duration::from_secs(duration.num_seconds().max(0) as u64)
                });

                Some(build_token_response(
                    access_token.clone(),
                    refresh_token.clone(),
                    expires_in,
                ))
            }
            _ => None,
        }
    }

    /// Convert SDK's StoredCredentials to our storage format
    async fn save_to_database(&self, creds: &StoredCredentials) -> Result<(), AuthError> {
        // Save token to credentials table
        if let Some(token_response) = &creds.token_response {
            let access_token = token_response.access_token().secret().to_string();
            let refresh_token = token_response
                .refresh_token()
                .map(|t| t.secret().to_string());
            let expires_at = token_response
                .expires_in()
                .map(|d| Utc::now() + Duration::seconds(d.as_secs() as i64));

            let credential = Credential {
                space_id: self.space_id,
                server_id: self.server_id.clone(),
                value: CredentialValue::OAuth {
                    access_token,
                    refresh_token,
                    expires_at,
                    token_type: "Bearer".to_string(),
                    scope: None,
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_used: Some(Utc::now()),
            };

            self.credential_repo
                .save(&credential)
                .await
                .map_err(|e| AuthError::InternalError(format!("Failed to save token: {}", e)))?;

            debug!(
                "[CredentialStore] Saved token for {}/{}",
                self.space_id, self.server_id
            );
        }

        // Save/update client registration if we have a new client_id
        // Note: client_id comes from DCR, we need to preserve redirect_uri from existing registration
        if !creds.client_id.is_empty() {
            let existing_reg = self
                .backend_oauth_repo
                .get(&self.space_id, &self.server_id)
                .await
                .ok()
                .flatten();

            // Only save if new or client_id changed
            let should_save = match &existing_reg {
                None => true,
                Some(reg) => reg.client_id != creds.client_id,
            };

            if should_save {
                // Preserve redirect_uri from existing registration, or use empty if new
                let redirect_uri = existing_reg
                    .as_ref()
                    .and_then(|r| r.redirect_uri.clone())
                    .unwrap_or_default();

                let registration = OutboundOAuthRegistration::new(
                    self.space_id,
                    &self.server_id,
                    &self.server_url,
                    &creds.client_id,
                    redirect_uri,
                );

                self.backend_oauth_repo
                    .save(&registration)
                    .await
                    .map_err(|e| {
                        AuthError::InternalError(format!("Failed to save registration: {}", e))
                    })?;

                debug!(
                    "[CredentialStore] Saved client registration for {}/{}",
                    self.space_id, self.server_id
                );
            }
        }

        Ok(())
    }
}

#[async_trait]
impl CredentialStore for DatabaseCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        debug!(
            "[CredentialStore] load() called for {}/{}",
            self.space_id, self.server_id
        );

        // Check cache first
        {
            let cache = self.cache.read().await;
            if cache.is_some() {
                debug!(
                    "[CredentialStore] Returning cached credentials for {}/{}",
                    self.space_id, self.server_id
                );
                return Ok(cache.clone());
            }
        }

        // Load from database
        let registration = self
            .backend_oauth_repo
            .get(&self.space_id, &self.server_id)
            .await
            .map_err(|e| AuthError::InternalError(format!("Failed to load registration: {}", e)))?;

        let credential = self
            .credential_repo
            .get(&self.space_id, &self.server_id)
            .await
            .map_err(|e| AuthError::InternalError(format!("Failed to load credential: {}", e)))?;

        let stored = match (registration, credential) {
            (Some(reg), Some(cred)) => {
                debug!(
                    "[CredentialStore] Loaded registration + token for {}/{}, client_id={}",
                    self.space_id, self.server_id, reg.client_id
                );
                let token_response = Self::to_token_response(&cred);
                Some(StoredCredentials {
                    client_id: reg.client_id,
                    token_response,
                })
            }
            (Some(reg), None) => {
                // Have registration but no token yet
                debug!(
                    "[CredentialStore] Loaded registration (no token) for {}/{}, client_id={} - will reuse for DCR",
                    self.space_id, self.server_id, reg.client_id
                );
                Some(StoredCredentials {
                    client_id: reg.client_id,
                    token_response: None,
                })
            }
            (None, Some(cred)) => {
                // Shouldn't happen normally, but handle gracefully
                warn!(
                    "[CredentialStore] Token without registration for {}/{}",
                    self.space_id, self.server_id
                );
                let token_response = Self::to_token_response(&cred);
                // Use empty client_id - will need to re-register
                Some(StoredCredentials {
                    client_id: String::new(),
                    token_response,
                })
            }
            (None, None) => {
                debug!(
                    "[CredentialStore] No registration or token for {}/{} - will do fresh DCR",
                    self.space_id, self.server_id
                );
                None
            }
        };

        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = stored.clone();
        }

        Ok(stored)
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        // Save to database
        self.save_to_database(&credentials).await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = Some(credentials);
        }

        Ok(())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        // Clear tokens only (keep registration for re-auth)
        self.credential_repo
            .clear_tokens(&self.space_id, &self.server_id)
            .await
            .map_err(|e| AuthError::InternalError(format!("Failed to clear tokens: {}", e)))?;

        // Clear cache
        {
            let mut cache = self.cache.write().await;
            *cache = None;
        }

        debug!(
            "[CredentialStore] Cleared tokens for {}/{}",
            self.space_id, self.server_id
        );
        Ok(())
    }
}

/// Build an OAuthTokenResponse from components.
/// This creates a response compatible with oauth2 crate's StandardTokenResponse.
fn build_token_response(
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<std::time::Duration>,
) -> OAuthTokenResponse {
    use oauth2::{EmptyExtraTokenFields, StandardTokenResponse};

    let mut response = StandardTokenResponse::new(
        AccessToken::new(access_token),
        BasicTokenType::Bearer,
        EmptyExtraTokenFields {},
    );

    if let Some(refresh) = refresh_token {
        response.set_refresh_token(Some(RefreshToken::new(refresh)));
    }

    if let Some(expires) = expires_in {
        response.set_expires_in(Some(&expires));
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_token_response() {
        let response = build_token_response(
            "access123".to_string(),
            Some("refresh456".to_string()),
            Some(std::time::Duration::from_secs(3600)),
        );

        assert_eq!(response.access_token().secret(), "access123");
        assert_eq!(
            response.refresh_token().map(|t| t.secret().as_str()),
            Some("refresh456")
        );
    }
}
