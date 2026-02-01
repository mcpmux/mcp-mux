//! McpMux Gateway
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

pub use auth::AccessKeyAuth;
pub use oauth::{OAuthConfig, OAuthManager, OAuthToken};
pub use permissions::{PermissionFilter, PermissionSet};
pub use server::{
    AutoConnectResult, DependenciesBuilder, GatewayConfig, GatewayDependencies, GatewayServer,
    GatewayState, PendingAuthorization, StartupOrchestrator,
};

// Pool module - SOLID architecture
pub use pool::{
    // Types
    CachedFeatures,
    // Server Manager (event-driven orchestrator)
    ConnectResult,
    ConnectionContext,
    ConnectionResult,
    // Services
    ConnectionService,
    ConnectionStatus,
    DatabaseCredentialStore,
    // Instance types
    DiscoveredFeatures,
    FeatureService,
    InstalledServerInfo,
    InstanceKey,
    InstanceState,
    McpClient,
    McpClientConnection,
    McpClientHandler,
    OAuthCallback,
    OAuthInitResult,
    OAuthTokenInfo,
    // OAuth
    OutboundOAuthManager,
    PoolService,
    // Service Factory (DRY)
    PoolServices,
    PoolStats,
    ReconnectResult,
    ResolvedTransport,
    // Routing types
    RoutedPrompt,
    RoutedResource,
    RoutedTool,
    RoutingService,
    ServerInstance,
    ServerKey,
    ServerManager,
    ServerState,
    ServiceFactory,
    TokenService,
    TransportConnectResult,
    TransportFactory,
    TransportType,
};

// Services module
pub use services::{EventEmitter, GrantService, PrefixCacheService};

// MCP module (rmcp-based implementation)
pub use mcp::McpMuxGatewayHandler;

// Event-driven architecture consumers
pub use consumers::MCPNotifier;
