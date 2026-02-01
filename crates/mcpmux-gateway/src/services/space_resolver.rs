//! Space Resolution Service
//!
//! Determines which space a client should access based on their connection mode.
//! Follows SRP: Single responsibility is space resolution logic.
//! Follows DIP: Depends on repository abstractions.

use anyhow::{anyhow, Result};
use mcpmux_core::SpaceRepository;
use mcpmux_storage::InboundClientRepository;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

/// Space resolver service
///
/// SRP: Only responsible for determining which space a client should use
/// OCP: Can be extended with new resolution strategies without modification
pub struct SpaceResolverService {
    client_repo: Arc<InboundClientRepository>,
    space_repo: Arc<dyn SpaceRepository>,
}

impl SpaceResolverService {
    pub fn new(
        client_repo: Arc<InboundClientRepository>,
        space_repo: Arc<dyn SpaceRepository>,
    ) -> Self {
        Self {
            client_repo,
            space_repo,
        }
    }

    /// Resolve which space a client should access
    ///
    /// Resolution strategy based on client's connection_mode:
    /// - "locked": Use client.locked_space_id
    /// - "follow_active": Use currently active space
    /// - "ask_on_change": Use last selected space (not implemented yet)
    pub async fn resolve_space_for_client(&self, client_id: &str) -> Result<Uuid> {
        // Get client record
        let client = self
            .client_repo
            .get_client(client_id)
            .await?
            .ok_or_else(|| anyhow!("Client not found: {}", client_id))?;

        match client.connection_mode.as_str() {
            "locked" => {
                // Use locked space
                let space_id_str = client
                    .locked_space_id
                    .ok_or_else(|| anyhow!("Client has locked mode but no locked_space_id"))?;

                let space_id = Uuid::parse_str(&space_id_str)
                    .map_err(|e| anyhow!("Invalid locked_space_id: {}", e))?;

                Ok(space_id)
            }
            "follow_active" => {
                // Use currently active space
                let active_space = self
                    .space_repo
                    .get_default()
                    .await?
                    .ok_or_else(|| anyhow!("No active space set"))?;

                Ok(active_space.id)
            }
            "ask_on_change" => {
                // TODO: Implement session-based space tracking
                // For now, fall back to active space
                warn!(
                    "[SpaceResolver] ask_on_change mode not fully implemented, using active space"
                );
                let active_space = self
                    .space_repo
                    .get_default()
                    .await?
                    .ok_or_else(|| anyhow!("No active space set"))?;

                Ok(active_space.id)
            }
            mode => {
                warn!(
                    "[SpaceResolver] Unknown connection mode: {}, defaulting to active space",
                    mode
                );
                let active_space = self
                    .space_repo
                    .get_default()
                    .await?
                    .ok_or_else(|| anyhow!("No active space set"))?;

                Ok(active_space.id)
            }
        }
    }
}
