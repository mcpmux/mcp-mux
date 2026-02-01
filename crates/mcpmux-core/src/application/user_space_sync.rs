//! User Space Sync Service
//!
//! Syncs servers from user space JSON configuration files into InstalledServer records.
//! This enables a unified connection flow regardless of server source (Registry vs UserConfig).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::domain::config::UserSpaceConfig;
use crate::domain::{InstallationSource, InstalledServer};
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
}
