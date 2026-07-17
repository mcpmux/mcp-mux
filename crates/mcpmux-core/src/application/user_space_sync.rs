//! User Space Sync Service
//!
//! Syncs servers from user space JSON configuration files into InstalledServer records.
//! This enables a unified connection flow regardless of server source (Registry vs UserConfig).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::domain::config::UserSpaceConfig;
use crate::domain::{InstallationSource, InstalledServer, ServerDefinition};
use crate::repository::InstalledServerRepository;

/// Result of a sync operation
#[derive(Debug, Default)]
pub struct SyncResult {
    /// Server IDs that were added
    pub added: Vec<String>,
    /// Server IDs that were updated
    pub updated: Vec<String>,
    /// Server IDs that were removed
    pub removed: Vec<String>,
    /// Server IDs adopted from a different installation source
    pub adopted: Vec<String>,
    /// Per-server sync failures as (server_id, error message)
    pub errors: Vec<(String, String)>,
}

impl SyncResult {
    /// Check if any changes were made
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty()
            || !self.updated.is_empty()
            || !self.removed.is_empty()
            || !self.adopted.is_empty()
    }

    /// Total number of changes
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.updated.len() + self.removed.len()
    }
}

/// Per-definition outcome inside the add/update sync loop.
enum SyncOutcome {
    Added,
    Updated,
    Adopted,
}

/// Service for syncing user space JSON config files to InstalledServer records
pub struct UserSpaceSyncService {
    installed_repo: Arc<dyn InstalledServerRepository>,
}

impl UserSpaceSyncService {
    /// Create a new sync service
    pub fn new(installed_repo: Arc<dyn InstalledServerRepository>) -> Self {
        Self { installed_repo }
    }

    /// Ensure no two user-config entries normalize to the same MCP server id.
    ///
    /// User-config keys are normalized into MCP-safe server ids; if two entries
    /// collapse to the same id the sync loop would update the same
    /// `InstalledServer` row and appear to overwrite the previous custom server.
    /// Reject that up front with a clear error instead of silently dropping one.
    fn ensure_unique_server_ids(definitions: &[ServerDefinition]) -> Result<()> {
        let mut seen_ids: HashMap<String, String> = HashMap::new();
        for definition in definitions {
            if let Some(first_name) =
                seen_ids.insert(definition.id.clone(), definition.name.clone())
            {
                anyhow::bail!(
                    "Multiple custom servers normalize to the same id '{}': '{}' and '{}'. Rename one mcpServers key to a distinct alphanumeric/hyphen/dot id.",
                    definition.id,
                    first_name,
                    definition.name
                );
            }
        }
        Ok(())
    }

