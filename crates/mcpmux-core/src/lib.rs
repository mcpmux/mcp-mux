//! # McpMux Core Library
//!
//! Domain logic, entities, and business rules for McpMux.
//!
//! ## Modules
//!
//! - `branding` - Centralized branding constants (generated from branding.toml)
//! - `domain` - Core entities (Space, InstalledServer, FeatureSet, Client)
//! - `registry` - MCP server registry schema and types
//! - `repository` - Data access traits
//! - `service` - Domain services
//! - `application` - Application services with event emission
//! - `event_bus` - Central event distribution system

pub mod application;
pub mod branding;
pub mod domain;
pub mod event_bus;
pub mod registry;
pub mod repository;
pub mod service;

// Re-export commonly used types
pub use domain::*;
pub use repository::*;
pub use service::*;

// Event-driven architecture exports
pub use application::{
    ApplicationServices, ApplicationServicesBuilder, ClientAppService, PermissionAppService,
    ServerAppService, SpaceAppService,
};
pub use event_bus::{
    create_shared_event_bus, EventBus, EventReceiver, EventSender, SharedEventBus,
};

use std::path::{Path, PathBuf};

/// Get the path to a space's configuration file (relative to a base spaces directory).
///
/// `space_id` must parse as a UUID — IPC callers pass attacker-influenceable
/// strings, and joining them raw would allow path traversal (`../..`) or, on
/// Windows, full path replacement (`Path::join` with an absolute path
/// discards the base). The canonical hyphenated form of the *parsed* UUID is
/// used as the filename, never the raw input.
pub fn get_space_config_path(spaces_dir: &Path, space_id: &str) -> Result<PathBuf, uuid::Error> {
    let id = uuid::Uuid::parse_str(space_id)?;
    Ok(spaces_dir.join(format!("{}.json", id)))
}
