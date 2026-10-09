//! User Space Sync Service
//!
//! Syncs servers from user space JSON configuration files into InstalledServer records.
//! This enables a unified connection flow regardless of server source (Registry vs UserConfig).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

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
}

impl SyncResult {
    /// Check if any changes were made
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.updated.is_empty() || !self.removed.is_empty()
    }

    /// Total number of changes
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.updated.len() + self.removed.len()
    }
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

    /// Report what [`Self::sync_from_file`] would change if `file_path` held
    /// `content`, without writing anything.
    ///
    /// Fails exactly where the real sync would before it touches storage:
    /// unparseable JSON or two keys that normalize to the same server id.
    /// Used for import dry-runs and to validate an import before the space
    /// file is replaced.
    pub async fn plan_from_content(
        &self,
        space_id: &str,
        file_path: &Path,
        content: &str,
    ) -> Result<SyncResult> {
        let (definitions, existing_map, _elsewhere) = self
            .parse_and_load_existing(space_id, file_path, content)
            .await?;
        let file_server_ids: HashSet<String> = definitions.iter().map(|d| d.id.clone()).collect();

        let mut result = SyncResult::default();
        for definition in definitions {
            if existing_map.contains_key(&definition.id) {
                result.updated.push(definition.id);
            } else {
                result.added.push(definition.id);
            }
        }
        result.removed = existing_map
            .into_keys()
            .filter(|id| !file_server_ids.contains(id))
            .collect();
        result.removed.sort();
        Ok(result)
    }

    /// Parse `content` as the user-space config stored at `file_path` and
    /// load the servers currently installed from that file into `space_id`,
    /// keyed by id. Rows installed from the same file into another Space
    /// (an older watcher put every Space's file into the default Space) are
    /// returned separately so the sync can move them.
    async fn parse_and_load_existing(
        &self,
        space_id: &str,
        file_path: &Path,
        content: &str,
    ) -> Result<(
        Vec<ServerDefinition>,
        HashMap<String, InstalledServer>,
        Vec<InstalledServer>,
    )> {
        let config: UserSpaceConfig = serde_json::from_str(content)
            .with_context(|| format!("Failed to parse config file: {:?}", file_path))?;

        let definitions = config.to_server_definitions(space_id, file_path.to_path_buf());

        // User-config keys are normalized into MCP-safe server IDs; reject two
        // entries that collapse to the same ID up front so the sync loop can't
        // silently overwrite one custom server with another.
        Self::ensure_unique_server_ids(&definitions)?;

        let existing = self
            .installed_repo
            .list_by_source_file(file_path)
            .await
            .with_context(|| "Failed to list existing servers from source file")?;

        let (existing, elsewhere): (Vec<_>, Vec<_>) =
            existing.into_iter().partition(|s| s.space_id == space_id);
        let existing_map = existing
            .into_iter()
            .map(|s| (s.server_id.clone(), s))
            .collect();

        Ok((definitions, existing_map, elsewhere))
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

        // 1. Read the JSON file
        let content = tokio::fs::read_to_string(file_path)
            .await
            .with_context(|| format!("Failed to read config file: {:?}", file_path))?;

        // 2-3. Parse into ServerDefinitions and load the servers already
        // installed from this file
        let (definitions, existing_map, elsewhere) = self
            .parse_and_load_existing(space_id, file_path, &content)
            .await?;

        // Rows from this file that sit in another Space are removed; the
        // servers still in the file are added below in the right Space.
        for stale in &elsewhere {
            warn!(
                server_id = %stale.server_id,
                installed_in = %stale.space_id,
                file_space = %space_id,
                "Moving a server installed from this file into the wrong Space"
            );
            self.installed_repo
                .uninstall(&stale.id)
                .await
                .with_context(|| format!("Failed to uninstall server: {}", stale.server_id))?;
        }

        let file_server_ids: HashSet<String> = definitions.iter().map(|d| d.id.clone()).collect();

        debug!(
            "Found {} servers in config file: {:?}",
            definitions.len(),
            file_server_ids
        );

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

            if let Some(existing_server) = existing_map.get(&server_id) {
                // Update: refresh cached_definition (config may have changed)
                let cached_def = serde_json::to_string(&definition).ok();
                self.installed_repo
                    .update_cached_definition(
                        &existing_server.id,
                        Some(definition.name.clone()),
                        cached_def,
                    )
                    .await
                    .with_context(|| format!("Failed to update server: {}", server_id))?;

                debug!("Updated server: {}", server_id);
                result.updated.push(server_id);
            } else {
                // Add: create new InstalledServer
                let installed = InstalledServer::new(space_id, &server_id)
                    .with_definition(&definition)
                    .with_source(InstallationSource::UserConfig {
                        file_path: file_path.to_path_buf(),
                    })
                    .with_enabled(true); // Auto-enable servers from user config

                self.installed_repo
                    .install(&installed)
                    .await
                    .with_context(|| format!("Failed to install server: {}", server_id))?;

                info!("Added server from user config: {}", server_id);
                result.added.push(server_id);
            }
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

    #[test]
    fn test_sync_result_has_changes() {
        let mut result = SyncResult::default();
        assert!(!result.has_changes());

        result.added.push("test".to_string());
        assert!(result.has_changes());
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
}
