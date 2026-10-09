//! File Watcher Service for User Space Config Files
//!
//! Watches user space JSON config files for changes and triggers sync.
//! Uses debouncing to avoid multiple syncs for rapid file changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use mcpmux_core::application::{SyncResult, UserSpaceSyncService};
use mcpmux_core::{InstalledServerRepository, SpaceRepository};

/// File watcher for user space configuration files
///
/// Monitors a directory for JSON file changes and syncs servers
/// from modified files using the UserSpaceSyncService.
pub struct SpaceFileWatcher {
    /// The watcher handle - kept alive to continue watching
    _watcher: RecommendedWatcher,
    /// Directory being watched (stored for builder API; construction uses it)
    #[allow(dead_code)]
    spaces_dir: PathBuf,
}

impl SpaceFileWatcher {
    /// Create a new file watcher for space config files
    ///
    /// # Arguments
    /// * `spaces_dir` - Directory containing space JSON config files
    /// * `sync_service` - Service to sync changes
    /// * `default_space_id` - Default space ID to use for synced servers
    /// * `space_repo` - Used to skip `<space-id>.json` files of Spaces that
    ///   don't exist
    /// * `event_emitter` - Optional callback to emit UI events after sync
    pub fn new<F>(
        spaces_dir: PathBuf,
        sync_service: Arc<UserSpaceSyncService>,
        default_space_id: String,
        space_repo: Arc<dyn SpaceRepository>,
        event_emitter: Option<F>,
    ) -> Result<Self>
    where
        F: Fn(&str, &SyncResult) + Send + Sync + 'static,
    {
        // Ensure directory exists
        if !spaces_dir.exists() {
            std::fs::create_dir_all(&spaces_dir)?;
        }

        // Channel for file change events
        let (tx, rx) = mpsc::channel::<PathBuf>(100);

        // Spawn debounced handler
        let sync_clone = sync_service.clone();
        let space_id = default_space_id.clone();
        let emitter = event_emitter.map(Arc::new);

        tokio::spawn(async move {
            Self::debounced_handler(rx, sync_clone, space_id, space_repo, emitter).await;
        });

        // Create file watcher
        let tx_clone = tx.clone();
        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Only handle modify/create events for JSON files
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        for path in event.paths {
                            if path.extension().is_some_and(|e| e == "json") {
                                debug!("File change detected: {:?}", path);
                                if let Err(e) = tx_clone.blocking_send(path) {
                                    warn!("Failed to send file change event: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("File watcher error: {}", e);
                }
            }
        })?;

        // Start watching the directory
        let mut watcher = watcher;
        watcher.watch(&spaces_dir, RecursiveMode::NonRecursive)?;

        info!("File watcher started for: {:?}", spaces_dir);

        Ok(Self {
            _watcher: watcher,
            spaces_dir,
        })
    }

