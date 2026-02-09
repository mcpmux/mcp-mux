//! Server log manager - file-based logging per server

use crate::{LogConfig, LogLevel, ServerLog};
use anyhow::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

/// Server log manager
pub struct ServerLogManager {
    config: LogConfig,
    writers: Arc<RwLock<HashMap<String, Arc<Mutex<ServerLogWriter>>>>>,
}

impl ServerLogManager {
    /// Create a new log manager
    pub fn new(config: LogConfig) -> Self {
        Self {
            config,
            writers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Sanitize server ID for filesystem (replace invalid chars)
    /// Server IDs use colon (com.cloudflare:docs) but colons are invalid in Windows paths
    fn sanitize_server_id(server_id: &str) -> String {
        server_id.replace(':', "_")
    }

    /// Get or create a log writer for a server
    async fn get_writer(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> Result<Arc<Mutex<ServerLogWriter>>> {
        let key = format!("{}/{}", space_id, server_id);

        // Fast path: writer exists
        {
            let readers = self.writers.read().await;
            if let Some(writer) = readers.get(&key) {
                return Ok(writer.clone());
            }
        }

        // Slow path: create new writer
        let mut writers = self.writers.write().await;

        // Double-check (another thread might have created it)
        if let Some(writer) = writers.get(&key) {
            return Ok(writer.clone());
        }

        // Create log directory with sanitized server ID
        let safe_server_id = Self::sanitize_server_id(server_id);
        let log_dir = self.config.base_dir.join(space_id).join(safe_server_id);
        let _: () = tokio::fs::create_dir_all(&log_dir)
            .await
            .context("Failed to create log directory")?;

        let writer = Arc::new(Mutex::new(
            ServerLogWriter::new(log_dir, &self.config).await?,
        ));

        writers.insert(key, writer.clone());
        Ok(writer)
    }

    /// Append a log entry
    pub async fn append(&self, space_id: &str, server_id: &str, log: ServerLog) -> Result<()> {
        let writer = self.get_writer(space_id, server_id).await?;
        let mut w = writer.lock().await;
        w.write(log).await
    }

    /// Read recent logs (tail behavior)
    pub async fn read_logs(
        &self,
        space_id: &str,
        server_id: &str,
        limit: usize,
        level_filter: Option<LogLevel>,
    ) -> Result<Vec<ServerLog>> {
        let safe_server_id = Self::sanitize_server_id(server_id);
        let log_dir = self.config.base_dir.join(space_id).join(safe_server_id);
        let current_log = log_dir.join("current.log");

        if !current_log.exists() {
            return Ok(vec![]);
        }

        // Read file and parse JSON lines
        let content: String = tokio::fs::read_to_string(&current_log).await?;
        let mut logs: Vec<ServerLog> = content
            .lines()
            .rev() // Start from end (most recent)
            .take(limit * 2) // Take more in case of filtering
            .filter_map(|line| {
                serde_json::from_str(line)
                    .map_err(|e| {
                        debug!("Failed to parse log line: {}", e);
                        e
                    })
                    .ok()
            })
            .filter(|log: &ServerLog| level_filter.is_none_or(|lvl| log.level >= lvl))
            .take(limit)
            .collect();

        logs.reverse(); // Return in chronological order
        Ok(logs)
    }

    /// Clear logs for a server
    pub async fn clear_logs(&self, space_id: &str, server_id: &str) -> Result<()> {
        let key = format!("{}/{}", space_id, server_id);

        // Close writer if open
        {
            let mut writers = self.writers.write().await;
            writers.remove(&key);
        }

        // Remove log directory
        let safe_server_id = Self::sanitize_server_id(server_id);
        let log_dir = self.config.base_dir.join(space_id).join(safe_server_id);
        if log_dir.exists() {
            let _: () = tokio::fs::remove_dir_all(&log_dir)
                .await
                .context("Failed to remove log directory")?;
            info!("Cleared logs for server {}/{}", space_id, server_id);
        }

        Ok(())
    }

    /// Get log file path for a server
    pub fn get_log_file(&self, space_id: &str, server_id: &str) -> PathBuf {
        let safe_server_id = Self::sanitize_server_id(server_id);
        self.config
            .base_dir
            .join(space_id)
            .join(safe_server_id)
            .join("current.log")
    }

    /// Get the base log directory
    pub fn base_dir(&self) -> &Path {
        &self.config.base_dir
    }

    /// Remove log files older than `retention_days` across all spaces/servers.
    ///
    /// Walks the entire log directory tree and removes rotated log files
    /// (`.log` and `.log.gz`, excluding `current.log`) whose modification
    /// time is older than the retention period. Empty server directories
    /// are cleaned up afterwards.
    pub async fn cleanup_logs_older_than(&self, retention_days: u32) -> Result<usize> {
        if retention_days == 0 {
            debug!("[LogCleanup] Retention set to 0 (keep forever), skipping");
            return Ok(0);
        }

        let cutoff = SystemTime::now() - Duration::from_secs(u64::from(retention_days) * 86400);
        let base = &self.config.base_dir;

        if !base.exists() {
            return Ok(0);
        }

        let mut removed = 0usize;

        // Walk: base_dir / <space_id> / <server_id> / *.log|*.log.gz
        let mut spaces = tokio::fs::read_dir(base).await?;
        while let Some(space_entry) = spaces.next_entry().await? {
            let space_path = space_entry.path();
            if !space_path.is_dir() {
                // Also clean top-level app tracing logs (mcpmux.YYYY-MM-DD.log)
                if is_removable_log_file(&space_path) {
                    if let Ok(meta) = tokio::fs::metadata(&space_path).await {
                        if let Ok(modified) = meta.modified() {
                            if modified < cutoff {
                                if let Err(e) = tokio::fs::remove_file(&space_path).await {
                                    warn!("[LogCleanup] Failed to remove {:?}: {}", space_path, e);
                                } else {
                                    debug!("[LogCleanup] Removed app log: {:?}", space_path);
                                    removed += 1;
                                }
                            }
                        }
                    }
                }
                continue;
            }

            let mut servers = match tokio::fs::read_dir(&space_path).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            while let Some(server_entry) = servers.next_entry().await? {
                let server_path = server_entry.path();
                if !server_path.is_dir() {
                    continue;
                }

                let mut files = match tokio::fs::read_dir(&server_path).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                while let Some(file_entry) = files.next_entry().await? {
                    let file_path = file_entry.path();
                    if !is_removable_log_file(&file_path) {
                        continue;
                    }

                    if let Ok(meta) = file_entry.metadata().await {
                        if let Ok(modified) = meta.modified() {
                            if modified < cutoff {
                                if let Err(e) = tokio::fs::remove_file(&file_path).await {
                                    warn!("[LogCleanup] Failed to remove {:?}: {}", file_path, e);
                                } else {
                                    debug!("[LogCleanup] Removed: {:?}", file_path);
                                    removed += 1;
                                }
                            }
                        }
                    }
                }

                // Remove empty server directory
                if dir_is_empty(&server_path).await {
                    let _ = tokio::fs::remove_dir(&server_path).await;
                }
            }

            // Remove empty space directory
            if dir_is_empty(&space_path).await {
                let _ = tokio::fs::remove_dir(&space_path).await;
            }
        }

        if removed > 0 {
            info!(
                "[LogCleanup] Removed {} log file(s) older than {} days",
                removed, retention_days
            );
        }

        Ok(removed)
    }
}

/// Writer for a single server's logs
struct ServerLogWriter {
    log_dir: PathBuf,
    current_file: File,
    current_size: u64,
    max_file_size: u64,
    max_files: usize,
    compress: bool,
}

impl ServerLogWriter {
    async fn new(log_dir: PathBuf, config: &LogConfig) -> Result<Self> {
        let current_path = log_dir.join("current.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current_path)
            .await
            .context("Failed to open log file")?;

        let current_size = file.metadata().await?.len();

        Ok(Self {
            log_dir,
            current_file: file,
            current_size,
            max_file_size: config.max_file_size,
            max_files: config.max_files,
            compress: config.compress,
        })
    }

    async fn write(&mut self, log: ServerLog) -> Result<()> {
        // Serialize to JSON line
        let mut line = serde_json::to_string(&log).context("Failed to serialize log entry")?;
        line.push('\n');

        let line_len = line.len() as u64;

        // Check if we need to rotate
        if self.current_size + line_len > self.max_file_size {
            self.rotate().await?;
        }

        // Write line
        self.current_file.write_all(line.as_bytes()).await?;
        self.current_file.flush().await?;
        self.current_size += line_len;

        Ok(())
    }

    async fn rotate(&mut self) -> Result<()> {
        info!("Rotating log file in {:?}", self.log_dir);

        // Close current file
        self.current_file.shutdown().await?;

        // Rename current.log to timestamped file
        let current_path = self.log_dir.join("current.log");
        let timestamp = chrono::Utc::now().format("%Y-%m-%d-%H%M%S");
        let rotated_path = self.log_dir.join(format!("{}.log", timestamp));

        tokio::fs::rename(&current_path, &rotated_path).await?;

        // Compress in background
        if self.compress {
            let rotated_path_clone = rotated_path.clone();
            tokio::spawn(async move {
                if let Err(e) = compress_log_file(&rotated_path_clone).await {
                    warn!("Failed to compress log file: {}", e);
                }
            });
        }

        // Cleanup old files
        self.cleanup_old_files().await?;

        // Create new current.log
        self.current_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current_path)
            .await?;

        self.current_size = 0;

        Ok(())
    }

    async fn cleanup_old_files(&self) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.log_dir).await?;
        let mut log_files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".log.gz") || (name.ends_with(".log") && name != "current.log") {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            log_files.push((path, modified));
                        }
                    }
                }
            }
        }

        // Sort by modification time (oldest first)
        log_files.sort_by_key(|(_, modified)| *modified);

        // Remove oldest files if we exceed max_files
        if log_files.len() > self.max_files {
            let to_remove = log_files.len() - self.max_files;
            for (path, _) in log_files.iter().take(to_remove) {
                if let Err(e) = tokio::fs::remove_file(path).await {
                    warn!("Failed to remove old log file {:?}: {}", path, e);
                } else {
                    debug!("Removed old log file: {:?}", path);
                }
            }
        }

        Ok(())
    }
}

