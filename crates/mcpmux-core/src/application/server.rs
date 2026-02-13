//! Server Application Service
//!
//! Manages server installation and configuration with automatic event emission.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::{DomainEvent, InstallationSource, InstalledServer, ServerDefinition};
use crate::event_bus::EventSender;
use crate::repository::{
    CredentialRepository, FeatureSetRepository, InstalledServerRepository, ServerFeatureRepository,
};

/// Application service for server installation and management
pub struct ServerAppService {
    server_repo: Arc<dyn InstalledServerRepository>,
    feature_set_repo: Option<Arc<dyn FeatureSetRepository>>,
    feature_repo: Option<Arc<dyn ServerFeatureRepository>>,
    credential_repo: Option<Arc<dyn CredentialRepository>>,
    event_sender: EventSender,
}

impl ServerAppService {
    pub fn new(
        server_repo: Arc<dyn InstalledServerRepository>,
        feature_set_repo: Option<Arc<dyn FeatureSetRepository>>,
        feature_repo: Option<Arc<dyn ServerFeatureRepository>>,
        credential_repo: Option<Arc<dyn CredentialRepository>>,
        event_sender: EventSender,
    ) -> Self {
        Self {
            server_repo,
            feature_set_repo,
            feature_repo,
            credential_repo,
            event_sender,
        }
    }

    /// List all installed servers
    pub async fn list(&self) -> Result<Vec<InstalledServer>> {
        self.server_repo.list().await
    }

    /// List servers for a specific space
    pub async fn list_for_space(&self, space_id: &str) -> Result<Vec<InstalledServer>> {
        self.server_repo.list_for_space(space_id).await
    }

    /// Get a server by space and server ID
    pub async fn get(&self, space_id: &str, server_id: &str) -> Result<Option<InstalledServer>> {
        self.server_repo.get_by_server_id(space_id, server_id).await
    }

    /// Install a server from registry
    ///
    /// Emits: `ServerInstalled`
    pub async fn install(
        &self,
        space_id: Uuid,
        server_id: &str,
        definition: &ServerDefinition,
        input_values: HashMap<String, String>,
    ) -> Result<InstalledServer> {
        let space_id_str = space_id.to_string();

        // Check if already installed
        if self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .is_some()
        {
            return Err(anyhow!("Server already installed in this space"));
        }

        // Create installation (disabled by default, user must enable)
        // Cache the definition for offline use
        let server = InstalledServer::new(&space_id_str, server_id)
            .with_inputs(input_values)
            .with_definition(definition)
            .with_enabled(false);

        self.server_repo.install(&server).await?;

        // Create server-all feature set
        if let Some(ref fs_repo) = self.feature_set_repo {
            if let Err(e) = fs_repo
                .ensure_server_all(&space_id_str, server_id, &definition.name)
                .await
            {
                tracing::warn!(
                    server_id = server_id,
                    error = %e,
                    "Failed to create server-all feature set"
                );
            }
        }

        info!(
            space_id = %space_id,
            server_id = server_id,
            "[ServerAppService] Installed server"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ServerInstalled {
            space_id,
            server_id: server_id.to_string(),
            server_name: definition.name.clone(),
        });

        Ok(server)
    }

    /// Uninstall a server
    ///
    /// For UserConfig servers, this also removes the entry from the source JSON file.
    /// For Registry/ManualEntry servers, this only removes the database record.
    ///
    /// Emits: `ServerUninstalled`
    pub async fn uninstall(&self, space_id: Uuid, server_id: &str) -> Result<()> {
        let space_id_str = space_id.to_string();

        let server = self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not installed"))?;

        // Source-aware cleanup: remove from config file if UserConfig
        if let InstallationSource::UserConfig { file_path } = &server.source {
            if let Err(e) = Self::remove_from_config_file(file_path, server_id) {
                warn!(
                    server_id = server_id,
                    file = %file_path.display(),
                    error = %e,
                    "Failed to remove server from config file"
                );
                // Continue with uninstall anyway - don't fail the whole operation
            } else {
                info!(
                    server_id = server_id,
                    file = %file_path.display(),
                    "Removed server from config file"
                );
            }
        }

        // Delete server-all feature set
        if let Some(ref fs_repo) = self.feature_set_repo {
            if let Err(e) = fs_repo.delete_server_all(&space_id_str, server_id).await {
                warn!(
                    server_id = server_id,
                    error = %e,
                    "Failed to delete server-all feature set"
                );
            }
        }

        // Delete discovered features
        if let Some(ref feature_repo) = self.feature_repo {
            if let Err(e) = feature_repo
                .delete_for_server(&space_id_str, server_id)
                .await
            {
                warn!(
                    server_id = server_id,
                    error = %e,
                    "Failed to delete server features"
                );
            }
        }

        // Delete all credentials for this server
        if let Some(ref cred_repo) = self.credential_repo {
            if let Err(e) = cred_repo.delete_all(&space_id, server_id).await {
                warn!(
                    server_id = server_id,
                    error = %e,
                    "Failed to delete server credentials"
                );
            }
        }

        // Uninstall from database
        self.server_repo.uninstall(&server.id).await?;

        info!(
            space_id = %space_id,
            server_id = server_id,
            source = ?server.source,
            "[ServerAppService] Uninstalled server"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ServerUninstalled {
            space_id,
            server_id: server_id.to_string(),
        });

        Ok(())
    }

