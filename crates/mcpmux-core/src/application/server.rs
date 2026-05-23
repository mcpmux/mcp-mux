//! Server Application Service
//!
//! Manages server installation and configuration with automatic event emission.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::{DomainEvent, InstallationSource, InstalledServer, ServerDefinition, UserServerEntry};
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

    /// Clone an installed server into a new manual-entry install in the same space.
    ///
    /// Copies the source `cached_definition`, assigns a suffixed `server_id`, clears credentials,
    /// and records lineage in `cloned_from`. Emits: `ServerInstalled`
    pub async fn clone_server(
        &self,
        space_id: Uuid,
        source_server_id: &str,
        suffix: &str,
        alias_override: Option<&str>,
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

        let server = InstalledServer::new(&space_id_str, &new_server_id)
            .with_definition(&definition)
            .with_source(InstallationSource::ManualEntry)
            .with_cloned_from(source_server_id)
            .with_enabled(false);

        self.server_repo.install(&server).await?;

        info!(
            space_id = %space_id,
            source_server_id = source_server_id,
            server_id = %new_server_id,
            "[ServerAppService] Cloned server"
        );

        self.event_sender.emit(DomainEvent::ServerInstalled {
            space_id,
            server_id: new_server_id.clone(),
            server_name: definition.name.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::RwLock;

    use crate::domain::{ServerSource, TransportConfig, TransportMetadata};
    use crate::event_bus::EventBus;
    use crate::repository::InstalledServerRepository;

    struct InMemoryInstalledServerRepo {
        servers: RwLock<HashMap<Uuid, InstalledServer>>,
    }

    impl InMemoryInstalledServerRepo {
        fn new() -> Self {
            Self {
                servers: RwLock::new(HashMap::new()),
            }
        }

        fn with_server(self, server: InstalledServer) -> Self {
            self.servers.write().unwrap().insert(server.id, server);
            self
        }
    }

    #[async_trait]
    impl InstalledServerRepository for InMemoryInstalledServerRepo {
        async fn list(&self) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self.servers.read().unwrap().values().cloned().collect())
        }

        async fn list_for_space(
            &self,
            space_id: &str,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .unwrap()
                .values()
                .filter(|server| server.space_id == space_id)
                .cloned()
                .collect())
        }

        async fn list_by_source_file(
            &self,
            _file_path: &std::path::Path,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(vec![])
        }

        async fn get(&self, id: &Uuid) -> crate::repository::RepoResult<Option<InstalledServer>> {
            Ok(self.servers.read().unwrap().get(id).cloned())
        }

        async fn get_by_server_id(
            &self,
            space_id: &str,
            server_id: &str,
        ) -> crate::repository::RepoResult<Option<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .unwrap()
                .values()
                .find(|server| server.space_id == space_id && server.server_id == server_id)
                .cloned())
        }

        async fn install(&self, server: &InstalledServer) -> crate::repository::RepoResult<()> {
            self.servers
                .write()
                .unwrap()
                .insert(server.id, server.clone());
            Ok(())
        }

        async fn update(&self, server: &InstalledServer) -> crate::repository::RepoResult<()> {
            self.servers
                .write()
                .unwrap()
                .insert(server.id, server.clone());
            Ok(())
        }

        async fn uninstall(&self, id: &Uuid) -> crate::repository::RepoResult<()> {
            self.servers.write().unwrap().remove(id);
            Ok(())
        }

        async fn list_enabled(
            &self,
            space_id: &str,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .unwrap()
                .values()
                .filter(|server| server.space_id == space_id && server.enabled)
                .cloned()
                .collect())
        }

        async fn list_enabled_all(&self) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .unwrap()
                .values()
                .filter(|server| server.enabled)
                .cloned()
                .collect())
        }

        async fn set_enabled(&self, id: &Uuid, enabled: bool) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().unwrap().get_mut(id) {
                server.enabled = enabled;
            }
            Ok(())
        }

        async fn set_oauth_connected(
            &self,
            id: &Uuid,
            connected: bool,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().unwrap().get_mut(id) {
                server.oauth_connected = connected;
            }
            Ok(())
        }

        async fn update_inputs(
            &self,
            id: &Uuid,
            input_values: HashMap<String, String>,
        ) -> crate::repository::RepoResult<()> {
            if let Some(server) = self.servers.write().unwrap().get_mut(id) {
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
            if let Some(server) = self.servers.write().unwrap().get_mut(id) {
                server.server_name = server_name;
                server.cached_definition = cached_definition;
            }
            Ok(())
        }
    }

    fn sample_definition(server_id: &str, name: &str) -> ServerDefinition {
        ServerDefinition {
            id: server_id.to_string(),
            name: name.to_string(),
            description: None,
            alias: None,
            auth: None,
            icon: None,
            transport: TransportConfig::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "posthog-mcp".to_string()],
                env: HashMap::new(),
                metadata: TransportMetadata::default(),
            },
            categories: vec![],
            publisher: None,
            source: ServerSource::Bundled,
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

    fn build_service(repo: Arc<dyn InstalledServerRepository>) -> ServerAppService {
        ServerAppService::new(repo, None, None, EventBus::new().sender())
    }

    #[tokio::test]
    async fn clone_server_happy_path() {
        let space_id = Uuid::new_v4();
        let space_id_str = space_id.to_string();
        let source = InstalledServer::new(&space_id_str, "posthog")
            .with_definition(&sample_definition("posthog", "PostHog"))
            .with_input("API_KEY", "secret");

        let repo = Arc::new(InMemoryInstalledServerRepo::new().with_server(source));
        let service = build_service(repo.clone());

        let cloned = service
            .clone_server(space_id, "posthog", "work", None)
            .await
            .expect("clone should succeed");

        assert_eq!(cloned.server_id, "posthog-work");
        assert_eq!(cloned.cloned_from.as_deref(), Some("posthog"));
        assert_eq!(cloned.source, InstallationSource::ManualEntry);
        assert!(!cloned.enabled);
        assert!(cloned.input_values.is_empty());
        assert_eq!(cloned.server_name.as_deref(), Some("PostHog (work)"));

        let definition = cloned.get_definition().expect("definition cached");
        assert_eq!(definition.id, "posthog-work");
        assert_eq!(definition.alias.as_deref(), Some("work"));

        let stored = repo
            .get_by_server_id(&space_id_str, "posthog-work")
            .await
            .expect("repo lookup")
            .expect("clone persisted");
        assert_eq!(stored.cloned_from.as_deref(), Some("posthog"));
    }

    #[tokio::test]
    async fn clone_server_rejects_collision() {
        let space_id = Uuid::new_v4();
        let space_id_str = space_id.to_string();
        let source = InstalledServer::new(&space_id_str, "posthog")
            .with_definition(&sample_definition("posthog", "PostHog"));
        let existing_clone = InstalledServer::new(&space_id_str, "posthog-work")
            .with_definition(&sample_definition("posthog-work", "PostHog (work)"));

        let repo = Arc::new(
            InMemoryInstalledServerRepo::new()
                .with_server(source)
                .with_server(existing_clone),
        );
        let service = build_service(repo);

        let error = service
            .clone_server(space_id, "posthog", "work", None)
            .await
            .expect_err("collision should fail");

        assert!(error.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn clone_server_rejects_missing_source() {
        let space_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryInstalledServerRepo::new());
        let service = build_service(repo);

        let error = service
            .clone_server(space_id, "posthog", "work", None)
            .await
            .expect_err("missing source should fail");

        assert!(error.to_string().contains("Source server not installed"));
    }

    #[tokio::test]
    async fn clone_server_normalizes_suffix_without_underscores() {
        let space_id = Uuid::new_v4();
        let space_id_str = space_id.to_string();
        let source = InstalledServer::new(&space_id_str, "posthog")
            .with_definition(&sample_definition("posthog", "PostHog"));

        let repo = Arc::new(InMemoryInstalledServerRepo::new().with_server(source));
        let service = build_service(repo);

        let cloned = service
            .clone_server(space_id, "posthog", "my_work", None)
            .await
            .expect("clone should succeed");

        assert_eq!(cloned.server_id, "posthog-mywork");
        assert_eq!(
            cloned.get_definition()
                .and_then(|definition| definition.alias),
            Some("mywork".to_string())
        );
    }

    #[tokio::test]
    async fn suggest_clone_suffix_skips_taken_ids() {
        let space_id = Uuid::new_v4();
        let space_id_str = space_id.to_string();
        let source = InstalledServer::new(&space_id_str, "posthog")
            .with_definition(&sample_definition("posthog", "PostHog"));
        let existing_clone = InstalledServer::new(&space_id_str, "posthog-work")
            .with_definition(&sample_definition("posthog-work", "PostHog (work)"));

        let repo = Arc::new(
            InMemoryInstalledServerRepo::new()
                .with_server(source)
                .with_server(existing_clone),
        );
        let service = build_service(repo);

        let suffix = service
            .suggest_clone_suffix(space_id, "posthog")
            .await
            .expect("suffix suggestion");

        assert_eq!(suffix, "personal");
    }
}
