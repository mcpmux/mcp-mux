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
use tracing::{debug, warn};
use uuid::Uuid;

/// Database-backed credential store for rmcp OAuth integration.
///
/// This adapter bridges our encrypted database storage to rmcp's CredentialStore trait,
/// allowing the SDK to handle token refresh automatically while we maintain persistent storage.
///
/// IMPORTANT: This store does NOT cache credentials to ensure that expires_in is always
/// recalculated on each load(). RMCP calls load() before each request to check token expiry,
/// so we must return fresh expiration data for automatic token refresh to work correctly.
pub struct DatabaseCredentialStore {
    space_id: Uuid,
    server_id: String,
    server_url: String,
    credential_repo: Arc<dyn CredentialRepository>,
    backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
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

        // NOTE: We intentionally DO NOT use a cache here because expires_in
        // must be recalculated on every load() call. RMCP's AuthClient calls
        // load() before each request to check if the token is expired.
        // If we cache the StoredCredentials with the OAuthTokenResponse,
        // the expires_in Duration becomes stale and RMCP won't refresh
        // expired tokens properly.

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

        Ok(stored)
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        // Save to database
        self.save_to_database(&credentials).await?;

        Ok(())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        // Clear tokens only (keep registration for re-auth)
        self.credential_repo
            .clear_tokens(&self.space_id, &self.server_id)
            .await
            .map_err(|e| AuthError::InternalError(format!("Failed to clear tokens: {}", e)))?;

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
    use std::sync::Arc;

    // Mock implementations for testing
    #[derive(Clone)]
    struct MockCredentialRepo {
        credential: Arc<tokio::sync::RwLock<Option<Credential>>>,
    }

    impl MockCredentialRepo {
        fn new() -> Self {
            Self {
                credential: Arc::new(tokio::sync::RwLock::new(None)),
            }
        }

        async fn set(&self, cred: Credential) {
            *self.credential.write().await = Some(cred);
        }
    }

    #[async_trait]
    impl CredentialRepository for MockCredentialRepo {
        async fn get(
            &self,
            _space_id: &Uuid,
            _server_id: &str,
        ) -> anyhow::Result<Option<Credential>> {
            Ok(self.credential.read().await.clone())
        }

        async fn save(&self, credential: &Credential) -> anyhow::Result<()> {
            *self.credential.write().await = Some(credential.clone());
            Ok(())
        }

        async fn delete(&self, _space_id: &Uuid, _server_id: &str) -> anyhow::Result<()> {
            *self.credential.write().await = None;
            Ok(())
        }

        async fn clear_tokens(&self, _space_id: &Uuid, _server_id: &str) -> anyhow::Result<bool> {
            let had_token = self.credential.read().await.is_some();
            *self.credential.write().await = None;
            Ok(had_token)
        }

        async fn list_for_space(&self, _space_id: &Uuid) -> anyhow::Result<Vec<Credential>> {
            Ok(vec![])
        }
    }

    #[derive(Clone)]
    struct MockOAuthRepo {
        registration: Arc<tokio::sync::RwLock<Option<OutboundOAuthRegistration>>>,
    }

    impl MockOAuthRepo {
        fn new() -> Self {
            Self {
                registration: Arc::new(tokio::sync::RwLock::new(None)),
            }
        }

        async fn set(&self, reg: OutboundOAuthRegistration) {
            *self.registration.write().await = Some(reg);
        }
    }

    #[async_trait]
    impl OutboundOAuthRepository for MockOAuthRepo {
        async fn get(
            &self,
            _space_id: &Uuid,
            _server_id: &str,
        ) -> anyhow::Result<Option<OutboundOAuthRegistration>> {
            Ok(self.registration.read().await.clone())
        }

        async fn save(&self, registration: &OutboundOAuthRegistration) -> anyhow::Result<()> {
            *self.registration.write().await = Some(registration.clone());
            Ok(())
        }

        async fn delete(&self, _space_id: &Uuid, _server_id: &str) -> anyhow::Result<()> {
            *self.registration.write().await = None;
            Ok(())
        }

        async fn list_for_space(
            &self,
            _space_id: &Uuid,
        ) -> anyhow::Result<Vec<OutboundOAuthRegistration>> {
            Ok(vec![])
        }
    }

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

