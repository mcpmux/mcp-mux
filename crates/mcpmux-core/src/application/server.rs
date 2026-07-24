//! Server Application Service
//!
//! Manages server installation and configuration with automatic event emission.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::{
    config::UserServerEntry, DomainEvent, InstallationSource, InstalledServer, ServerDefinition,
    ServerSource, UpdatePolicy,
};
use crate::event_bus::EventSender;
use crate::repository::{CredentialRepository, InstalledServerRepository, ServerFeatureRepository};

/// Application service for server installation and management
pub struct ServerAppService {
    server_repo: Arc<dyn InstalledServerRepository>,
    feature_repo: Option<Arc<dyn ServerFeatureRepository>>,
    credential_repo: Option<Arc<dyn CredentialRepository>>,
    event_sender: EventSender,
}

impl ServerAppService {
    pub fn new(
        server_repo: Arc<dyn InstalledServerRepository>,
        feature_repo: Option<Arc<dyn ServerFeatureRepository>>,
        credential_repo: Option<Arc<dyn CredentialRepository>>,
        event_sender: EventSender,
    ) -> Self {
        Self {
            server_repo,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn update_config(
        &self,
        space_id: Uuid,
        server_id: &str,
        input_values: HashMap<String, String>,
        env_overrides: Option<HashMap<String, String>>,
        args_append: Option<Vec<String>>,
        extra_headers: Option<HashMap<String, String>>,
        update_policy: Option<UpdatePolicy>,
        pinned_version: Option<String>,
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
        if let Some(policy) = update_policy {
            server.update_policy = policy;
        }
        if let Some(version) = pinned_version {
            server.pinned_version = Some(version);
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

    /// Set (or clear) the display name override for an installed server.
    ///
    /// Empty/whitespace values clear the override. Emits `ServerConfigUpdated`.
    pub async fn set_display_name_override(
        &self,
        space_id: Uuid,
        server_id: &str,
        value: Option<String>,
    ) -> Result<InstalledServer> {
        let space_id_str = space_id.to_string();

        let server = self
            .server_repo
            .get_by_server_id(&space_id_str, server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not installed"))?;

        let normalized = value
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        self.server_repo
            .set_display_name_override(&server.id, normalized.clone())
            .await?;

        info!(
            space_id = %space_id,
            server_id = server_id,
            has_override = normalized.is_some(),
            "[ServerAppService] Updated display name override"
        );

        self.event_sender.emit(DomainEvent::ServerConfigUpdated {
            space_id,
            server_id: server_id.to_string(),
        });

        let mut updated = server;
        updated.display_name_override = normalized;
        Ok(updated)
    }

    /// Clone an existing installed server into a new server ID with the given suffix.
    ///
    /// Emits: `ServerInstalled`
    pub async fn clone_server(
        &self,
        space_id: Uuid,
        source_server_id: &str,
        suffix: &str,
        alias_override: Option<&str>,
        display_name_override: Option<&str>,
    ) -> Result<InstalledServer> {
        let space_id_str = space_id.to_string();
        let new_server_id = Self::derive_clone_server_id(source_server_id, suffix)?;

        let source = self
            .server_repo
            .get_by_server_id(&space_id_str, source_server_id)
            .await?
            .ok_or_else(|| anyhow!("Source server not installed"))?;

        if self
            .server_repo
            .get_by_server_id(&space_id_str, &new_server_id)
            .await?
            .is_some()
        {
            return Err(anyhow!("Clone server ID already exists in this space"));
        }

        let mut definition = source
            .get_definition()
            .ok_or_else(|| anyhow!("Source server has no cached definition"))?;

        let normalized_suffix = UserServerEntry::normalize_server_id(suffix);
        let alias = alias_override
            .map(UserServerEntry::normalize_alias)
            .unwrap_or_else(|| normalized_suffix.clone());

        definition.id = new_server_id.clone();
        definition.name = format!("{} ({})", source.display_name(), normalized_suffix);
        definition.alias = Some(alias);
        definition.source = ServerSource::ManualEntry;

        let mut server = InstalledServer::new(&space_id_str, &new_server_id)
            .with_definition(&definition)
            .with_source(InstallationSource::ManualEntry)
            .with_cloned_from(source_server_id)
            .with_display_name_override(display_name_override)
            .with_update_policy(source.update_policy)
            .with_pinned_version(source.pinned_version.clone())
            .with_inputs(source.input_values.clone())
            .with_enabled(false);
        server.extra_headers = source.extra_headers.clone();

        self.server_repo.install(&server).await?;

        info!(
            space_id = %space_id,
            source_server_id = source_server_id,
            server_id = %new_server_id,
            "[ServerAppService] Cloned server"
        );

        let event_name = server
            .display_name_override
            .clone()
            .unwrap_or_else(|| definition.name.clone());

        self.event_sender.emit(DomainEvent::ServerInstalled {
            space_id,
            server_id: new_server_id.clone(),
            server_name: event_name,
        });

        Ok(server)
    }

    /// Return whether a suffixed clone ID is available in the given space.
    pub async fn is_clone_id_available(
        &self,
        space_id: Uuid,
        source_server_id: &str,
        suffix: &str,
    ) -> Result<bool> {
        let space_id_str = space_id.to_string();
        let new_server_id = match Self::derive_clone_server_id(source_server_id, suffix) {
            Ok(id) => id,
            Err(_) => return Ok(false),
        };

        Ok(self
            .server_repo
            .get_by_server_id(&space_id_str, &new_server_id)
            .await?
            .is_none())
    }

    /// List installed servers in a space that were cloned from the given source.
    pub async fn list_clone_dependents(
        &self,
        space_id: &str,
        source_server_id: &str,
    ) -> Result<Vec<InstalledServer>> {
        let servers = self.server_repo.list_for_space(space_id).await?;
        Ok(servers
            .into_iter()
            .filter(|server| server.cloned_from.as_deref() == Some(source_server_id))
            .collect())
    }

    /// Suggest the first available default suffix for cloning a server.
    pub async fn suggest_clone_suffix(
        &self,
        space_id: Uuid,
        source_server_id: &str,
    ) -> Result<String> {
        const DEFAULT_SUFFIXES: &[&str] = &["work", "personal", "prod", "staging"];

        for suffix in DEFAULT_SUFFIXES {
            if self
                .is_clone_id_available(space_id, source_server_id, suffix)
                .await?
            {
                return Ok((*suffix).to_string());
            }
        }

        for index in 2..100 {
            let suffix = index.to_string();
            if self
                .is_clone_id_available(space_id, source_server_id, &suffix)
                .await?
            {
                return Ok(suffix);
            }
        }

        Err(anyhow!("No available clone suffix"))
    }

    /// Derive the normalized clone server ID from a base install ID and user suffix.
    fn derive_clone_server_id(base_server_id: &str, suffix: &str) -> Result<String> {
        let normalized_suffix = UserServerEntry::normalize_server_id(suffix);
        if normalized_suffix.is_empty() {
            return Err(anyhow!("Clone suffix cannot be empty"));
        }

        let composite = format!("{base_server_id}-{normalized_suffix}");
        Ok(UserServerEntry::normalize_server_id(&composite))
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use crate::domain::{
        AuthConfig, InputDefinition, ServerSource, TransportConfig, TransportMetadata,
    };
    use crate::event_bus::EventBus;
    use crate::repository::InstalledServerRepository;

    /// Minimal in-memory repo for clone_server unit tests.
    struct InMemoryInstalledServerRepository {
        servers: RwLock<Vec<InstalledServer>>,
    }

    impl InMemoryInstalledServerRepository {
        fn new() -> Self {
            Self {
                servers: RwLock::new(Vec::new()),
            }
        }

        async fn seed(&self, server: InstalledServer) {
            self.servers.write().await.push(server);
        }

        async fn get_by_server_id(
            &self,
            space_id: &str,
            server_id: &str,
        ) -> Option<InstalledServer> {
            self.servers
                .read()
                .await
                .iter()
                .find(|s| s.space_id == space_id && s.server_id == server_id)
                .cloned()
        }
    }

    #[async_trait]
    impl InstalledServerRepository for InMemoryInstalledServerRepository {
        async fn list(&self) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self.servers.read().await.clone())
        }

        async fn list_for_space(
            &self,
            space_id: &str,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .await
                .iter()
                .filter(|s| s.space_id == space_id)
                .cloned()
                .collect())
        }

        async fn list_by_source_file(
            &self,
            _file_path: &std::path::Path,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(Vec::new())
        }

        async fn get(&self, id: &Uuid) -> crate::repository::RepoResult<Option<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .await
                .iter()
                .find(|s| &s.id == id)
                .cloned())
        }

        async fn get_by_server_id(
            &self,
            space_id: &str,
            server_id: &str,
        ) -> crate::repository::RepoResult<Option<InstalledServer>> {
            Ok(self.get_by_server_id(space_id, server_id).await)
        }

        async fn install(&self, server: &InstalledServer) -> crate::repository::RepoResult<()> {
            self.servers.write().await.push(server.clone());
            Ok(())
        }

        async fn update(&self, server: &InstalledServer) -> crate::repository::RepoResult<()> {
            let mut servers = self.servers.write().await;
            if let Some(existing) = servers.iter_mut().find(|s| s.id == server.id) {
                *existing = server.clone();
                Ok(())
            } else {
                anyhow::bail!("server not found for update: {}", server.server_id)
            }
        }

        async fn uninstall(&self, id: &Uuid) -> crate::repository::RepoResult<()> {
            self.servers.write().await.retain(|s| &s.id != id);
            Ok(())
        }

        async fn list_enabled(
            &self,
            space_id: &str,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .await
                .iter()
                .filter(|s| s.space_id == space_id && s.enabled)
                .cloned()
                .collect())
        }

        async fn list_enabled_all(&self) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .await
                .iter()
                .filter(|s| s.enabled)
                .cloned()
                .collect())
        }

        async fn set_enabled(&self, id: &Uuid, enabled: bool) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().await.iter_mut().find(|s| &s.id == id) {
                server.enabled = enabled;
            }
            Ok(())
        }

        async fn set_oauth_connected(
            &self,
            id: &Uuid,
            connected: bool,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().await.iter_mut().find(|s| &s.id == id) {
                server.oauth_connected = connected;
            }
            Ok(())
        }

        async fn update_inputs(
            &self,
            id: &Uuid,
            input_values: HashMap<String, String>,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().await.iter_mut().find(|s| &s.id == id) {
                server.input_values = input_values;
            }
            Ok(())
        }

        async fn update_cached_definition(
            &self,
            id: &Uuid,
            server_name: Option<String>,
            cached_definition: Option<String>,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().await.iter_mut().find(|s| &s.id == id) {
                server.server_name = server_name;
                server.cached_definition = cached_definition;
            }
            Ok(())
        }

        async fn set_display_name_override(
            &self,
            id: &Uuid,
            value: Option<String>,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().await.iter_mut().find(|s| &s.id == id) {
                server.display_name_override = value;
            }
            Ok(())
        }

        async fn update_version_cache(
            &self,
            id: &Uuid,
            latest_available_version: Option<String>,
            current_version: Option<String>,
            version_checked_at: chrono::DateTime<Utc>,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().await.iter_mut().find(|s| &s.id == id) {
                server.latest_available_version = latest_available_version;
                server.current_version = current_version;
                server.version_checked_at = Some(version_checked_at);
            }
            Ok(())
        }
    }

    fn user_space_http_definition(server_id: &str) -> ServerDefinition {
        ServerDefinition {
            id: server_id.to_string(),
            name: "PostHog Personal".to_string(),
            description: None,
            alias: Some("posthog".to_string()),
            auth: Some(AuthConfig::ApiKey {
                instructions: None,
            }),
            icon: None,
            transport: TransportConfig::Http {
                url: "https://mcp.posthog.com/mcp".to_string(),
                headers: HashMap::new(),
                metadata: TransportMetadata {
                    inputs: vec![InputDefinition {
                        id: "POSTHOG_API_KEY".to_string(),
                        label: "API Key".to_string(),
                        r#type: "password".to_string(),
                        required: true,
                        secret: true,
                        description: None,
                        default: None,
                        placeholder: None,
                        obtain_url: None,
                        obtain_instructions: None,
                    }],
                },
            },
            categories: vec![],
            publisher: None,
            source: ServerSource::UserSpace {
                space_id: "space-1".to_string(),
                file_path: PathBuf::from("/tmp/posthog.json"),
            },
            badges: vec![],
            hosting_type: Default::default(),
            license: None,
            license_url: None,
            installation: None,
            capabilities: None,
            sponsored: None,
            media: None,
            changelog_url: None,
        }
    }

    #[tokio::test]
    async fn clone_server_rewrites_source_and_seeds_auth_headers() {
        let space_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryInstalledServerRepository::new());
        let event_bus = EventBus::new();

        let parent_headers = HashMap::from([
            (
                "Authorization".to_string(),
                "Bearer phx_parent_token".to_string(),
            ),
            (
                "x-posthog-project-id".to_string(),
                "345911".to_string(),
            ),
        ]);
        let parent_inputs = HashMap::from([(
            "POSTHOG_API_KEY".to_string(),
            "phc_parent_key".to_string(),
        )]);

        let definition = user_space_http_definition("posthog-personal");
        let mut source = InstalledServer::new(space_id.to_string(), "posthog-personal")
            .with_definition(&definition)
            .with_source(InstallationSource::UserConfig {
                file_path: PathBuf::from("/tmp/posthog.json"),
            })
            .with_inputs(parent_inputs.clone());
        source.extra_headers = parent_headers.clone();
        repo.seed(source).await;

        let service = ServerAppService::new(
            repo.clone(),
            None,
            None,
            event_bus.sender(),
        );

        let cloned = service
            .clone_server(space_id, "posthog-personal", "mesh", None, None)
            .await
            .expect("clone should succeed");

        assert_eq!(cloned.source, InstallationSource::ManualEntry);
        assert_eq!(cloned.extra_headers, parent_headers);
        assert_eq!(cloned.input_values, parent_inputs);

        let definition = cloned
            .get_definition()
            .expect("clone should cache definition");
        assert!(
            !matches!(definition.source, ServerSource::UserSpace { .. }),
            "cloned definition source must not remain UserSpace"
        );
        assert_eq!(definition.source, ServerSource::ManualEntry);

        let persisted = repo
            .get_by_server_id(&space_id.to_string(), "posthog-personal-mesh")
            .await
            .expect("cloned row should be persisted");
        assert_eq!(persisted.extra_headers, parent_headers);
    }
}
