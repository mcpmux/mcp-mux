//! Gateway services
//!
//! Business logic services following SOLID principles:
//! - Each service has single responsibility (SRP)
//! - Services depend on abstractions (DIP)
//! - Open for extension, closed for modification (OCP)

mod authorization;
mod client_metadata_service;
mod event_emitter;
mod feature_set_resolver;
mod grant_service;
pub mod meta_tools;
mod notification_emitter;
mod prefix_cache;
mod session_roots;
mod space_resolver;

pub use authorization::AuthorizationService;
pub use client_metadata_service::ClientMetadataService;
pub use event_emitter::EventEmitter;
pub use feature_set_resolver::{FeatureSetResolverService, ResolutionSource, ResolvedFeatureSet};
pub use grant_service::GrantService;
pub use meta_tools::{
    is_meta_tool, ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher,
    ApprovalRequest, ApprovalScope, MetaToolRegistry, MCPMUX_PREFIX,
};
pub use notification_emitter::NotificationEmitter;
pub use prefix_cache::PrefixCacheService;
pub use session_roots::SessionRootsRegistry;
pub use space_resolver::SpaceResolverService;
