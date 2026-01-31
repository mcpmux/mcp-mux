//! Gateway services
//!
//! Business logic services following SOLID principles:
//! - Each service has single responsibility (SRP)
//! - Services depend on abstractions (DIP)
//! - Open for extension, closed for modification (OCP)

mod authorization;
mod client_metadata_service;
mod space_resolver;
mod prefix_cache;
mod event_emitter;
mod grant_service;
mod notification_emitter;

pub use authorization::AuthorizationService;
pub use client_metadata_service::ClientMetadataService;
pub use space_resolver::SpaceResolverService;
pub use prefix_cache::PrefixCacheService;
pub use event_emitter::EventEmitter;
pub use grant_service::GrantService;
pub use notification_emitter::NotificationEmitter;

