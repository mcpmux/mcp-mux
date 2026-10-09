//! Signal handling and graceful gateway shutdown.
//!
//! The desktop already installs SIGTERM/SIGINT handlers in
//! `apps/desktop/src-tauri/src/lib.rs`. This module provides the same
//! behaviour for the daemon plus a portable [`wait_for_shutdown`] future
//! that resolves with a label identifying which signal triggered the
//! shutdown.
//!
//! The graceful-shutdown helper [`shutdown_gateway_handle`] sends the
//! shutdown signal, awaits the join handle with a 2-second timeout, and
//! aborts as a last resort if axum hasn't drained in time.
//! [`shutdown_gateway_runtime`] pairs it with a bounded backend-pool drain;
//! both the desktop and the daemon tear their gateway down through it.

use std::sync::Arc;
use std::time::Duration;

use mcpmux_gateway::{GatewayServerHandle, PoolService};
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

/// Upper bound on draining the backend pool when the gateway goes down.
///
/// Each client close is itself bounded (1.5s); this caps the extra time spent
/// waiting on connects that were already in flight.
pub const POOL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Tear down a gateway: close its listener and drain its backend pool,
/// concurrently and both bounded.
///
/// Running them side by side keeps the desktop's app-exit path inside its
/// ~2.5s budget, and closes the listener while backends are going away
/// instead of serving requests that can only fail.
pub async fn shutdown_gateway_runtime(
    handle: Option<GatewayServerHandle>,
    pool_service: Option<Arc<PoolService>>,
) {
    let drain_pool = async {
        if let Some(pool) = pool_service {
            if timeout(POOL_SHUTDOWN_TIMEOUT, pool.shutdown())
                .await
                .is_err()
            {
                warn!(
                    "[runtime] backend pool did not drain within {:?}; continuing shutdown",
                    POOL_SHUTDOWN_TIMEOUT
                );
            }
        }
    };
    let close_listener = async {
        if let Some(h) = handle {
            shutdown_gateway_handle(h).await;
        }
    };
    tokio::join!(drain_pool, close_listener);
    // A server whose graceful close didn't finish in time (or whose handle is
    // never dropped, as on app exit) would keep its process group running.
    mcpmux_gateway::pool::transport::kill_all_stdio_groups();
}
