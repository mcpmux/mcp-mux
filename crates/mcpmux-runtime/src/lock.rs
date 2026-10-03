//! Cross-process exclusive lock for the data directory.
//!
//! `flock(2)` semantics on Unix (`LockFileEx` on Windows): the lock is
//! released when the holding file descriptor is closed (i.e. when the
//! `DataDirLock` value is dropped). Two processes opening the same
//! `mcpmux.lock` cannot both hold the exclusive lock at the same time.
//!
//! The lockfile contents are NOT load-bearing — the kernel-level lock is
//! the source of truth. The contents (`<pid>\n<started_at_unix>\n<version>\n`)
//! exist only so we can report the owner PID to the operator when a second
//! process tries to acquire and fails. A missing or corrupt lockfile is
//! not an error condition (the kernel still told us it was held); we just
//! return `LockHeldUnknown` instead of `LockHeld { owner }`.
//!
//! A stale lockfile from a crashed process must NOT be silently removed —
//! the kernel releases the lock automatically when the fd closes, even if
//! the process never gets to clean up. See the roadmap's Phase 2 work for
//! permission / mode audits that may refuse startup.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use tracing::{debug, info};

use crate::error::{LockOwner, RuntimeError};

/// Filename inside the data directory used for the exclusive lock.
pub const LOCK_FILENAME: &str = "mcpmux.lock";
const OWNER_FILENAME: &str = "mcpmux.lock.owner";

/// Holder of the data-directory exclusive lock. Drop to release.
pub struct DataDirLock {
    file: std::fs::File,
    path: PathBuf,
}

impl std::fmt::Debug for DataDirLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataDirLock")
            .field("path", &self.path)
            .finish()
    }
}

impl DataDirLock {
    /// Acquire the exclusive lock on `<data_dir>/<LOCK_FILENAME>`. Creates
    /// the file (and the data directory if needed) if it does not exist.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::DataDirCreate`] — directory creation failed.
    /// - [`RuntimeError::LockHeld`] — another process holds the lock; the
    ///   lockfile was readable so we know the owner PID.
    /// - [`RuntimeError::LockHeldUnknown`] — lock held, but the lockfile
    ///   was missing / corrupt; the operator must investigate by hand.
    /// - [`RuntimeError::Io`] — any other filesystem failure.
    pub fn acquire(data_dir: &Path) -> Result<Self, RuntimeError> {
        std::fs::create_dir_all(data_dir).map_err(|source| RuntimeError::DataDirCreate {
            path: data_dir.to_path_buf(),
            source,
        })?;

        let path = data_dir.join(LOCK_FILENAME);
        let owner_path = data_dir.join(OWNER_FILENAME);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;

        // The separate, unlocked metadata file remains readable while
        // Windows holds the primary lock via `LockFileEx`.
        let recorded_owner = read_owner_file(&owner_path).ok().flatten();

        if let Err(source) = file.try_lock_exclusive() {
            // Best-effort: report the already-read owner PID so the operator
            // can identify the holder. The kernel lock remains authoritative.
            let owner = recorded_owner;
            return Err(match owner {
                Some(owner) => RuntimeError::LockHeld { owner, source },
                None => RuntimeError::LockHeldUnknown { source },
            });
        }

        write_owner(&file)?;
        write_owner_file(&owner_path)?;

        info!(path = %path.display(), "[runtime] acquired data-directory lock");

        Ok(Self { file, path })
    }

    /// Like [`Self::acquire`], but keeps retrying for up to `wait` while
    /// another process holds the lock, backing off between attempts.
    ///
    /// Covers the in-place self-update relaunch, where the OS starts the new
    /// build before the old process has finished exiting and released its
    /// lock. A holder that keeps the lock for the whole window still fails
    /// with the usual lock-held error. A zero `wait` behaves like `acquire`.
    pub async fn acquire_with_wait(data_dir: &Path, wait: Duration) -> Result<Self, RuntimeError> {
        let deadline = tokio::time::Instant::now() + wait;
        let mut backoff = Duration::from_millis(50);
        let mut logged = false;
        loop {
            match Self::acquire(data_dir) {
                Err(e) if e.is_lock_held() && tokio::time::Instant::now() < deadline => {
                    if !logged {
                        info!(
                            error = %e,
                            "[runtime] data directory is locked; waiting up to {:?} for the holder to exit",
                            wait
                        );
                        logged = true;
                    }
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    tokio::time::sleep(backoff.min(remaining)).await;
                    backoff = (backoff * 2).min(Duration::from_millis(500));
                }
                other => return other,
            }
        }
    }

