//! Typed errors for the runtime bootstrap.

use std::path::PathBuf;

use thiserror::Error;

/// Errors raised by the shared runtime bootstrap.
///
/// Each variant maps to one of the runtime's failure modes. Callers can
/// downcast via `RuntimeError::is_lock_held()` etc.; the `Display` impl is
/// shaped for operator-facing error messages (no secret values, no paths
/// the operator didn't already pass in).
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The data directory is already locked by another McpMux process.
    ///
    /// Includes the owning PID when the lockfile is readable, so the
    /// operator knows who to ask before removing the lock by hand. The
    /// underlying `std::io::Error` from `fs2::FileExt::try_lock_exclusive`
    /// is preserved as the source for diagnostics.
    #[error("data directory is locked by another McpMux process ({owner})")]
    LockHeld {
        owner: LockOwner,
        #[source]
        source: std::io::Error,
    },

    /// `try_lock_exclusive` failed without producing a parseable lockfile.
    /// We couldn't read the owning PID, so the operator has to investigate
    /// by hand (e.g. `lsof data-dir/mcpmux.lock`).
    #[error("data directory is locked by another McpMux process (owner unknown)")]
    LockHeldUnknown {
        #[source]
        source: std::io::Error,
    },

    /// The data directory did not exist and could not be created. The
    /// parent path is included for the operator; the underlying
    /// `std::io::Error` explains why (permission denied, read-only fs, ...).
    #[error("failed to create data directory {path}: {source}")]
    DataDirCreate {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The `--data-dir` flag was malformed (relative path that resolves to
    /// a non-existent parent, symlink loop, etc.).
    #[error("invalid --data-dir path {path}: {reason}")]
    InvalidDataDir { path: PathBuf, reason: String },

    /// The master key provider (DPAPI / OS keychain / file) failed. Most
    /// commonly the OS keychain is unavailable on a headless host and the
    /// file fallback was disabled by `--key-provider=keychain`.
    #[error("key provider failure: {0}")]
    KeyProvider(String),

    /// The `mcpmux_gateway::DependenciesBuilder::build` call failed. Always
    /// a programming error (missing required dependency).
    #[error("gateway dependency build failed: {0}")]
    GatewayDeps(String),

    /// Wrapper for `std::io::Error` from any other IO path during init.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Read-only description of the process that currently holds the data-dir
/// lockfile. Parsed from the lockfile contents (PID + optional start time
/// + optional version).
#[derive(Debug, Clone)]
pub struct LockOwner {
    /// OS process id recorded in the lockfile.
    pub pid: u32,
    /// Unix seconds when the owner started, if recorded.
    pub started_at_unix: Option<u64>,
    /// McpMux version that wrote the lockfile, if recorded.
    pub version: Option<String>,
}

impl std::fmt::Display for LockOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.started_at_unix, self.version.as_deref()) {
            (Some(ts), Some(v)) => write!(f, "pid {} started {} v{}", self.pid, ts, v),
            (Some(ts), None) => write!(f, "pid {} started {}", self.pid, ts),
            (None, Some(v)) => write!(f, "pid {} v{}", self.pid, v),
            (None, None) => write!(f, "pid {}", self.pid),
        }
    }
}

impl RuntimeError {
    /// `true` when the failure was caused by another process already holding
    /// the data-dir lock.
    pub fn is_lock_held(&self) -> bool {
        matches!(self, Self::LockHeld { .. } | Self::LockHeldUnknown { .. })
    }
}
