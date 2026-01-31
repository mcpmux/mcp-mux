//! MCMux Gateway
//!
//! MCP proxy server that provides:
//! - OAuth 2.1 authentication for remote MCP servers
//! - Request routing and aggregation
//! - Permission filtering via FeatureSets
//! - Client access key authentication
//! - Dependency Injection for clean architecture
//! - Event-driven architecture via DomainEvent consumers

pub mod auth;
pub mod consumers;
pub mod logging;
pub mod mcp;
pub mod oauth;
pub mod permissions;
pub mod pool;
pub mod server;
pub mod services;

pub use oauth::{OAuthConfig, OAuthManager, OAuthToken};
pub use server::{
    AutoConnectResult, GatewayConfig, GatewayServer, GatewayState, StartupOrchestrator,
    GatewayDependencies, DependenciesBuilder, PendingAuthorization,
};
pub use auth::AccessKeyAuth;
pub use permissions::{PermissionFilter, PermissionSet};

// Pool module - SOLID architecture
pub use pool::{
    // Services
    ConnectionService, FeatureService, PoolService, RoutingService, TokenService,
    // Service Factory (DRY)
    PoolServices, ServiceFactory,
    // Types
    CachedFeatures, ConnectionContext, ConnectionResult, InstalledServerInfo, PoolStats, ReconnectResult,
    ResolvedTransport, TransportConnectResult, TransportFactory,
    // Instance types
    DiscoveredFeatures, InstanceKey, InstanceState, McpClient, McpClientConnection,
    McpClientHandler, ServerInstance, TransportType,
    // OAuth
    OutboundOAuthManager, DatabaseCredentialStore, OAuthCallback, OAuthInitResult, OAuthTokenInfo,
    // Routing types
    RoutedPrompt, RoutedResource, RoutedTool,
    // Server Manager (event-driven orchestrator)
    ConnectResult, ConnectionStatus, ServerKey, ServerManager, ServerState,
};

// Services module
pub use services::{EventEmitter, GrantService, PrefixCacheService};

// MCP module (rmcp-based implementation)
pub use mcp::McmuxGatewayHandler;

// Event-driven architecture consumers
pub use consumers::MCPNotifier;
