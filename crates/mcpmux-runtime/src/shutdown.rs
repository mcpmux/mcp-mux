//! Signal handling and graceful gateway shutdown.
//!
//! The desktop already installs SIGTERM/SIGINT handlers in
//! `apps/desktop/src-tauri/src/lib.rs`. This module provides the same
//! behaviour for the daemon plus a portable [`wait_for_shutdown`] future
//! that resolves with a label identifying which signal triggered the
//! shutdown.
//!
//! The graceful-shutdown helper [`shutdown_gateway_handle`] mirrors
//! `apps/desktop/src-tauri/src/commands/gateway::shutdown_gateway_handle`:
//! send the shutdown signal, await the join handle with a 2-second
//! timeout, abort as a last resort if axum hasn't drained in time.

use std::time::Duration;

use mcpmux_gateway::GatewayServerHandle;
use tokio::time::timeout;
use tracing::{info, warn};

/// Resolve when a termination signal arrives. Returns a label describing
/// which signal triggered the shutdown so the daemon can log a useful
/// exit message.
///
/// On Unix: SIGTERM or SIGINT. On Windows: Ctrl-C, Ctrl-Break, console
/// close, logoff, or shutdown.
pub async fn wait_for_shutdown() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                warn!("[runtime] failed to install SIGTERM handler: {}", e);
                return "signal-error";
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(e) => {
                warn!("[runtime] failed to install SIGINT handler: {}", e);
                return "signal-error";
            }
        };

        tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = sigint.recv() => "SIGINT",
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};
        let (mut c_c, mut c_break, mut c_close, mut c_logoff, mut c_shutdown) = match (
            ctrl_c(),
            ctrl_break(),
            ctrl_close(),
            ctrl_logoff(),
            ctrl_shutdown(),
        ) {
            (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e)) => (a, b, c, d, e),
            _ => return "signal-error",
        };

        tokio::select! {
            _ = c_c.recv() => "Ctrl+C",
            _ = c_break.recv() => "Ctrl+Break",
            _ = c_close.recv() => "Console close",
            _ = c_logoff.recv() => "Logoff",
            _ = c_shutdown.recv() => "Shutdown",
        }
    }
}

/// Gracefully shut down a running gateway handle.
///
/// Sends the shutdown signal, awaits the gateway task with a 2-second
/// timeout (matching the desktop's behaviour), then aborts as a last
/// resort. The `2s` cap exists because Windows would otherwise flag a
/// "process not responding" response if axum takes too long to drain.
pub async fn shutdown_gateway_handle(mut handle: GatewayServerHandle) {
    let abort = handle.task.abort_handle();
    handle.shutdown();
    match timeout(Duration::from_secs(2), handle.task).await {
        Ok(Ok(Ok(()))) => info!("[runtime] gateway task exited cleanly"),
        Ok(Ok(Err(e))) => warn!("[runtime] gateway task returned error: {}", e),
        Ok(Err(e)) if e.is_cancelled() => info!("[runtime] gateway task was already cancelled"),
        Ok(Err(e)) => warn!("[runtime] gateway task join error: {}", e),
        Err(_) => {
            warn!(
                "[runtime] graceful shutdown timed out after 2s — aborting task \
                 (listener may briefly linger in kernel)"
            );
            abort.abort();
        }
    }
}