    /// Remove a server entry from a JSON config file
    fn remove_from_config_file(file_path: &std::path::Path, server_id: &str) -> Result<()> {
        // Read current config
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| anyhow!("Failed to read config file: {}", e))?;

        // Parse as JSON
        let mut config: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| anyhow!("Failed to parse config: {}", e))?;

        // Get mcpServers object
        let servers = config
            .get_mut("mcpServers")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| anyhow!("Config file missing mcpServers object"))?;

        // Remove server
        if servers.remove(server_id).is_none() {
            // Server not found in file - this is fine, might already be removed
            return Ok(());
        }

        // Write back the modified config
        let new_content = serde_json::to_string_pretty(&config)
            .map_err(|e| anyhow!("Failed to serialize config: {}", e))?;

        std::fs::write(file_path, new_content)
            .map_err(|e| anyhow!("Failed to write config file: {}", e))?;

        Ok(())
    }

    /// Update server configuration (inputs, env overrides, args, headers)
    ///
    /// Emits: `ServerConfigUpdated`
    pub async fn update_config(
        &self,
        space_id: Uuid,
        server_id: &str,
        input_values: HashMap<String, String>,
        env_overrides: Option<HashMap<String, String>>,
        args_append: Option<Vec<String>>,
        extra_headers: Option<HashMap<String, String>>,
    ) -> Result<InstalledServer> {
        let space_id_str = space_id.to_string();

        let mut server = self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not installed"))?;

        server.input_values = input_values;
        if let Some(env) = env_overrides {
            server.env_overrides = env;
        }
        if let Some(args) = args_append {
            server.args_append = args;
        }
        if let Some(headers) = extra_headers {
            server.extra_headers = headers;
        }
        server.updated_at = chrono::Utc::now();

        self.server_repo.update(&server).await?;

        info!(
            space_id = %space_id,
            server_id = server_id,
            "[ServerAppService] Updated server config"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ServerConfigUpdated {
            space_id,
            server_id: server_id.to_string(),
        });

        Ok(server)
    }

    /// Enable a server
    ///
    /// Emits: `ServerEnabled`
    pub async fn enable(&self, space_id: Uuid, server_id: &str) -> Result<()> {
        let space_id_str = space_id.to_string();

        let server = self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not installed"))?;

        self.server_repo.set_enabled(&server.id, true).await?;

        info!(
            space_id = %space_id,
            server_id = server_id,
            "[ServerAppService] Enabled server"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ServerEnabled {
            space_id,
            server_id: server_id.to_string(),
        });

        Ok(())
    }

    /// Disable a server
    ///
    /// Emits: `ServerDisabled`
    pub async fn disable(&self, space_id: Uuid, server_id: &str) -> Result<()> {
        let space_id_str = space_id.to_string();

        let server = self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not installed"))?;

        self.server_repo.set_enabled(&server.id, false).await?;

        info!(
            space_id = %space_id,
            server_id = server_id,
            "[ServerAppService] Disabled server"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::ServerDisabled {
            space_id,
            server_id: server_id.to_string(),
        });

        Ok(())
    }

    /// Update OAuth connected status
    pub async fn set_oauth_connected(
        &self,
        space_id: Uuid,
        server_id: &str,
        connected: bool,
    ) -> Result<()> {
        let space_id_str = space_id.to_string();

        let server = self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not installed"))?;

        self.server_repo
            .set_oauth_connected(&server.id, connected)
            .await?;

        info!(
            space_id = %space_id,
            server_id = server_id,
            connected = connected,
            "[ServerAppService] Updated OAuth status"
        );

        Ok(())
    }
}
