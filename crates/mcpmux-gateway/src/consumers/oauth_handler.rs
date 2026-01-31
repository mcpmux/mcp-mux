//! OAuth Event Handler - Consumes OAuth completion events
//!
//! This consumer listens to OAuth completion events from OutboundOAuthManager
//! and updates the `oauth_connected` flag in the database.
//!
//! # Purpose
//!
//! Ensures that the `oauth_connected` flag accurately reflects user OAuth approval
//! status, which is used to prevent auto-connection without explicit user consent.

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug};
use mcpmux_core::InstalledServerRepository;

use crate::pool::OAuthCompleteEvent;

/// OAuth event handler
pub struct OAuthEventHandler {
    /// Repository for updating oauth_connected flag
    installed_server_repo: Arc<dyn InstalledServerRepository + Send + Sync>,
}

impl OAuthEventHandler {
    /// Create a new OAuth event handler
    pub fn new(
        installed_server_repo: Arc<dyn InstalledServerRepository + Send + Sync>,
    ) -> Self {
        Self {
            installed_server_repo,
        }
    }

    /// Start listening to OAuth completion events
    ///
    /// This spawns a background task that:
    /// 1. Listens for OAuth completion events
    /// 2. Updates `oauth_connected=true` on success
    /// 3. Updates `oauth_connected=false` on failure/cancel
    ///
    /// The task runs indefinitely until the receiver is dropped.
    pub fn start(
        self: Arc<Self>,
        mut oauth_rx: broadcast::Receiver<OAuthCompleteEvent>,
    ) {
        tokio::spawn(async move {
            info!("[OAuthHandler] Started listening for OAuth completion events");

            loop {
                match oauth_rx.recv().await {
                    Ok(event) => {
                        if let Err(e) = self.handle_event(event).await {
                            error!("[OAuthHandler] Failed to handle OAuth event: {}", e);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("[OAuthHandler] Lagged behind, skipped {} events", skipped);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("[OAuthHandler] OAuth event channel closed");
                        break;
                    }
                }
            }

            info!("[OAuthHandler] Stopped listening for OAuth completion events");
        });
    }

    /// Handle a single OAuth completion event
    async fn handle_event(&self, event: OAuthCompleteEvent) -> anyhow::Result<()> {
        let space_id_str = event.space_id.to_string();

        if event.success {
            info!(
                "[OAuthHandler] OAuth SUCCESS for {}/{} - setting oauth_connected=true",
                event.space_id, event.server_id
            );

            // Get installed server
            let installed = self
                .installed_server_repo
                .get_by_server_id(&space_id_str, &event.server_id)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Server {}/{} not found in installed_servers",
                        event.space_id,
                        event.server_id
                    )
                })?;

            // Set oauth_connected=true
            self.installed_server_repo
                .set_oauth_connected(&installed.id, true)
                .await?;

            info!(
                "[OAuthHandler] ✓ Updated {}/{} oauth_connected=true",
                event.space_id, event.server_id
            );
        } else {
            debug!(
                "[OAuthHandler] OAuth FAILED for {}/{}: {:?} - NOT changing oauth_connected",
                event.space_id, event.server_id, event.error
            );
            // Do NOT set oauth_connected=false on failure - let user retry
            // Only clear it on explicit disable/cancel/uninstall
        }

        Ok(())
    }
}