    /// Sync servers from a user space JSON file into InstalledServer records
    ///
    /// This performs a 3-way diff:
    /// 1. Servers in file but not in DB → ADD
    /// 2. Servers in both file and DB → UPDATE (refresh cached_definition)
    /// 3. Servers in DB but not in file → REMOVE
    ///
    /// # Arguments
    /// * `space_id` - The space to sync servers into
    /// * `file_path` - Path to the user space JSON config file
    ///
    /// # Returns
    /// A `SyncResult` with lists of added, updated, and removed server IDs
    pub async fn sync_from_file(&self, space_id: &str, file_path: &Path) -> Result<SyncResult> {
        info!("Syncing servers from file: {:?}", file_path);

        // 1. Parse the JSON file
        let content = tokio::fs::read_to_string(file_path)
            .await
            .with_context(|| format!("Failed to read config file: {:?}", file_path))?;

        let config: UserSpaceConfig = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", file_path))?;

        // 2. Convert to ServerDefinitions
        let definitions = config.to_server_definitions(space_id, file_path.to_path_buf());

        // User-config keys are normalized into MCP-safe server IDs; reject two
        // entries that collapse to the same ID up front so the sync loop can't
        // silently overwrite one custom server with another.
        Self::ensure_unique_server_ids(&definitions)?;

        let file_server_ids: HashSet<String> = definitions.iter().map(|d| d.id.clone()).collect();

        debug!(
            "Found {} servers in config file: {:?}",
            definitions.len(),
            file_server_ids
        );

        // 3. Get existing servers from this file
        let existing = self
            .installed_repo
            .list_by_source_file(file_path)
            .await
            .with_context(|| "Failed to list existing servers from source file")?;

        let existing_map: std::collections::HashMap<String, InstalledServer> = existing
            .into_iter()
            .map(|s| (s.server_id.clone(), s))
            .collect();

        let existing_ids: HashSet<String> = existing_map.keys().cloned().collect();

        debug!(
            "Found {} existing servers from this file: {:?}",
            existing_ids.len(),
            existing_ids
        );

        let mut result = SyncResult::default();

        // 4. Add/Update servers from file
        for definition in definitions {
            let server_id = definition.id.clone();

            let outcome = if let Some(existing_server) = existing_map.get(&server_id) {
                let cached_def = serde_json::to_string(&definition).ok();
                self.installed_repo
                    .update_cached_definition(
                        &existing_server.id,
                        Some(definition.name.clone()),
                        cached_def,
                    )
                    .await
                    .map(|_| SyncOutcome::Updated)
            } else if let Some(mut other_source) = self
                .installed_repo
                .get_by_server_id(space_id, &server_id)
                .await
                .unwrap_or(None)
            {
                other_source.source = InstallationSource::UserConfig {
                    file_path: file_path.to_path_buf(),
                };
                other_source.cached_definition = serde_json::to_string(&definition).ok();
                other_source.server_name = Some(definition.name.clone());
                self.installed_repo
                    .update(&other_source)
                    .await
                    .map(|_| SyncOutcome::Adopted)
            } else {
                let installed = InstalledServer::new(space_id, &server_id)
                    .with_definition(&definition)
                    .with_source(InstallationSource::UserConfig {
                        file_path: file_path.to_path_buf(),
                    })
                    .with_enabled(true);

                self.installed_repo
                    .install(&installed)
                    .await
                    .map(|_| SyncOutcome::Added)
            };

            match outcome {
                Ok(SyncOutcome::Added) => {
                    info!("Added server from user config: {}", server_id);
                    result.added.push(server_id);
                }
                Ok(SyncOutcome::Updated) => {
                    debug!("Updated server: {}", server_id);
                    result.updated.push(server_id);
                }
                Ok(SyncOutcome::Adopted) => {
                    info!("Adopted server from another source: {}", server_id);
                    result.adopted.push(server_id);
                }
                Err(e) => result.errors.push((server_id, e.to_string())),
            }
        }

        if result.added.is_empty()
            && result.updated.is_empty()
            && result.adopted.is_empty()
            && !result.errors.is_empty()
        {
            anyhow::bail!(
                "All {} servers failed to sync: {:?}",
                result.errors.len(),
                result.errors
            );
        }

        // 5. Remove servers no longer in file
        for (server_id, existing_server) in &existing_map {
            if !file_server_ids.contains(server_id) {
                self.installed_repo
                    .uninstall(&existing_server.id)
                    .await
                    .with_context(|| format!("Failed to uninstall server: {}", server_id))?;

                info!("Removed server no longer in config: {}", server_id);
                result.removed.push(server_id.clone());
            }
        }

        if result.has_changes() {
            info!(
                "Sync complete: {} added, {} updated, {} removed",
                result.added.len(),
                result.updated.len(),
                result.removed.len()
            );
        } else {
            debug!("Sync complete: no changes");
        }

        Ok(result)
    }

    /// Remove all servers that were installed from a specific file
    ///
    /// Used when a config file is deleted or explicitly unloaded.
    pub async fn remove_all_from_file(&self, file_path: &Path) -> Result<Vec<String>> {
        info!("Removing all servers from file: {:?}", file_path);

        let servers = self
            .installed_repo
            .list_by_source_file(file_path)
            .await
            .with_context(|| "Failed to list servers from source file")?;

        let mut removed = Vec::new();

        for server in servers {
            self.installed_repo
                .uninstall(&server.id)
                .await
                .with_context(|| format!("Failed to uninstall server: {}", server.server_id))?;

            info!("Removed server: {}", server.server_id);
            removed.push(server.server_id);
        }

        Ok(removed)
    }

