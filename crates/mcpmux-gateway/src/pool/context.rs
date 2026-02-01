//! Connection context for MCP server connections.
//!
//! This module provides a context object that bundles per-connection parameters,
//! reducing function signature complexity throughout the connection pipeline.

use uuid::Uuid;

use super::transport::ResolvedTransport;

/// Context for a server connection attempt.
///
/// Bundles all per-call parameters needed for connecting to an MCP server.
/// Services (like SpaceRepository) are injected into the services themselves,
/// not passed through the context.
///
/// # Example
/// ```ignore
/// let ctx = ConnectionContext::new(space_id, "my-server")
///     .with_transport(transport)
///     .auto_reconnect(true);
///
/// pool_service.connect(&ctx).await;
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionContext {
    /// The space this connection belongs to
    pub space_id: Uuid,

    /// The server identifier
    pub server_id: String,

    /// Resolved transport configuration (command, args, env or URL)
    pub transport: ResolvedTransport,

    /// Whether this is an auto-reconnect (background) vs manual (user-initiated) connect
    /// - `true`: Don't start OAuth flow or open browser (background reconnection)
    /// - `false`: Full OAuth flow with browser if needed (user clicked Connect)
    pub auto_reconnect: bool,
}

impl ConnectionContext {
    /// Create a new connection context with required fields.
    pub fn new(space_id: Uuid, server_id: impl Into<String>, transport: ResolvedTransport) -> Self {
        Self {
            space_id,
            server_id: server_id.into(),
            transport,
            auto_reconnect: false,
        }
    }

    /// Set auto-reconnect mode (builder pattern).
    pub fn with_auto_reconnect(mut self, auto_reconnect: bool) -> Self {
        self.auto_reconnect = auto_reconnect;
        self
    }

    /// Convenience: create context for manual user-initiated connection.
    pub fn manual(
        space_id: Uuid,
        server_id: impl Into<String>,
        transport: ResolvedTransport,
    ) -> Self {
        Self::new(space_id, server_id, transport).with_auto_reconnect(false)
    }

    /// Convenience: create context for background auto-reconnection.
    pub fn auto(
        space_id: Uuid,
        server_id: impl Into<String>,
        transport: ResolvedTransport,
    ) -> Self {
        Self::new(space_id, server_id, transport).with_auto_reconnect(true)
    }
}
