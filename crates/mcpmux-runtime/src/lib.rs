//! Shared runtime bootstrap for McpMux desktop and headless daemon.
//!
//! Both `apps/desktop/src-tauri` and `apps/daemon` call into this crate to
//! resolve the data directory, acquire the exclusive lock, open the SQLite
//! database, build the repository graph, and (for the gateway path) hand
//! the result to `mcpmux_gateway::GatewayServer`.
//!
//! No Tauri dependency is permitted in this crate, `mcpmux-core`,
//! `mcpmux-gateway`, or `mcpmux-storage` — verified in CI by
//! `cargo tree -p mcpmux-runtime | grep tauri`.

mod error;
mod event_bridge;
mod health;
mod init;
mod lock;
mod logging;
pub mod master_key;
mod paths;
mod shutdown;

pub use error::{LockOwner, RuntimeError};
pub use event_bridge::spawn_event_bridge;
pub use health::{wait_for_health, HealthCheckConfig, HealthStatus};
pub use init::{
    KeyProviderPolicy, Repositories, Runtime, RuntimeBuilder, RuntimeConfig, DEFAULT_REGISTRY_URL,
    DEFAULT_SERVER_LOG_MAX_FILES, DEFAULT_SERVER_LOG_MAX_FILE_SIZE,
};
pub use lock::DataDirLock;
pub use logging::{default_filter, init_tracing, LogSink, TracingConfig};
pub use paths::{
    control_dir, control_dir_under, control_socket_path, default_data_dir, resolve_data_dir,
    DATA_DIR_NAME,
};
pub use shutdown::{
    shutdown_gateway_handle, shutdown_gateway_runtime, wait_for_shutdown, POOL_SHUTDOWN_TIMEOUT,
};