    /// Check if a file path is already being tracked as a source
    pub async fn is_file_tracked(&self, file_path: &Path) -> Result<bool> {
        let servers = self.installed_repo.list_by_source_file(file_path).await?;

        Ok(!servers.is_empty())
    }

    /// Get all servers from a specific source file
    pub async fn get_servers_from_file(&self, file_path: &Path) -> Result<Vec<InstalledServer>> {
        self.installed_repo
            .list_by_source_file(file_path)
            .await
            .with_context(|| format!("Failed to list servers from file: {:?}", file_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashSet;
    use tempfile::tempdir;
    use tokio::sync::RwLock;
    use uuid::Uuid;

    #[test]
    fn test_sync_result_has_changes() {
        let mut result = SyncResult::default();
        assert!(!result.has_changes());

        result.added.push("test".to_string());
        assert!(result.has_changes());

        let mut adopted_only = SyncResult::default();
        adopted_only.adopted.push("adopted".to_string());
        assert!(adopted_only.has_changes());
    }

    #[test]
    fn test_sync_result_total_changes() {
        let mut result = SyncResult::default();
        result.added.push("a".to_string());
        result.updated.push("b".to_string());
        result.removed.push("c".to_string());

        assert_eq!(result.total_changes(), 3);
    }

    fn definitions_from(json: &str) -> Vec<ServerDefinition> {
        let config: UserSpaceConfig = serde_json::from_str(json).expect("valid config json");
        config.to_server_definitions("space-1", std::path::PathBuf::from("test.json"))
    }

    #[test]
    fn ensure_unique_server_ids_rejects_colliding_normalized_ids() {
        // "My Server" and "my_server" both normalize to "myserver".
        let definitions = definitions_from(
            r#"{ "mcpServers": {
                "My Server": { "command": "echo" },
                "my_server": { "command": "echo" }
            } }"#,
        );

        let err = UserSpaceSyncService::ensure_unique_server_ids(&definitions)
            .expect_err("colliding normalized ids must be rejected");
        assert!(
            err.to_string().contains("myserver"),
            "error should name the colliding id, got: {err}"
        );
    }

    #[test]
    fn ensure_unique_server_ids_accepts_distinct_ids() {
        let definitions = definitions_from(
            r#"{ "mcpServers": {
                "alpha": { "command": "echo" },
                "beta": { "command": "echo" }
            } }"#,
        );