    /// Debounced handler for file changes
    ///
    /// Groups rapid file changes and syncs after a debounce period.
    async fn debounced_handler<F>(
        mut rx: mpsc::Receiver<PathBuf>,
        sync_service: Arc<UserSpaceSyncService>,
        default_space_id: String,
        space_repo: Arc<dyn SpaceRepository>,
        event_emitter: Option<Arc<F>>,
    ) where
        F: Fn(&str, &SyncResult) + Send + Sync + 'static,
    {
        let debounce_duration = Duration::from_millis(500);
        let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

        loop {
            tokio::select! {
                // Receive new file change events
                Some(path) = rx.recv() => {
                    pending.insert(path, Instant::now());
                }
                // Check for debounced events
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    let now = Instant::now();
                    let ready: Vec<PathBuf> = pending
                        .iter()
                        .filter(|(_, time)| now.duration_since(**time) >= debounce_duration)
                        .map(|(path, _)| path.clone())
                        .collect();

                    for path in ready {
                        pending.remove(&path);

                        // Each Space's config lives in `<space-id>.json`, so a
                        // change syncs into that Space. Other names (legacy
                        // files) keep going to the default Space.
                        let space_id = match space_id_for_config_file(&path) {
                            Some(id) => {
                                // A file named after a Space that doesn't exist
                                // would install servers into no Space.
                                let exists = match uuid::Uuid::parse_str(&id) {
                                    Ok(uuid) => matches!(space_repo.get(&uuid).await, Ok(Some(_))),
                                    Err(_) => false,
                                };
                                if !exists {
                                    warn!(
                                        "Ignoring {:?}: no Space with that id exists",
                                        path
                                    );
                                    continue;
                                }
                                id
                            }
                            None => default_space_id.clone(),
                        };
                        let space_id = &space_id;

                        info!("Syncing changes from: {:?}", path);

                        match sync_service.sync_from_file(space_id, &path).await {
                            Ok(result) => {
                                if result.has_changes() {
                                    info!(
                                        "Sync complete: {} added, {} updated, {} removed",
                                        result.added.len(),
                                        result.updated.len(),
                                        result.removed.len()
                                    );

                                    // Emit event for UI refresh
                                    if let Some(ref emitter) = event_emitter {
                                        emitter(space_id, &result);
                                    }
                                } else {
                                    debug!("Sync complete: no changes");
                                }
                            }
                            Err(e) => {
                                error!("Sync failed for {:?}: {}", path, e);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get the directory being watched
    #[allow(dead_code)]
    pub fn spaces_dir(&self) -> &Path {
        &self.spaces_dir
    }
}

/// Builder for SpaceFileWatcher with fluent API
#[allow(dead_code)]
pub struct SpaceFileWatcherBuilder {
    spaces_dir: PathBuf,
    installed_repo: Arc<dyn InstalledServerRepository>,
    space_repo: Arc<dyn SpaceRepository>,
    default_space_id: String,
}

#[allow(dead_code)]
impl SpaceFileWatcherBuilder {
    /// Create a new builder
    pub fn new(
        spaces_dir: PathBuf,
        installed_repo: Arc<dyn InstalledServerRepository>,
        space_repo: Arc<dyn SpaceRepository>,
    ) -> Self {
        Self {
            spaces_dir,
            installed_repo,
            space_repo,
            default_space_id: "default".to_string(),
        }
    }

    /// Set the default space ID for synced servers
    pub fn with_default_space_id(mut self, space_id: impl Into<String>) -> Self {
        self.default_space_id = space_id.into();
        self
    }

    /// Build the file watcher without event emitter
    pub fn build(self) -> Result<SpaceFileWatcher> {
        let sync_service = Arc::new(UserSpaceSyncService::new(self.installed_repo));
        SpaceFileWatcher::new::<fn(&str, &SyncResult)>(
            self.spaces_dir,
            sync_service,
            self.default_space_id,
            self.space_repo,
            None,
        )
    }

    /// Build the file watcher with an event emitter
    pub fn build_with_emitter<F>(self, emitter: F) -> Result<SpaceFileWatcher>
    where
        F: Fn(&str, &SyncResult) + Send + Sync + 'static,
    {
        let sync_service = Arc::new(UserSpaceSyncService::new(self.installed_repo));
        SpaceFileWatcher::new(
            self.spaces_dir,
            sync_service,
            self.default_space_id,
            self.space_repo,
            Some(emitter),
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_builder_default_space_id() {
        // Just test the builder pattern compiles
        // Actual functionality tested via integration tests
    }
}

/// The Space a config file belongs to: its file stem when that is a Space id.
fn space_id_for_config_file(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    uuid::Uuid::parse_str(stem).ok().map(|id| id.to_string())
}

#[cfg(test)]
mod space_file_tests {
    use super::space_id_for_config_file;
    use std::path::Path;

    #[test]
    fn config_files_map_to_their_space() {
        let id = "6f2b1c9e-6a0e-4c1d-9a3e-1f2d3c4b5a69";
        assert_eq!(
            space_id_for_config_file(Path::new(&format!("/data/spaces/{id}.json"))).as_deref(),
            Some(id)
        );
        assert_eq!(
            space_id_for_config_file(Path::new("/data/spaces/default.json")),
            None
        );
    }
}
