//! # MCMux Core Library
//!
//! Domain logic, entities, and business rules for MCMux.
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
pub use event_bus::{EventBus, EventSender, EventReceiver, SharedEventBus, create_shared_event_bus};
pub use application::{
    ApplicationServices, ApplicationServicesBuilder,
    SpaceAppService, ServerAppService, PermissionAppService, ClientAppService,
};

use std::path::{Path, PathBuf};

/// Get the path to a space's configuration file (relative to a base spaces directory)
pub fn get_space_config_path(spaces_dir: &Path, space_id: &str) -> PathBuf {
    spaces_dir.join(format!("{}.json", space_id))
}