        assert!(UserSpaceSyncService::ensure_unique_server_ids(&definitions).is_ok());
    }

    /// In-memory `InstalledServerRepository` for sync unit tests.
    struct InMemoryInstalledServerRepository {
        servers: RwLock<Vec<InstalledServer>>,
        fail_server_ids: RwLock<HashSet<String>>,
    }

    impl InMemoryInstalledServerRepository {
        fn new() -> Self {
            Self {
                servers: RwLock::new(Vec::new()),
                fail_server_ids: RwLock::new(HashSet::new()),
            }
        }

        async fn seed(&self, server: InstalledServer) {
            self.servers.write().await.push(server);
        }

        async fn set_fail_for(&self, server_ids: &[&str]) {
            let mut fail_ids = self.fail_server_ids.write().await;
            fail_ids.clear();
            fail_ids.extend(server_ids.iter().map(|id| (*id).to_string()));
        }

        async fn get_server_by_server_id(
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

        async fn should_fail(&self, server_id: &str) -> bool {
            self.fail_server_ids.read().await.contains(server_id)
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
            file_path: &Path,
        ) -> crate::repository::RepoResult<Vec<InstalledServer>> {
            Ok(self
                .servers
                .read()
                .await
                .iter()
                .filter(|s| s.source_file_path() == Some(&file_path.to_path_buf()))
                .cloned()
                .collect())
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
            Ok(self.get_server_by_server_id(space_id, server_id).await)
        }

        async fn install(&self, server: &InstalledServer) -> crate::repository::RepoResult<()> {
            if self.should_fail(&server.server_id).await {
                anyhow::bail!("simulated install failure for {}", server.server_id);
            }
            self.servers.write().await.push(server.clone());
            Ok(())
        }

        async fn update(&self, server: &InstalledServer) -> crate::repository::RepoResult<()> {
            if self.should_fail(&server.server_id).await {
                anyhow::bail!("simulated update failure for {}", server.server_id);
            }
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
            input_values: std::collections::HashMap<String, String>,
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
            let server_id = self
                .servers
                .read()
                .await
                .iter()
                .find(|s| &s.id == id)
                .map(|s| s.server_id.clone())
                .unwrap_or_default();
            if self.should_fail(&server_id).await {
                anyhow::bail!("simulated update_cached_definition failure for {server_id}");
            }
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

    async fn write_config_file(json: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("space-config.json");
        tokio::fs::write(&path, json)
            .await
            .expect("write config file");
        (dir, path)
    }

    #[tokio::test]
    async fn sync_from_file_adopts_cross_source_id_collision() {
        let repo = Arc::new(InMemoryInstalledServerRepository::new());
        let (_dir, file_path) = write_config_file(
            r#"{ "mcpServers": {
                "home-assistant": { "command": "echo", "name": "Home Assistant Config" }
            } }"#,
        )
        .await;

        repo.seed(
            InstalledServer::new("space-1", "home-assistant")
                .with_source(InstallationSource::Registry),
        )
        .await;

        let service = UserSpaceSyncService::new(repo.clone());
        let result = service
            .sync_from_file("space-1", &file_path)
            .await
            .expect("sync should succeed via adoption");

        assert_eq!(result.adopted, vec!["home-assistant".to_string()]);
        assert!(result.added.is_empty());
        assert!(result.errors.is_empty());

        let adopted = repo
            .get_server_by_server_id("space-1", "home-assistant")
            .await
            .expect("adopted server should exist");
        assert_eq!(
            adopted.source,
            InstallationSource::UserConfig {
                file_path: file_path.clone()
            }
        );
        assert_eq!(
            adopted.server_name.as_deref(),
            Some("Home Assistant Config")
        );
    }

    #[tokio::test]
    async fn sync_from_file_continues_past_one_failure() {
        let repo = Arc::new(InMemoryInstalledServerRepository::new());
        repo.set_fail_for(&["broken-server"]).await;
        let (_dir, file_path) = write_config_file(
            r#"{ "mcpServers": {
                "good-server": { "command": "echo" },
                "broken-server": { "command": "false" },
                "another-good": { "command": "echo" }
            } }"#,
        )
        .await;

        let service = UserSpaceSyncService::new(repo);
        let result = service
            .sync_from_file("space-1", &file_path)
            .await
            .expect("partial success should return Ok");

        assert_eq!(result.added.len(), 2);
        assert!(result.added.contains(&"good-server".to_string()));
        assert!(result.added.contains(&"another-good".to_string()));
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].0, "broken-server");
        assert!(
            result.errors[0].1.contains("simulated install failure"),
            "unexpected error: {}",
            result.errors[0].1
        );
    }

    #[tokio::test]
    async fn sync_from_file_errors_when_every_item_fails() {
        let repo = Arc::new(InMemoryInstalledServerRepository::new());
        repo.set_fail_for(&["alpha", "beta"]).await;
        let (_dir, file_path) = write_config_file(
            r#"{ "mcpServers": {
                "alpha": { "command": "echo" },
                "beta": { "command": "echo" }
            } }"#,
        )
        .await;

        let service = UserSpaceSyncService::new(repo);
        let err = service
            .sync_from_file("space-1", &file_path)
            .await
            .expect_err("all-fail sync should bail");

        assert!(
            err.to_string().contains("All 2 servers failed to sync"),
            "unexpected error: {err}"
        );
    }
}
