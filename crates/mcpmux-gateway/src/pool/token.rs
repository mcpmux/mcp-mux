//! Token Service - OAuth token lifecycle management
//!
//! TokenService provides token lifecycle operations:
//! - `clear_tokens()` - Clears tokens on disconnect (logout)
//!
//! **NOTE**: Token refresh is now handled automatically by RMCP's AuthClient
//! with DatabaseCredentialStore. See `http.rs` transport for details.

use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::{CredentialRepository, OutboundOAuthRepository};
use tracing::info;
use uuid::Uuid;

/// TokenService - OAuth token lifecycle management
///
/// Primary function is clearing tokens on disconnect.
/// Token refresh is handled automatically by RMCP's AuthClient.
pub struct TokenService {
    credential_repo: Arc<dyn CredentialRepository>,
    #[allow(dead_code)] // Kept for potential future use
    backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
}

impl TokenService {
    pub fn new(
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
    ) -> Self {
        Self {
            credential_repo,
            backend_oauth_repo,
        }
    }

    /// Clear tokens for a server (logout/disconnect).
    /// Keeps client_id in BackendOAuthRegistration for DCR reuse.
    pub async fn clear_tokens(&self, space_id: Uuid, server_id: &str) -> Result<()> {
        self.credential_repo
            .clear_tokens(&space_id, server_id)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to clear tokens: {}", e))?;

        info!(
            "[TokenService] Cleared tokens for {}/{}",
            space_id, server_id
        );

        Ok(())
    }
}