    /// Path to the lockfile inside the data directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Best-effort read of the current owner's PID + version. Always
    /// returns the contents that were written by the *last* successful
    /// acquirer — the kernel-level lock is the actual source of truth.
    pub fn owner(&self) -> Option<LockOwner> {
        read_owner(&self.file).ok().flatten()
    }
}

impl Drop for DataDirLock {
    fn drop(&mut self) {
        // Unlock is best-effort; even if it fails, the kernel releases the
        // lock when the fd closes (which Drop does as the last step).
        if let Err(e) = self.file.unlock() {
            debug!(error = %e, "[runtime] unlock returned error (kernel will release on close)");
        }
    }
}

/// Parse the lockfile contents as `<pid>\n<started_at>\n<version>\n`.
///
/// Returns `Ok(None)` when the file is empty or truncated; `Err` on IO
/// failure. A missing file is also `Ok(None)` — the kernel-level lock is
/// the source of truth, not the file contents.
fn read_owner(file: &std::fs::File) -> std::io::Result<Option<LockOwner>> {
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;
    parse_owner(&contents)
}

fn read_owner_file(path: &Path) -> std::io::Result<Option<LockOwner>> {
    parse_owner(&std::fs::read_to_string(path)?)
}

fn parse_owner(contents: &str) -> std::io::Result<Option<LockOwner>> {
    let mut lines = contents.lines();
    let Some(pid_str) = lines.next() else {
        return Ok(None);
    };
    let pid: u32 = match pid_str.parse() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let started_at_unix = lines.next().and_then(|s| s.parse().ok());
    let version = lines.next().map(|s| s.to_string());
    Ok(Some(LockOwner {
        pid,
        started_at_unix,
        version,
    }))
}

/// Write the current PID + start time + version into the lockfile.
///
/// We `set_len(0)` first so a previous acquirer's stale lines never confuse
/// `read_owner` if the new lockfile is shorter.
fn write_owner(file: &std::fs::File) -> std::io::Result<()> {
    let contents = owner_contents();
    let mut writer = file;
    writer.set_len(0)?;
    writer.write_all(contents.as_bytes())?;
    writer.sync_all()
}

fn write_owner_file(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, owner_contents())
}

fn owner_contents() -> String {
    let pid = std::process::id();
    let started_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let version = env!("CARGO_PKG_VERSION");

    format!("{}\n{}\n{}\n", pid, started_at_unix, version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn acquire_succeeds_and_releases_on_drop() {
        let dir = TempDir::new().unwrap();
        {
            let lock = DataDirLock::acquire(dir.path()).expect("acquire");
            assert!(lock.path().exists());
        }
        // After drop, a fresh acquire must succeed.
        let lock2 = DataDirLock::acquire(dir.path()).expect("re-acquire");
        drop(lock2);
    }

    #[test]
    fn second_acquire_fails_with_owner_info() {
        let dir = TempDir::new().unwrap();
        let lock1 = DataDirLock::acquire(dir.path()).unwrap();
        let err = DataDirLock::acquire(dir.path()).unwrap_err();
        assert!(
            err.is_lock_held(),
            "expected lock-held error, got {:?}",
            err
        );

        // Operator sees the owner PID via the lockfile.
        let owner = lock1.owner().expect("owner readable");
        assert_eq!(owner.pid, std::process::id());
        assert_eq!(owner.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));

        drop(lock1);
    }

    #[tokio::test]
    async fn acquire_with_wait_succeeds_once_the_holder_releases() {
        let dir = TempDir::new().unwrap();
        let holder = DataDirLock::acquire(dir.path()).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(holder);
        });

        let lock = DataDirLock::acquire_with_wait(dir.path(), Duration::from_secs(5))
            .await
            .expect("lock acquired after the holder released it");
        release.join().unwrap();
        drop(lock);
    }

    #[tokio::test]
    async fn acquire_with_wait_gives_up_while_the_lock_stays_held() {
        let dir = TempDir::new().unwrap();
        let _holder = DataDirLock::acquire(dir.path()).unwrap();

        let err = DataDirLock::acquire_with_wait(dir.path(), Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(err.is_lock_held(), "expected lock-held error, got {err:?}");
    }
}
