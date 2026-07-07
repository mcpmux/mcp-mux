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
use uuid::Uuid;

use mcpmux_core::application::{SyncResult, UserSpaceSyncService};
use mcpmux_core::InstalledServerRepository;

type SyncSuccessHandler = Arc<dyn Fn(&str, &SyncResult) + Send + Sync>;
type SyncErrorHandler = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Optional UI callbacks for background config sync.
pub struct SpaceFileWatcherEmitters {
    pub on_success: Option<SyncSuccessHandler>,
    pub on_error: Option<SyncErrorHandler>,
}

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
    /// * `default_space_id` - Fallback space ID when filename stem is not a UUID
    /// * `emitters` - Optional callbacks for sync success/failure
    pub fn new(
        spaces_dir: PathBuf,
        sync_service: Arc<UserSpaceSyncService>,
        default_space_id: String,
        emitters: SpaceFileWatcherEmitters,
    ) -> Result<Self> {
        // Ensure directory exists
        if !spaces_dir.exists() {
            std::fs::create_dir_all(&spaces_dir)?;
        }

        // Channel for file change events
        let (tx, rx) = mpsc::channel::<PathBuf>(100);

        // Spawn debounced handler
        let sync_clone = sync_service.clone();
        let space_id = default_space_id.clone();

        tokio::spawn(async move {
            Self::debounced_handler(rx, sync_clone, space_id, emitters).await;
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

    /// Resolve space ID from a config filename stem when it is a UUID.
    fn space_id_from_config_path(path: &Path, default_space_id: &str) -> String {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| Uuid::parse_str(stem).is_ok())
            .map(String::from)
            .unwrap_or_else(|| default_space_id.to_string())
    }

    /// Debounced handler for file changes
    ///
    /// Groups rapid file changes and syncs after a debounce period.
    async fn debounced_handler(
        mut rx: mpsc::Receiver<PathBuf>,
        sync_service: Arc<UserSpaceSyncService>,
        default_space_id: String,
        emitters: SpaceFileWatcherEmitters,
    ) {
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

                        let space_id =
                            Self::space_id_from_config_path(&path, &default_space_id);

                        info!("Syncing changes from: {:?}", path);

                        match sync_service.sync_from_file(&space_id, &path).await {
                            Ok(result) => {
                                if result.has_changes() {
                                    info!(
                                        "Sync complete: {} added, {} updated, {} removed",
                                        result.added.len(),
                                        result.updated.len(),
                                        result.removed.len()
                                    );

                                    if let Some(ref emitter) = emitters.on_success {
                                        emitter(&space_id, &result);
                                    }
                                } else {
                                    debug!("Sync complete: no changes");
                                }
                            }
                            Err(e) => {
                                let message = e.to_string();
                                error!("Sync failed for {:?}: {}", path, message);
                                if let Some(ref emitter) = emitters.on_error {
                                    emitter(&space_id, &message);
                                }
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
    default_space_id: String,
}

#[allow(dead_code)]
impl SpaceFileWatcherBuilder {
    /// Create a new builder
    pub fn new(spaces_dir: PathBuf, installed_repo: Arc<dyn InstalledServerRepository>) -> Self {
        Self {
            spaces_dir,
            installed_repo,
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
        SpaceFileWatcher::new(
            self.spaces_dir,
            sync_service,
            self.default_space_id,
            SpaceFileWatcherEmitters {
                on_success: None,
                on_error: None,
            },
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
            SpaceFileWatcherEmitters {
                on_success: Some(Arc::new(emitter)),
                on_error: None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn space_id_from_config_path_uses_uuid_stem() {
        let path = Path::new("/tmp/spaces/00000000-0000-0000-0000-000000000001.json");
        assert_eq!(
            SpaceFileWatcher::space_id_from_config_path(path, "fallback"),
            "00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn space_id_from_config_path_falls_back_for_non_uuid_stem() {
        let path = Path::new("/tmp/spaces/default.json");
        assert_eq!(
            SpaceFileWatcher::space_id_from_config_path(path, "fallback-id"),
            "fallback-id"
        );
    }
}
