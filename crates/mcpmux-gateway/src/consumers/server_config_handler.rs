//! Server Config Event Handler - Evicts pool instances on config changes
//!
//! Listens for `ServerConfigUpdated` and removes the pooled instance for
//! enabled servers so the next connect rebuilds transport from DB.

use mcpmux_core::{DomainEvent, InstalledServerRepository};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::pool::PoolService;

/// Evicts pooled server instances when configuration changes.
pub struct ServerConfigUpdatedHandler {
    installed_server_repo: Arc<dyn InstalledServerRepository + Send + Sync>,
    pool_service: Arc<PoolService>,
}

impl ServerConfigUpdatedHandler {
    /// Create a handler wired to the installed-server repo and connection pool.
    pub fn new(
        installed_server_repo: Arc<dyn InstalledServerRepository + Send + Sync>,
        pool_service: Arc<PoolService>,
    ) -> Self {
        Self {
            installed_server_repo,
            pool_service,
        }
    }

    /// Start listening to domain events on a background task.
    pub fn start(self: Arc<Self>, mut event_rx: broadcast::Receiver<DomainEvent>) {
        tokio::spawn(async move {
            info!("[ServerConfigHandler] Started listening for ServerConfigUpdated events");

            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        if let Err(error) = self.handle_event(event).await {
                            warn!("[ServerConfigHandler] Failed to handle event: {error}");
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("[ServerConfigHandler] Lagged behind, skipped {skipped} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("[ServerConfigHandler] Event channel closed");
                        break;
                    }
                }
            }
        });
    }

    /// Handle one domain event, evicting the pool instance when applicable.
    async fn handle_event(&self, event: DomainEvent) -> anyhow::Result<()> {
        let DomainEvent::ServerConfigUpdated { space_id, server_id } = event else {
            return Ok(());
        };

        self.handle_config_updated(space_id, &server_id).await
    }

    /// Remove a pooled instance for an enabled server after a config/definition write.
    async fn handle_config_updated(&self, space_id: Uuid, server_id: &str) -> anyhow::Result<()> {
        let space_id_str = space_id.to_string();
        let Some(installed) = self
            .installed_server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
        else {
            debug!(
                "[ServerConfigHandler] Server {space_id}/{server_id} not found, skipping eviction"
            );
            return Ok(());
        };

        if !installed.enabled {
            debug!(
                "[ServerConfigHandler] Skipping eviction for disabled server {space_id}/{server_id}"
            );
            return Ok(());
        }

        info!(
            "[ServerConfigHandler] Evicting pool instance for enabled server {space_id}/{server_id} after config update"
        );
        self.pool_service.remove_instance(space_id, server_id);
        Ok(())
    }
}