    #[tokio::test]
    async fn test_expires_in_recalculated_on_each_load() {
        // This test verifies the critical fix: expires_in must be recalculated
        // on each load() call, not cached with stale values
        let space_id = Uuid::new_v4();
        let server_id = "test-server";
        let server_url = "https://test.example.com";

        let cred_repo = Arc::new(MockCredentialRepo::new());
        let oauth_repo = Arc::new(MockOAuthRepo::new());

        // Set up a registration
        let registration = OutboundOAuthRegistration::new(
            space_id,
            server_id,
            server_url,
            "test-client-id",
            "http://localhost:3000/callback".to_string(),
        );
        oauth_repo.set(registration).await;

        // Set up a credential that expires in 10 seconds
        let expires_at = Utc::now() + Duration::seconds(10);
        let credential = Credential {
            space_id,
            server_id: server_id.to_string(),
            value: CredentialValue::OAuth {
                access_token: "token123".to_string(),
                refresh_token: Some("refresh123".to_string()),
                expires_at: Some(expires_at),
                token_type: "Bearer".to_string(),
                scope: None,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used: Some(Utc::now()),
        };
        cred_repo.set(credential).await;

        let store =
            DatabaseCredentialStore::new(space_id, server_id, server_url, cred_repo, oauth_repo);

        // First load - should have ~10 seconds
        let stored1 = store.load().await.unwrap().unwrap();
        let token1 = stored1.token_response.as_ref().unwrap();
        let expires_in_1 = token1.expires_in().unwrap();

        assert!(expires_in_1.as_secs() >= 9 && expires_in_1.as_secs() <= 10);

        // Wait 2 seconds
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Second load - should have ~8 seconds (recalculated, not cached)
        let stored2 = store.load().await.unwrap().unwrap();
        let token2 = stored2.token_response.as_ref().unwrap();
        let expires_in_2 = token2.expires_in().unwrap();

        // This is the critical assertion: expires_in should decrease because it's recalculated
        assert!(
            expires_in_2.as_secs() >= 7 && expires_in_2.as_secs() <= 8,
            "Expected expires_in to decrease from ~10s to ~8s, but got {} seconds",
            expires_in_2.as_secs()
        );

        // Verify it actually decreased
        assert!(
            expires_in_2 < expires_in_1,
            "expires_in should decrease on subsequent loads (was {}, now {})",
            expires_in_1.as_secs(),
            expires_in_2.as_secs()
        );
    }

    #[tokio::test]
    async fn test_expired_token_detected() {
        // Verify that an expired token is properly detected
        let space_id = Uuid::new_v4();
        let server_id = "test-server";
        let server_url = "https://test.example.com";

        let cred_repo = Arc::new(MockCredentialRepo::new());
        let oauth_repo = Arc::new(MockOAuthRepo::new());

        // Set up registration
        let registration = OutboundOAuthRegistration::new(
            space_id,
            server_id,
            server_url,
            "test-client-id",
            "http://localhost:3000/callback".to_string(),
        );
        oauth_repo.set(registration).await;

        // Set up a credential that already expired (5 seconds ago)
        let expires_at = Utc::now() - Duration::seconds(5);
        let credential = Credential {
            space_id,
            server_id: server_id.to_string(),
            value: CredentialValue::OAuth {
                access_token: "expired_token".to_string(),
                refresh_token: Some("refresh123".to_string()),
                expires_at: Some(expires_at),
                token_type: "Bearer".to_string(),
                scope: None,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used: Some(Utc::now()),
        };
        cred_repo.set(credential).await;

        let store =
            DatabaseCredentialStore::new(space_id, server_id, server_url, cred_repo, oauth_repo);

        // Load should return token with expires_in = 0 (expired)
        let stored = store.load().await.unwrap().unwrap();
        let token = stored.token_response.as_ref().unwrap();
        let expires_in = token.expires_in().unwrap();

        assert_eq!(
            expires_in.as_secs(),
            0,
            "Expired token should have expires_in = 0, got {} seconds",
            expires_in.as_secs()
        );
    }

    #[tokio::test]
    async fn test_save_updates_database() {
        // Verify that save() writes to database, not just cache
        let space_id = Uuid::new_v4();
        let server_id = "test-server";
        let server_url = "https://test.example.com";

        let cred_repo = Arc::new(MockCredentialRepo::new());
        let oauth_repo = Arc::new(MockOAuthRepo::new());

        let store = DatabaseCredentialStore::new(
            space_id,
            server_id,
            server_url,
            Arc::clone(&cred_repo) as Arc<dyn CredentialRepository>,
            Arc::clone(&oauth_repo) as Arc<dyn OutboundOAuthRepository>,
        );

        // Save new credentials
        let token_response = build_token_response(
            "new_token".to_string(),
            Some("new_refresh".to_string()),
            Some(std::time::Duration::from_secs(3600)),
        );

        let credentials = StoredCredentials {
            client_id: "new-client-id".to_string(),
            token_response: Some(token_response),
        };

        store.save(credentials).await.unwrap();

        // Verify they were written to database by checking the mock repo directly
        let saved_cred = cred_repo.get(&space_id, server_id).await.unwrap().unwrap();
        match saved_cred.value {
            CredentialValue::OAuth { access_token, .. } => {
                assert_eq!(access_token, "new_token");
            }
            _ => panic!("Expected OAuth credential"),
        }

        let saved_reg = oauth_repo.get(&space_id, server_id).await.unwrap().unwrap();
        assert_eq!(saved_reg.client_id, "new-client-id");
    }
}
