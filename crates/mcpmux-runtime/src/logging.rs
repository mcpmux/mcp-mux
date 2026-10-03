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

/// Build the desktop's per-crate debug-level filter as the default. Matches
/// the behavior the desktop shipped with before the runtime extraction.
pub fn default_filter() -> String {
    "info,mcpmux_core=debug,mcpmux_gateway=debug,mcpmux_storage=debug,mcpmux_mcp=debug,mcpmux_lib=debug,mcpmux_runtime=debug,tauri=info,tao=warn,wry=warn".to_string()
}

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
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!(
                    "{}: failed to create logs directory {}: {}",
                    branding::DISPLAY_NAME,
                    dir.display(),
                    e
                );
            }

            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix(prefix)
                .filename_suffix("log")
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
