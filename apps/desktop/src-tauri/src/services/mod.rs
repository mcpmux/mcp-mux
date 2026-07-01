//! Services module
//!
//! Background services for the desktop application.

pub mod admin_server;
pub mod admin_write_runtime;
pub mod file_watcher;
pub mod ui_events;

pub use admin_server::AdminServerState;
pub use file_watcher::SpaceFileWatcher;
