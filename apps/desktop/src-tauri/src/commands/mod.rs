//! Tauri IPC Commands
//!
//! This module contains all commands that can be invoked from the frontend.
//! Commands are organized by feature area.

pub mod client;
pub mod client_install;
pub mod config_export;
pub mod credential;
pub mod feature_members;
pub mod feature_set;
pub mod gateway;
pub mod logs;
pub mod meta_tool_approval;
pub mod oauth;
pub mod server;
pub mod server_discovery;
pub mod server_feature;
pub mod server_manager;
pub mod session_overrides;
pub mod settings;
pub mod space;
pub mod workspace_binding;

// Re-export commands for convenience
pub use client::*;
pub use client_install::*;
pub use config_export::*;
pub use feature_members::*;
pub use feature_set::*;
pub use gateway::*;
pub use logs::*;
pub use meta_tool_approval::*;
pub use oauth::*;
pub use server::*;
pub use server_discovery::*;
pub use server_feature::*;
pub use server_manager::*;
pub use session_overrides::*;
pub use settings::*;
pub use space::*;
pub use workspace_binding::*;
