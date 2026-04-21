//! Service Container - holds all initialized services
//!
//! Follows Inversion of Control pattern - services are created once
//! and reused throughout the application lifecycle.

use std::sync::Arc;

use crate::pool::{PoolServices, ServerManager, ServiceFactory};
use crate::services::{
    meta_tools, ApprovalBroker, AuthorizationService, ClientMetadataService,
    FeatureSetResolverService, GrantService, MetaToolRegistry, PrefixCacheService,
    SessionRootsRegistry, SpaceResolverService,
};
use mcpmux_core::DomainEvent;

use super::{dependencies::GatewayDependencies, GatewayState, StartupOrchestrator};

/// Container for all Gateway services
///
/// Follows Single Responsibility - only holds service references,
/// doesn't create or manage them.
/// Follows Dependency Injection - services are created once and injected
#[derive(Clone)]
pub struct ServiceContainer {
    /// All pool-related services
    pub pool_services: PoolServices,

    /// Server manager for event-driven connection orchestration
    pub server_manager: Arc<ServerManager>,

    /// Startup orchestrator for initialization tasks
    pub startup_orchestrator: Arc<StartupOrchestrator>,

    /// Authorization service for checking client permissions (SRP)
    pub authorization_service: Arc<AuthorizationService>,

    /// FeatureSet resolver v2 (pin > workspace > space-active).
    pub feature_set_resolver: Arc<FeatureSetResolverService>,

    /// Registry of per-session workspace roots (populated from MCP `roots/list`).
    pub session_roots: Arc<SessionRootsRegistry>,

    /// Broker that asks the desktop UI for user approval on meta-tool writes.
    /// Shared with the Tauri layer so it can attach a publisher + respond.
    pub approval_broker: Arc<ApprovalBroker>,

    /// Built-in `mcpmux_*` meta tools advertised alongside backend tools.
    pub meta_tool_registry: Arc<MetaToolRegistry>,

    /// Space resolver for determining client's active space (SRP)
    pub space_resolver_service: Arc<SpaceResolverService>,

    /// Prefix cache service for tool name qualification (SRP)
    pub prefix_cache_service: Arc<PrefixCacheService>,

    /// Client metadata service for OAuth client information
    pub client_metadata_service: Arc<ClientMetadataService>,

    /// Grant service for centralized grant management with auto-notifications (SRP + DRY)
    pub grant_service: Arc<GrantService>,

    /// Gateway state (for accessing base_url, JWT secret, etc.)
    pub gateway_state: Arc<tokio::sync::RwLock<GatewayState>>,

    /// Gateway dependencies (for accessing repositories, etc.)
    pub dependencies: GatewayDependencies,
}

impl ServiceContainer {
    /// Initialize all services from dependencies
    ///
    /// Follows Dependency Injection - creates services by wiring dependencies together.
    pub fn initialize(
        deps: &GatewayDependencies,
        domain_event_tx: tokio::sync::broadcast::Sender<DomainEvent>,
        gateway_state: Arc<tokio::sync::RwLock<GatewayState>>,
    ) -> Self {
        // Create prefix cache service with dependencies
        let prefix_cache_service = Arc::new(PrefixCacheService::new().with_dependencies(
            deps.installed_server_repo.clone(),
            deps.server_discovery.clone(),
        ));

        // Create pool services using factory (pass event_tx and prefix_cache)
        let pool_services = ServiceFactory::create_pool_services(
            deps,
            domain_event_tx.clone(),
            prefix_cache_service.clone(),
        );

        // Extract server_manager before moving pool_services
        let server_manager = pool_services.server_manager.clone();

        // Create startup orchestrator
        let startup_orchestrator = Arc::new(StartupOrchestrator::new(
            pool_services.pool_service.clone(),
            server_manager.clone(),
            deps.clone(),
            prefix_cache_service.clone(),
        ));

        // Resolver v2 — now authoritative. AuthorizationService delegates here.
        let session_roots = SessionRootsRegistry::new();
        let feature_set_resolver = Arc::new(FeatureSetResolverService::new(
            deps.inbound_mcp_client_repo.clone(),
            deps.space_repo.clone(),
            deps.workspace_binding_repo.clone(),
            session_roots.clone(),
        ));

        // Authorization service is now a thin adapter over the resolver.
        let authorization_service =
            Arc::new(AuthorizationService::new(feature_set_resolver.clone()));

        // Approval broker for meta-tool writes. Publisher is attached later
        // by the Tauri layer; until then, writes return `approval_required`.
        let approval_broker = Arc::new(ApprovalBroker::new());

        // Registry of built-in `mcpmux_*` meta tools (introspection + self-
        // management). Each write tool is gated by the broker above.
        let meta_tool_registry = meta_tools::build_default_registry(
            deps.inbound_mcp_client_repo.clone(),
            deps.space_repo.clone(),
            deps.feature_set_repo.clone(),
            deps.workspace_binding_repo.clone(),
            deps.feature_repo.clone(),
            feature_set_resolver.clone(),
            pool_services.feature_service.clone(),
            session_roots.clone(),
            approval_broker.clone(),
            domain_event_tx.clone(),
            deps.settings_repo.clone(),
        );

        // Create space resolver service (DIP: inject repository dependencies)
        let space_resolver_service = Arc::new(SpaceResolverService::new(
            deps.inbound_client_repo.clone(),
            deps.space_repo.clone(),
        ));

        // Create client metadata service
        let client_metadata_service = deps.client_metadata_service.clone();

        // Create grant service (centralized grant management with domain events)
        // Emits domain events (what happened) instead of implementation-specific events (what to do)
        let grant_service = Arc::new(GrantService::new(
            deps.inbound_client_repo.clone(), // Concrete type (pragmatic)
            deps.feature_set_repo.clone(),    // Trait (DIP)
            domain_event_tx.clone(),          // Direct event bus (decoupled)
        ));

        Self {
            pool_services,
            server_manager,
            startup_orchestrator,
            authorization_service,
            feature_set_resolver,
            session_roots,
            approval_broker,
            meta_tool_registry,
            space_resolver_service,
            prefix_cache_service,
            client_metadata_service,
            grant_service,
            gateway_state,
            dependencies: deps.clone(),
        }
    }
}
