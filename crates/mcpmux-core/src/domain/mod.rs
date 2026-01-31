//! Domain entities, value objects, and events
//!
//! This module contains all domain-level types for McpMux:
//! - Entities (Space, InstalledServer, FeatureSet, Client, etc.)
//! - Value Objects (ConnectionStatus, FeatureType, etc.)
//! - Domain Events (DomainEvent enum for event-driven architecture)

mod client;
pub mod config;
mod credential;
mod event;
mod feature_set;
mod installed_server;
mod outbound_oauth_registration;
mod server;
mod server_feature;
mod server_log;
mod space;

// Export event types first (ConnectionStatus is defined here)
pub use event::{
    ConnectionStatus, DiscoveredCapabilities, DomainEvent, DomainEventEnvelope,
};

// Export entities (installed_server re-exports ConnectionStatus from event)
pub use client::*;
pub use config::*;
pub use credential::*;
pub use feature_set::*;
pub use installed_server::{InstalledServer, InstallationSource};
pub use outbound_oauth_registration::*;
pub use server::*;
pub use server_feature::*;
pub use server_log::*;
pub use space::*;