/// Check if a path is a removable log file (not `current.log`)
fn is_removable_log_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    name != "current.log" && (name.ends_with(".log") || name.ends_with(".log.gz"))
}

/// Check if a directory is empty
async fn dir_is_empty(path: &Path) -> bool {
    match tokio::fs::read_dir(path).await {
        Ok(mut rd) => rd.next_entry().await.ok().flatten().is_none(),
        Err(_) => false,
    }
}

/// Compress a log file using gzip
async fn compress_log_file(path: &Path) -> Result<()> {
    let gz_path = path.with_extension("log.gz");

    // Read original file
    let content = tokio::fs::read(path).await?;

    // Compress using blocking IO in a separate task
    let gz_path_clone = gz_path.clone();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::create(&gz_path_clone)?;
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(&content)?;
        encoder.finish()?;
        Ok::<_, anyhow::Error>(())
    })
    .await??;

    // Remove original file
    tokio::fs::remove_file(path).await?;

    info!("Compressed log file: {:?} -> {:?}", path, gz_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogSource;

    #[tokio::test]
    async fn test_log_manager_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = LogConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_file_size: 1024,
            max_files: 5,
            compress: false,
        };

        let manager = ServerLogManager::new(config);

        // Write some logs
        for i in 0..10 {
            let log = ServerLog::new(
                LogLevel::Info,
                LogSource::App,
                format!("Test message {}", i),
            );
            manager.append("space1", "server1", log).await.unwrap();
        }

        // Read logs
        let logs = manager
            .read_logs("space1", "server1", 5, None)
            .await
            .unwrap();
        assert_eq!(logs.len(), 5);
        assert_eq!(logs[0].message, "Test message 5");
        assert_eq!(logs[4].message, "Test message 9");
    }

    #[tokio::test]
    async fn test_log_level_filter() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = LogConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_file_size: 1024 * 1024,
            max_files: 5,
            compress: false,
        };

        let manager = ServerLogManager::new(config);

        // Write logs with different levels
        manager
            .append(
                "space1",
                "server1",
                ServerLog::new(LogLevel::Debug, LogSource::App, "Debug msg"),
            )
            .await
            .unwrap();
        manager
            .append(
                "space1",
                "server1",
                ServerLog::new(LogLevel::Info, LogSource::App, "Info msg"),
            )
            .await
            .unwrap();
        manager
            .append(
                "space1",
                "server1",
                ServerLog::new(LogLevel::Warn, LogSource::App, "Warn msg"),
            )
            .await
            .unwrap();
        manager
            .append(
                "space1",
                "server1",
                ServerLog::new(LogLevel::Error, LogSource::App, "Error msg"),
            )
            .await
            .unwrap();

        // Filter by warn and above
        let logs = manager
            .read_logs("space1", "server1", 10, Some(LogLevel::Warn))
            .await
            .unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].message, "Warn msg");
        assert_eq!(logs[1].message, "Error msg");
    }

    #[tokio::test]
    async fn test_cleanup_logs_older_than() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = LogConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_file_size: 1024 * 1024,
            max_files: 100,
            compress: false,
        };
        let manager = ServerLogManager::new(config);

        // Create server log directory
        let server_dir = temp_dir.path().join("space1").join("server1");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();

        // Create a current.log (should never be removed)
        tokio::fs::write(server_dir.join("current.log"), b"current")
            .await
            .unwrap();

        // Create an old rotated log file
        let old_log = server_dir.join("2020-01-01-000000.log");
        tokio::fs::write(&old_log, b"old data").await.unwrap();

        // Create an old compressed log file
        let old_gz = server_dir.join("2020-01-02-000000.log.gz");
        tokio::fs::write(&old_gz, b"old compressed").await.unwrap();

        // Set modification time to the past (use filetime crate or just rely on the default)
        // Since the files were just created, they won't be old enough with retention_days=1
        // But with retention_days=0, nothing should be cleaned
        let removed = manager.cleanup_logs_older_than(0).await.unwrap();
        assert_eq!(removed, 0, "retention_days=0 should skip cleanup");

        // With a very large retention (e.g., 36500 days = 100 years), nothing should be cleaned
        let removed = manager.cleanup_logs_older_than(36500).await.unwrap();
        assert_eq!(removed, 0, "files just created should not be removed");

        // Verify current.log still exists
        assert!(server_dir.join("current.log").exists());
        // Verify rotated logs still exist
        assert!(old_log.exists());
        assert!(old_gz.exists());
    }

    #[tokio::test]
    async fn test_cleanup_removes_old_files() {
        use filetime::FileTime;

        let temp_dir = tempfile::tempdir().unwrap();
        let config = LogConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_file_size: 1024 * 1024,
            max_files: 100,
            compress: false,
        };
        let manager = ServerLogManager::new(config);

        // Create server log directory
        let server_dir = temp_dir.path().join("space1").join("server1");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();

        // Create current.log
        tokio::fs::write(server_dir.join("current.log"), b"current")
            .await
            .unwrap();

        // Create old rotated log and set its mtime to 60 days ago
        let old_log = server_dir.join("2020-01-01-000000.log");
        tokio::fs::write(&old_log, b"old data").await.unwrap();
        let sixty_days_ago =
            FileTime::from_system_time(SystemTime::now() - Duration::from_secs(60 * 86400));
        filetime::set_file_mtime(&old_log, sixty_days_ago).unwrap();

        // Create old compressed log and set its mtime to 60 days ago
        let old_gz = server_dir.join("2020-01-02-000000.log.gz");
        tokio::fs::write(&old_gz, b"old compressed").await.unwrap();
        filetime::set_file_mtime(&old_gz, sixty_days_ago).unwrap();

        // Create a recent rotated log (keep default mtime = now)
        let recent_log = server_dir.join("2026-02-07-120000.log");
        tokio::fs::write(&recent_log, b"recent data").await.unwrap();

        // Cleanup with 30-day retention
        let removed = manager.cleanup_logs_older_than(30).await.unwrap();
        assert_eq!(removed, 2, "should remove 2 old files");

        // current.log must survive
        assert!(server_dir.join("current.log").exists());
        // Recent log must survive
        assert!(recent_log.exists());
        // Old logs should be gone
        assert!(!old_log.exists());
        assert!(!old_gz.exists());
    }

    #[tokio::test]
    async fn test_cleanup_removes_app_tracing_logs() {
        use filetime::FileTime;

        let temp_dir = tempfile::tempdir().unwrap();
        let config = LogConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_file_size: 1024 * 1024,
            max_files: 100,
            compress: false,
        };
        let manager = ServerLogManager::new(config);

        // Create app-level tracing log files at the top level of base_dir
        let old_app_log = temp_dir.path().join("mcpmux.2025-01-01.log");
        tokio::fs::write(&old_app_log, b"old tracing")
            .await
            .unwrap();
        let sixty_days_ago =
            FileTime::from_system_time(SystemTime::now() - Duration::from_secs(60 * 86400));
        filetime::set_file_mtime(&old_app_log, sixty_days_ago).unwrap();

        let recent_app_log = temp_dir.path().join("mcpmux.2026-02-08.log");
        tokio::fs::write(&recent_app_log, b"recent tracing")
            .await
            .unwrap();

        let removed = manager.cleanup_logs_older_than(30).await.unwrap();
        assert_eq!(removed, 1);
        assert!(!old_app_log.exists());
        assert!(recent_app_log.exists());
    }

    #[tokio::test]
    async fn test_cleanup_empty_dirs_removed() {
        use filetime::FileTime;

        let temp_dir = tempfile::tempdir().unwrap();
        let config = LogConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_file_size: 1024 * 1024,
            max_files: 100,
            compress: false,
        };
        let manager = ServerLogManager::new(config);

        // Create a server dir with only one old log (no current.log)
        let server_dir = temp_dir.path().join("space1").join("server_gone");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();

        let old_log = server_dir.join("2020-01-01-000000.log");
        tokio::fs::write(&old_log, b"old").await.unwrap();
        let old_time =
            FileTime::from_system_time(SystemTime::now() - Duration::from_secs(90 * 86400));
        filetime::set_file_mtime(&old_log, old_time).unwrap();

        let removed = manager.cleanup_logs_older_than(30).await.unwrap();
        assert_eq!(removed, 1);

        // Server dir and space dir should be cleaned up
        assert!(!server_dir.exists());
        assert!(!temp_dir.path().join("space1").exists());
    }
}
