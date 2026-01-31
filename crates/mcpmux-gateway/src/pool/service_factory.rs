//! Service Factory - DRY service initialization
//!
//! Centralizes the creation of all pool services to avoid duplication.
//! Follows the Factory pattern for clean dependency injection.

use std::sync::Arc;

use crate::server::GatewayDependencies;
use mcpmux_core::DomainEvent;

use super::{
    OutboundOAuthManager, ConnectionService, FeatureService, PoolService, RoutingService, ServerManager,
    TokenService,
};

/// Bundle of all pool services - follows DRY principle
#[derive(Clone)]
pub struct PoolServices {
    pub pool_service: Arc<PoolService>,
    pub connection_service: Arc<ConnectionService>,
    pub feature_service: Arc<FeatureService>,
    pub token_service: Arc<TokenService>,
    pub oauth_manager: Arc<OutboundOAuthManager>,
    pub routing_service: Arc<RoutingService>,
    pub server_manager: Arc<ServerManager>,
}

/// Factory for creating pool services
///
/// Uses dependency injection container for clean initialization.
pub struct ServiceFactory;

impl ServiceFactory {
    /// Create all pool services with proper dependency injection
    ///
    /// This method encapsulates the complex wiring of services,
    /// ensuring consistency across different entry points (Desktop, CLI, tests).
    ///
    /// # Arguments
    /// * `deps` - Dependency injection container with all required repositories
    /// * `event_tx` - Event sender for unified domain event emission (non-blocking)
    /// * `prefix_cache` - Prefix cache service for runtime prefix assignment
    pub fn create_pool_services(
        deps: &GatewayDependencies,
        event_tx: tokio::sync::broadcast::Sender<DomainEvent>,
        prefix_cache: Arc<crate::services::PrefixCacheService>,
    ) -> PoolServices {
        // TokenService - single source of truth for token management
        let token_service = Arc::new(TokenService::new(
            deps.credential_repo.clone(),
            deps.backend_oauth_repo.clone(),
        ));

        // OutboundOAuthManager - handles OAuth flows
        // Inject SpaceRepository so OAuthManager can look up space names for DCR client_name
        let mut oauth_manager = OutboundOAuthManager::new()
            .with_log_manager(deps.log_manager.clone())
            .with_space_repo(deps.space_repo.clone());
        
        // Add settings repo for port persistence if available
        if let Some(ref settings_repo) = deps.settings_repo {
            oauth_manager = oauth_manager.with_settings_repo(settings_repo.clone());
        }
        let oauth_manager = Arc::new(oauth_manager);

        // ConnectionService - manages connect/disconnect lifecycle
        let connection_service = Arc::new(
            ConnectionService::new(
                token_service.clone(),
                oauth_manager.clone(),
                deps.credential_repo.clone(),
                deps.backend_oauth_repo.clone(),
                prefix_cache.clone(),
            )
            .with_log_manager(deps.log_manager.clone())
            .with_event_tx(event_tx.clone()),
        );

        // FeatureService - discovers and caches MCP features
        let feature_service = Arc::new(FeatureService::new(
            deps.feature_repo.clone(),
            deps.feature_set_repo.clone(),
            prefix_cache.clone(), // Clone here since we use it again below
        ));

        // ServerManager - event-driven orchestrator for server state
        // No longer has circular dependency with PoolService
        let server_manager = Arc::new(ServerManager::new(
            event_tx,
            feature_service.clone(),
            connection_service.clone(),
            prefix_cache.clone(),
        ));

        // PoolService - connection pool orchestrator
        // No longer needs ServerManager reference
        let pool_service = Arc::new(PoolService::new(
            connection_service.clone(),
            feature_service.clone(),
            token_service.clone(),
        ));

        // RoutingService - handles request dispatch
        // NOTE: No longer needs token_service - RMCP's AuthClient handles token refresh per-request
        let routing_service = Arc::new(RoutingService::new(
            feature_service.clone(),
            pool_service.clone(),
            deps.log_manager.clone(),
        ));

        PoolServices {
            pool_service,
            connection_service,
            feature_service,
            token_service,
            oauth_manager,
            routing_service,
            server_manager,
        }
    }
}
