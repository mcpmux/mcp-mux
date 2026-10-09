//! Logging init extracted from `apps/desktop/src-tauri/src/lib.rs`.
//!
//! Two sinks:
//! - [`LogSink::StdoutOnly`] — journald picks up the daemon's stdout/stderr.
//! - [`LogSink::DailyRolling`] — daily-rotated files in a directory; mirrors
//!   the desktop's behavior so end-users get the same `mcpmux.<date>.log`
//!   shape they had before.
//!
//! The caller is responsible for holding the returned
//! [`tracing_appender::non_blocking::WorkerGuard`] alive for the lifetime of
//! the program (dropping it stops file logging and may lose buffered
//! events).

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use mcpmux_core::branding;

/// Where the runtime's tracing output should land.
#[derive(Debug, Default, Clone)]
pub enum LogSink {
    /// Write only to stdout/stderr (the default for `mcpmuxd` under
    /// systemd-journald).
    #[default]
    StdoutOnly,
    /// Daily-rotated files in `dir` with the given filename prefix.
    DailyRolling { dir: PathBuf, prefix: String },
}

/// What [`init_tracing`] installs.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    pub sink: LogSink,
    /// `RUST_LOG`-style directive; see [`tracing_subscriber::EnvFilter`].
    pub filter: String,
    /// ANSI colors in the console layer. Leave on for interactive TTYs,
    /// off for systemd-journald (it captures raw bytes).
    pub console_ansi: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            sink: LogSink::StdoutOnly,
            filter: default_filter(),
            console_ansi: true,
        }
    }
}

/// The default tracing filter: per-crate debug in development builds, `info`
/// in release builds (debug output includes request details that don't
/// belong in a log file kept on disk). `RUST_LOG` overrides either.
pub fn default_filter() -> String {
    if cfg!(debug_assertions) {
        "info,mcpmux_core=debug,mcpmux_gateway=debug,mcpmux_storage=debug,mcpmux_mcp=debug,mcpmux_lib=debug,mcpmux_runtime=debug,tauri=info,tao=warn,wry=warn".to_string()
    } else {
        "info,tauri=info,tao=warn,wry=warn".to_string()
    }
}

/// Daily log files kept before the oldest is deleted.
pub const MAX_LOG_FILES: usize = 14;

/// Initialize the global tracing subscriber.
///
/// Returns `Some(WorkerGuard)` when a non-blocking file writer was created;
/// the caller MUST hold it alive. Returns `None` for `LogSink::StdoutOnly`.
///
/// Idempotent in the sense that calling it twice will silently re-init the
/// global subscriber — operators should call exactly once during process
/// startup.
pub fn init_tracing(config: &TracingConfig) -> Option<WorkerGuard> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(config.filter.clone()));

    let console_layer = fmt::layer()
        .with_ansi(config.console_ansi)
        .compact()
        .with_thread_names(false)
        .with_line_number(false)
        .with_file(false)
        .with_target(true);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer);

    match &config.sink {
        LogSink::StdoutOnly => {
            subscriber.init();
            None
        }
        LogSink::DailyRolling { dir, prefix } => {
            if let Err(e) = crate::private_dir::ensure_private_dir(dir) {
                // Don't write logs where they can't be kept private (or at
                // all): log to stdout instead.
                eprintln!(
                    "{}: cannot use logs directory {} ({}); logging to stdout",
                    branding::DISPLAY_NAME,
                    dir.display(),
                    e
                );
                subscriber.init();
                return None;
            }

            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix(prefix)
                .filename_suffix("log")
                .max_log_files(MAX_LOG_FILES)
                .build(dir)
                .expect("failed to create rolling file appender");

            let (non_blocking, guard) = tracing_appender::non_blocking(appender);

            let file_layer = fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_thread_ids(true)
                .with_line_number(true)
                .with_file(true)
                .with_target(true);

            subscriber.with(file_layer).init();

            Some(guard)
        }
    }
}

#[allow(dead_code)]
pub fn default_sink_for_desktop(data_dir: &Path) -> LogSink {
    LogSink::DailyRolling {
        dir: data_dir.join("logs"),
        prefix: branding::LOG_PREFIX.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_mentions_every_internal_crate() {
        let filter = default_filter();
        for crate_name in [
            "mcpmux_core",
            "mcpmux_gateway",
            "mcpmux_storage",
            "mcpmux_mcp",
            "mcpmux_lib",
            "mcpmux_runtime",
        ] {
            assert!(
                filter.contains(crate_name),
                "default filter missing crate: {}",
                crate_name
            );
        }
    }

    #[test]
    fn desktop_sink_is_daily_rolling_under_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let sink = default_sink_for_desktop(tmp.path());
        match sink {
            LogSink::DailyRolling { dir, prefix } => {
                assert_eq!(dir, tmp.path().join("logs"));
                assert_eq!(prefix, branding::LOG_PREFIX);
            }
            LogSink::StdoutOnly => panic!("desktop sink should be rotating, not stdout"),
        }
    }
}
