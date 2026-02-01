//! Server Pool - MCP connection management with SOLID architecture
//!
//! This module provides a clean, SOLID-compliant architecture for managing
//! MCP server connections:
//!
//! - **TokenService**: Single source of truth for OAuth token management
//! - **TransportFactory**: Creates transport instances (Stdio, HTTP)
//! - **ConnectionService**: Handles connect/disconnect lifecycle
//! - **FeatureService**: Discovers and caches MCP features
//! - **RoutingService**: Dispatches requests with permission filtering
//! - **PoolService**: Orchestrates all services

mod connection;
mod context;
mod credential_store;
mod features;
mod instance;
mod oauth;
mod oauth_utils;
mod routing;
mod server_manager;
mod service;
mod service_factory;
mod token;
pub mod transport;

// Context
pub use context::ConnectionContext;

// Instance types
pub use instance::{
    DiscoveredFeatures, InstanceKey, InstanceState, McpClient, McpClientConnection,
    McpClientHandler, ServerInstance, TransportType,
};

// OAuth
pub use credential_store::DatabaseCredentialStore;
pub use oauth::{
    OAuthCallback, OAuthCompleteEvent, OAuthInitResult, OAuthTokenInfo, OutboundOAuthManager,
};

// SOLID Services
pub use connection::{ConnectionResult, ConnectionService};
pub use features::{CachedFeatures, FeatureService};
pub use routing::{RoutedPrompt, RoutedResource, RoutedTool, RoutingService};
pub use service::{InstalledServerInfo, PoolService, PoolStats, ReconnectResult};
pub use token::TokenService;
pub use transport::{ResolvedTransport, Transport, TransportConnectResult, TransportFactory};

// Server Manager (Event-driven orchestrator)
pub use server_manager::{ConnectResult, ConnectionStatus, ServerKey, ServerManager, ServerState};

// Service Factory (DRY initialization)
pub use service_factory::{PoolServices, ServiceFactory};
