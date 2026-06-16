//! Domain services
//!
//! Business logic that operates on domain entities via repositories.

pub mod app_settings_service;
mod cimd_fetcher;
mod client_install;
mod config_export;
pub mod gateway_port_service;
mod registry_api_client;
mod server_discovery;
mod server_log_manager;
mod space_service;

pub use app_settings_service::{keys, AppSettingsService};
pub use cimd_fetcher::*;
pub use client_install::{cursor_deep_link, vscode_deep_link};
pub use config_export::*;
pub use gateway_port_service::{
    allocate_dynamic_port, is_port_available, wait_for_port_available, GatewayPortService,
    PortAllocationError, PortResolution, AUTOSTART_PORT_WAIT, DEFAULT_GATEWAY_PORT,
};
pub use registry_api_client::*;
pub use server_discovery::*;
pub use server_log_manager::*;
pub use space_service::*;
