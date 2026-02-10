//! Transport abstraction for MCP connections
//!
//! Provides a Transport trait and factory for creating different transport types.
//! This follows the Open/Closed Principle - new transports can be added without
//! modifying existing code.

mod http;
pub mod resolution;
mod stdio; // Expose resolution module

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use mcpmux_core::{CredentialRepository, OutboundOAuthRepository, ServerLogManager};
use uuid::Uuid;

pub use http::HttpTransport;
pub use stdio::{configure_child_process_platform, StdioTransport};

// Re-export TransportType from mcpmux-core as the single source of truth
pub use mcpmux_core::TransportType;

use super::instance::{McpClient, McpClientHandler};

/// Result of a transport connection attempt
pub enum TransportConnectResult {
    /// Successfully connected
    Connected(McpClient),
    /// OAuth required - returns server URL for OAuth flow
    OAuthRequired { server_url: String },
    /// Connection failed
    Failed(String),
}

/// Transport trait for MCP connections
///
/// Each transport implementation handles the specifics of connecting
/// to an MCP server using a particular protocol.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Attempt to connect to the MCP server
    async fn connect(&self) -> TransportConnectResult;

    /// Get the transport type
    fn transport_type(&self) -> TransportType;

    /// Get a description for logging
    fn description(&self) -> String;
}

/// Resolved transport configuration ready for connection.
///
/// All placeholders like `${input:API_KEY}` have been replaced with actual values.
/// This is the runtime representation, distinct from `mcpmux_core::TransportConfig`
/// which is the registry/template format.
#[derive(Debug, Clone)]
pub enum ResolvedTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        headers: HashMap<String, String>,
    },
}

impl ResolvedTransport {
    /// Get the transport type for this config
    pub fn transport_type(&self) -> TransportType {
        match self {
            ResolvedTransport::Stdio { .. } => TransportType::Stdio,
            ResolvedTransport::Http { .. } => TransportType::Http,
        }
    }

    /// Get URL for HTTP transports
    pub fn url(&self) -> Option<&str> {
        match self {
            ResolvedTransport::Http { url, .. } => Some(url),
            ResolvedTransport::Stdio { .. } => None,
        }
    }

    /// Generate a config hash for instance keying (excludes auth tokens)
    pub fn config_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        match self {
            ResolvedTransport::Stdio { command, args, env } => {
                "stdio".hash(&mut hasher);
                command.hash(&mut hasher);
                args.hash(&mut hasher);
                let mut env_pairs: Vec<_> = env.iter().collect();
                env_pairs.sort_by_key(|(k, _)| *k);
                for (k, v) in env_pairs {
                    k.hash(&mut hasher);
                    v.hash(&mut hasher);
                }
            }
            ResolvedTransport::Http { url, headers } => {
                "http".hash(&mut hasher);
                url.hash(&mut hasher);
                let mut header_pairs: Vec<_> = headers.iter().collect();
                header_pairs.sort_by_key(|(k, _)| *k);
                for (k, v) in header_pairs {
                    // Skip authorization headers for hashing
                    if !k.eq_ignore_ascii_case("authorization") {
                        k.hash(&mut hasher);
                        v.hash(&mut hasher);
                    }
                }
            }
        }
        hasher.finish()
    }
}

/// Factory for creating transport instances
pub struct TransportFactory;

impl TransportFactory {
    /// Create a transport from configuration
    ///
    /// For HTTP transports, the repositories are used to create a DatabaseCredentialStore
    /// that enables automatic token refresh via RMCP's AuthClient.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        config: &ResolvedTransport,
        space_id: Uuid,
        server_id: String,
        credential_repo: Arc<dyn CredentialRepository>,
        backend_oauth_repo: Arc<dyn OutboundOAuthRepository>,
        log_manager: Option<Arc<ServerLogManager>>,
        connect_timeout: std::time::Duration,
        event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
    ) -> Box<dyn Transport> {
        match config {
            ResolvedTransport::Stdio { command, args, env } => Box::new(StdioTransport::new(
                command.clone(),
                args.clone(),
                env.clone(),
                space_id,
                server_id,
                log_manager,
                connect_timeout,
                event_tx,
            )),
            ResolvedTransport::Http { url, headers } => Box::new(HttpTransport::new(
                url.clone(),
                headers.clone(),
                space_id,
                server_id,
                credential_repo,
                backend_oauth_repo,
                log_manager,
                connect_timeout,
                event_tx,
            )),
        }
    }
}

/// Create an MCP client handler for a server
pub fn create_client_handler(
    server_id: &str,
    space_id: uuid::Uuid,
    event_tx: Option<tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>>,
) -> McpClientHandler {
    McpClientHandler::new(server_id, space_id, event_tx)
}
