//! Database-backed CredentialStore adapter for rmcp SDK integration.
//!
//! Bridges our typed credential rows (CredentialRepository) and
//! client registrations (OutboundOAuthRepository) to rmcp's unified
//! CredentialStore interface.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use mcpmux_core::{
    Credential, CredentialRepository, CredentialType, OutboundOAuthRegistration,
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

    /// Build an OAuthTokenResponse from separate access_token and refresh_token credentials.
    fn build_token_response(
        access_cred: &Credential,
        refresh_cred: Option<&Credential>,
    ) -> OAuthTokenResponse {
        // Recalculate expires_in from stored expires_at
        let expires_in = access_cred.expires_at.map(|exp| {
            let duration = exp - Utc::now();
            std::time::Duration::from_secs(duration.num_seconds().max(0) as u64)
        });

        build_token_response(
            access_cred.value.clone(),
            refresh_cred.map(|r| r.value.clone()),
            expires_in,
        )
    }

    /// Save SDK's StoredCredentials to our typed credential rows.
    async fn save_to_database(&self, creds: &StoredCredentials) -> Result<(), AuthError> {
        // Save tokens as separate rows
        if let Some(token_response) = &creds.token_response {
            let access_token_str = token_response.access_token().secret().to_string();
            let expires_at = token_response
                .expires_in()
                .map(|d| Utc::now() + Duration::seconds(d.as_secs() as i64));

            // Save access_token row
            let access_cred = Credential::access_token(
                self.space_id,
                &self.server_id,
                access_token_str,
                expires_at,
            );
            self.credential_repo.save(&access_cred).await.map_err(|e| {
                AuthError::InternalError(format!("Failed to save access token: {}", e))
            })?;

            // Save refresh_token row (if present in response).
            // If the response doesn't include a refresh_token, preserve the existing one
            // in the database — some providers (e.g. Atlassian) omit it during token rotation.
            if let Some(refresh_token) = token_response.refresh_token() {
                let refresh_cred = Credential::refresh_token(
                    self.space_id,
                    &self.server_id,
                    refresh_token.secret().to_string(),
                    None, // Refresh tokens typically don't have a fixed expiry
                );
                self.credential_repo
                    .save(&refresh_cred)
                    .await
                    .map_err(|e| {
                        AuthError::InternalError(format!("Failed to save refresh token: {}", e))
                    })?;
            }
            // If no refresh_token in response, existing refresh_token row stays untouched

            debug!(
                "[CredentialStore] Saved tokens for {}/{}",
                self.space_id, self.server_id
            );
        }

        // Save/update client registration if we have a new client_id
        if !creds.client_id.is_empty() {
            let existing_reg = self
                .backend_oauth_repo
                .get(&self.space_id, &self.server_id)
                .await
                .ok()
                .flatten();

            let should_save = match &existing_reg {
                None => true,
                Some(reg) => reg.client_id != creds.client_id,
            };

            if should_save {
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

        // Load from database — no caching, expires_in recalculated each time
        let registration = self
            .backend_oauth_repo
            .get(&self.space_id, &self.server_id)
            .await
            .map_err(|e| AuthError::InternalError(format!("Failed to load registration: {}", e)))?;

        // Load access_token and refresh_token as separate rows
        let access_cred = self
            .credential_repo
            .get(
                &self.space_id,
                &self.server_id,
                &CredentialType::AccessToken,
            )
            .await
            .map_err(|e| AuthError::InternalError(format!("Failed to load access token: {}", e)))?;

        let refresh_cred = self
            .credential_repo
            .get(
                &self.space_id,
                &self.server_id,
                &CredentialType::RefreshToken,
            )
            .await
            .map_err(|e| {
                AuthError::InternalError(format!("Failed to load refresh token: {}", e))
            })?;

        let stored = match (registration, access_cred.as_ref()) {
            (Some(reg), Some(access)) => {
                debug!(
                    "[CredentialStore] Loaded registration + token for {}/{}, client_id={}",
                    self.space_id, self.server_id, reg.client_id
                );
                let token_response = Self::build_token_response(access, refresh_cred.as_ref());
                Some(StoredCredentials {
                    client_id: reg.client_id,
                    token_response: Some(token_response),
                })
            }
            (Some(reg), None) => {
                debug!(
                    "[CredentialStore] Loaded registration (no token) for {}/{}, client_id={} - will reuse for DCR",
                    self.space_id, self.server_id, reg.client_id
                );
                Some(StoredCredentials {
                    client_id: reg.client_id,
                    token_response: None,
                })
            }
            (None, Some(access)) => {
                warn!(
                    "[CredentialStore] Token without registration for {}/{}",
                    self.space_id, self.server_id
                );
                let token_response = Self::build_token_response(access, refresh_cred.as_ref());
                Some(StoredCredentials {
                    client_id: String::new(),
                    token_response: Some(token_response),
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
        self.save_to_database(&credentials).await
    }

    async fn clear(&self) -> Result<(), AuthError> {
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
        credentials: Arc<tokio::sync::RwLock<Vec<Credential>>>,
    }

    impl MockCredentialRepo {
        fn new() -> Self {
            Self {
                credentials: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl CredentialRepository for MockCredentialRepo {
        async fn get(
            &self,
            space_id: &Uuid,
            server_id: &str,
            credential_type: &CredentialType,
        ) -> anyhow::Result<Option<Credential>> {
            let creds = self.credentials.read().await;
            Ok(creds
                .iter()
                .find(|c| {
                    c.space_id == *space_id
                        && c.server_id == server_id
                        && c.credential_type == *credential_type
                })
                .cloned())
        }

        async fn get_all(
            &self,
            space_id: &Uuid,
            server_id: &str,
        ) -> anyhow::Result<Vec<Credential>> {
            let creds = self.credentials.read().await;
            Ok(creds
                .iter()
                .filter(|c| c.space_id == *space_id && c.server_id == server_id)
                .cloned()
                .collect())
        }

        async fn save(&self, credential: &Credential) -> anyhow::Result<()> {
            let mut creds = self.credentials.write().await;
            // Upsert: remove existing with same key, then insert
            creds.retain(|c| {
                !(c.space_id == credential.space_id
                    && c.server_id == credential.server_id
                    && c.credential_type == credential.credential_type)
            });
            creds.push(credential.clone());
            Ok(())
        }

        async fn delete(
            &self,
            space_id: &Uuid,
            server_id: &str,
            credential_type: &CredentialType,
        ) -> anyhow::Result<()> {
            let mut creds = self.credentials.write().await;
            creds.retain(|c| {
                !(c.space_id == *space_id
                    && c.server_id == server_id
                    && c.credential_type == *credential_type)
            });
            Ok(())
        }

        async fn delete_all(&self, space_id: &Uuid, server_id: &str) -> anyhow::Result<()> {
            let mut creds = self.credentials.write().await;
            creds.retain(|c| !(c.space_id == *space_id && c.server_id == server_id));
            Ok(())
        }

        async fn clear_tokens(&self, space_id: &Uuid, server_id: &str) -> anyhow::Result<bool> {
            let mut creds = self.credentials.write().await;
            let before = creds.len();
            creds.retain(|c| {
                !(c.space_id == *space_id
                    && c.server_id == server_id
                    && c.credential_type.is_oauth())
            });
            Ok(creds.len() < before)
        }

        async fn list_for_space(&self, space_id: &Uuid) -> anyhow::Result<Vec<Credential>> {
            let creds = self.credentials.read().await;
            Ok(creds
                .iter()
                .filter(|c| c.space_id == *space_id)
                .cloned()
                .collect())
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

        // Set up access_token that expires in 10 seconds
        let access_cred = Credential::access_token(
            space_id,
            server_id,
            "token123",
            Some(Utc::now() + Duration::seconds(10)),
        );
        let refresh_cred = Credential::refresh_token(space_id, server_id, "refresh123", None);

        cred_repo.save(&access_cred).await.unwrap();
        cred_repo.save(&refresh_cred).await.unwrap();

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

        assert!(
            expires_in_2.as_secs() >= 7 && expires_in_2.as_secs() <= 8,
            "Expected expires_in to decrease from ~10s to ~8s, but got {} seconds",
            expires_in_2.as_secs()
        );

        assert!(
            expires_in_2 < expires_in_1,
            "expires_in should decrease on subsequent loads (was {}, now {})",
            expires_in_1.as_secs(),
            expires_in_2.as_secs()
        );
    }

    #[tokio::test]
    async fn test_expired_token_detected() {
        let space_id = Uuid::new_v4();
        let server_id = "test-server";
        let server_url = "https://test.example.com";

        let cred_repo = Arc::new(MockCredentialRepo::new());
        let oauth_repo = Arc::new(MockOAuthRepo::new());

        let registration = OutboundOAuthRegistration::new(
            space_id,
            server_id,
            server_url,
            "test-client-id",
            "http://localhost:3000/callback".to_string(),
        );
        oauth_repo.set(registration).await;

        // Set up access_token that already expired (5 seconds ago)
        let access_cred = Credential::access_token(
            space_id,
            server_id,
            "expired_token",
            Some(Utc::now() - Duration::seconds(5)),
        );
        let refresh_cred = Credential::refresh_token(space_id, server_id, "refresh123", None);

        cred_repo.save(&access_cred).await.unwrap();
        cred_repo.save(&refresh_cred).await.unwrap();

        let store =
            DatabaseCredentialStore::new(space_id, server_id, server_url, cred_repo, oauth_repo);

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

        // Verify access_token row
        let saved_access = cred_repo
            .get(&space_id, server_id, &CredentialType::AccessToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved_access.value, "new_token");

        // Verify refresh_token row
        let saved_refresh = cred_repo
            .get(&space_id, server_id, &CredentialType::RefreshToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved_refresh.value, "new_refresh");

        // Verify registration
        let saved_reg = oauth_repo.get(&space_id, server_id).await.unwrap().unwrap();
        assert_eq!(saved_reg.client_id, "new-client-id");
    }

    #[tokio::test]
    async fn test_refresh_token_preserved_when_not_in_response() {
        let space_id = Uuid::new_v4();
        let server_id = "test-server";
        let server_url = "https://test.example.com";

        let cred_repo = Arc::new(MockCredentialRepo::new());
        let oauth_repo = Arc::new(MockOAuthRepo::new());

        // Pre-populate with existing refresh token
        let existing_refresh =
            Credential::refresh_token(space_id, server_id, "original_refresh", None);
        cred_repo.save(&existing_refresh).await.unwrap();

        let store = DatabaseCredentialStore::new(
            space_id,
            server_id,
            server_url,
            Arc::clone(&cred_repo) as Arc<dyn CredentialRepository>,
            Arc::clone(&oauth_repo) as Arc<dyn OutboundOAuthRepository>,
        );

        // Save new token response WITHOUT refresh_token
        let token_response = build_token_response(
            "rotated_access".to_string(),
            None, // No refresh token in response
            Some(std::time::Duration::from_secs(3600)),
        );

        let credentials = StoredCredentials {
            client_id: "client-id".to_string(),
            token_response: Some(token_response),
        };

        store.save(credentials).await.unwrap();

        // Access token should be updated
        let saved_access = cred_repo
            .get(&space_id, server_id, &CredentialType::AccessToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved_access.value, "rotated_access");

        // Refresh token should still be the original (not overwritten)
        let saved_refresh = cred_repo
            .get(&space_id, server_id, &CredentialType::RefreshToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved_refresh.value, "original_refresh");
    }
}
