//! Server instance representation
//!
//! Each (space_id, server_id) pair gets its own isolated ServerInstance.
//! No sharing between spaces - this is a security boundary.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mcpmux_core::{DomainEvent, LogLevel, LogSource, ServerLog, ServerLogManager};
use parking_lot::RwLock;
use rmcp::model::{ClientCapabilities, ClientInfo, Implementation, LoggingLevel};
use rmcp::service::{NotificationContext, RunningService};
use rmcp::RoleClient;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

// Re-export TransportType from mcpmux-core as the single source of truth
pub use mcpmux_core::TransportType;

/// Type alias for the MCP client service
pub type McpClient = RunningService<RoleClient, McpClientHandler>;

/// Upper bound on [`McpClientConnection::shutdown`].
///
/// rmcp's own cleanup waits several seconds for a child to exit after stdin
/// closes, and can block indefinitely behind a write to a server that stopped
/// reading. Shutdown runs on the app-exit path, which has ~2.5s in total
/// before Windows reports the app as not responding.
const CLIENT_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(1500);

/// Client handler for MCP connections
#[derive(Clone)]
pub struct McpClientHandler {
    info: ClientInfo,
    server_id: String,
    space_id: Uuid,
    event_tx: Option<tokio::sync::broadcast::Sender<DomainEvent>>,
    log_manager: Option<Arc<ServerLogManager>>,
    /// Per list kind: a forward is already scheduled (see `forward_list_changed`).
    pending_list_changed: [Arc<AtomicBool>; 3],
    /// Secret values scrubbed from the server's log messages.
    redact: Arc<Vec<String>>,
}

/// Upstream `*/list_changed` notifications arriving within this window are
/// forwarded as one event, so a chatty server can't flood the shared domain
/// event bus (whose lagging subscribers would then drop other events).
pub const LIST_CHANGED_COALESCE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug)]
enum ListKind {
    Tools = 0,
    Prompts = 1,
    Resources = 2,
}

impl std::fmt::Debug for McpClientHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClientHandler")
            .field("server_id", &self.server_id)
            .field("space_id", &self.space_id)
            .field("log_manager", &self.log_manager.is_some())
            .finish()
    }
}

impl McpClientHandler {
    pub fn new(
        server_id: &str,
        space_id: Uuid,
        event_tx: Option<tokio::sync::broadcast::Sender<DomainEvent>>,
        log_manager: Option<Arc<ServerLogManager>>,
    ) -> Self {
        let mut client_info =
            Implementation::new(format!("mcpmux-{}", server_id), env!("CARGO_PKG_VERSION"));
        client_info.title = Some("McpMux Gateway".to_string());
        Self {
            info: ClientInfo::new(ClientCapabilities::default(), client_info),
            server_id: server_id.to_string(),
            space_id,
            event_tx,
            log_manager,
            pending_list_changed: Default::default(),
            redact: Arc::new(Vec::new()),
        }
    }

    /// Scrub these secret values (longest first) from the server's log
    /// messages.
    pub fn with_redacted(mut self, redact: Arc<Vec<String>>) -> Self {
        self.redact = redact;
        self
    }

    /// Forward an upstream `list_changed` to the domain event bus, at most
    /// once per [`LIST_CHANGED_COALESCE`] per kind. The first notification
    /// schedules the event; any arriving before it is sent are folded into
    /// it. The event is sent at the end of the window, so the last change in
    /// a burst is never lost.
    fn forward_list_changed(&self, kind: ListKind, event: DomainEvent) {
        let Some(tx) = self.event_tx.clone() else {
            warn!(
                server_id = %self.server_id,
                space_id = %self.space_id,
                "[McpClientHandler] ⚠️ No event_tx available - cannot forward list_changed"
            );
            return;
        };
        let pending = Arc::clone(&self.pending_list_changed[kind as usize]);
        if pending.swap(true, Ordering::AcqRel) {
            debug!(
                server_id = %self.server_id,
                "[McpClientHandler] list_changed folded into the pending forward"
            );
            return;
        }
        let server_id = self.server_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(LIST_CHANGED_COALESCE).await;
            pending.store(false, Ordering::Release);
            info!(
                server_id = %server_id,
                kind = ?kind,
                "[McpClientHandler] Forwarding backend list_changed"
            );
            if let Err(e) = tx.send(event) {
                warn!(
                    server_id = %server_id,
                    error = %e,
                    "[McpClientHandler] ⚠️ Failed to forward list_changed (no subscribers)"
                );
            }
        });
    }

    /// Convert MCP protocol LoggingLevel to our internal LogLevel
    fn convert_logging_level(level: &LoggingLevel) -> LogLevel {
        match level {
            LoggingLevel::Debug => LogLevel::Debug,
            LoggingLevel::Info | LoggingLevel::Notice => LogLevel::Info,
            LoggingLevel::Warning => LogLevel::Warn,
            LoggingLevel::Error
            | LoggingLevel::Critical
            | LoggingLevel::Alert
            | LoggingLevel::Emergency => LogLevel::Error,
        }
    }
}

impl rmcp::ClientHandler for McpClientHandler {
    fn get_info(&self) -> ClientInfo {
        self.info.clone()
    }

    // Handle notifications from backend MCP servers. Each is forwarded to
    // the domain event bus, coalesced per kind (see `forward_list_changed`).
    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        debug!(
            server_id = %self.server_id,
            space_id = %self.space_id,
            "[McpClientHandler] 🔔 Backend server sent tools/list_changed notification"
        );
        self.forward_list_changed(
            ListKind::Tools,
            DomainEvent::ToolsChanged {
                server_id: self.server_id.clone(),
                space_id: self.space_id,
            },
        );
        std::future::ready(())
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        debug!(
            server_id = %self.server_id,
            space_id = %self.space_id,
            "[McpClientHandler] 🔔 Backend server sent prompts/list_changed notification"
        );
        self.forward_list_changed(
            ListKind::Prompts,
            DomainEvent::PromptsChanged {
                server_id: self.server_id.clone(),
                space_id: self.space_id,
            },
        );
        std::future::ready(())
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        debug!(
            server_id = %self.server_id,
            space_id = %self.space_id,
            "[McpClientHandler] 🔔 Backend server sent resources/list_changed notification"
        );
        self.forward_list_changed(
            ListKind::Resources,
            DomainEvent::ResourcesChanged {
                server_id: self.server_id.clone(),
                space_id: self.space_id,
            },
        );
        std::future::ready(())
    }

    fn on_logging_message(
        &self,
        params: rmcp::model::LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let server_id = self.server_id.clone();
        let space_id = self.space_id;
        let log_manager = self.log_manager.clone();
        let redact = Arc::clone(&self.redact);
        async move {
            // Format the log message from the MCP data field, then scrub the
            // server's own secrets and cap it like a stderr line.
            let message = match &params.data {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let mut message = crate::pool::transport::scrub_secrets(&message, &redact);
            let max = crate::pool::transport::MAX_SERVER_LOG_LINE;
            if message.len() > max {
                let mut cut = max;
                while !message.is_char_boundary(cut) {
                    cut -= 1;
                }
                message.truncate(cut);
                message.push('…');
            }

            let level = Self::convert_logging_level(&params.level);

            debug!(
                server_id = %server_id,
                space_id = %space_id,
                level = ?params.level,
                logger = ?params.logger,
                "[McpClientHandler] Server log: {}",
                message
            );

            if let Some(log_manager) = &log_manager {
                let mut log = ServerLog::new(level, LogSource::Server, &message);
                // Include logger name in metadata if present
                if let Some(logger) = &params.logger {
                    log = log.with_metadata(serde_json::json!({ "logger": logger }));
                }
                let _ = log_manager
                    .append(&space_id.to_string(), &server_id, log)
                    .await;
            }
        }
    }
}

/// Instance key - identifies a server instance for debugging/logging.
/// Note: Actual instance lookup uses (space_id, server_id) tuple in PoolService.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceKey {
    /// Space ID that owns this instance
    pub space_id: Uuid,
    /// Human-readable description (e.g., "http:https://mcp.example.com")
    pub description: String,
}

impl InstanceKey {
    /// Create instance key for STDIO transport.
    pub fn stdio(
        space_id: Uuid,
        command: &str,
        _args: &[String],
        _env: &HashMap<String, String>,
    ) -> Self {
        Self {
            space_id,
            description: format!("stdio:{}", command),
        }
    }

    /// Create instance key for HTTP transport.
    pub fn http(space_id: Uuid, url: &str, _headers: &HashMap<String, String>) -> Self {
        Self {
            space_id,
            description: format!("http:{}", url),
        }
    }
}

/// Connection state for a server instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceState {
    /// Not connected
    Disconnected,
    /// Currently connecting
    Connecting,
    /// Connected and ready
    Connected,
    /// Connection failed
    Failed,
    /// OAuth flow in progress
    OAuthPending,
}

/// Features discovered from an MCP server.
#[derive(Debug, Clone, Default)]
pub struct DiscoveredFeatures {
    /// Available tools
    pub tools: Vec<serde_json::Value>,
    /// Available prompts  
    pub prompts: Vec<serde_json::Value>,
    /// Available resources
    pub resources: Vec<serde_json::Value>,
}

/// Statistics for a server instance.
#[derive(Debug, Clone)]
pub struct InstanceStats {
    /// Current state
    pub state: InstanceState,
    /// When instance was connected (if connected)
    pub connected_at: Option<Instant>,
    /// Last connection attempt
    pub last_attempt: Option<Instant>,
    /// Consecutive failures
    pub consecutive_failures: u32,
    /// Total requests served
    pub requests_served: u64,
    /// Last error message
    pub last_error: Option<String>,
}

impl Default for InstanceStats {
    fn default() -> Self {
        Self {
            state: InstanceState::Disconnected,
            connected_at: None,
            last_attempt: None,
            consecutive_failures: 0,
            requests_served: 0,
            last_error: None,
        }
    }
}

/// An isolated MCP server instance.
/// Each (space_id, server_id) pair gets exactly one instance - no sharing.
pub struct ServerInstance {
    /// Instance key for debugging/logging
    pub key: InstanceKey,
    /// Server ID from the registry
    pub server_id: String,
    /// Transport type  
    pub transport_type: TransportType,
    /// Connection statistics
    pub stats: RwLock<InstanceStats>,
    /// Discovered features (populated after connection)
    pub features: RwLock<Option<DiscoveredFeatures>>,
    /// The actual MCP client connection
    client: RwLock<Option<McpClientConnection>>,
}

/// The actual MCP client connection.
pub enum McpClientConnection {
    /// STDIO transport - child process with MCP client
    Stdio { client: McpClient },
    /// HTTP transport - streamable HTTP
    Http { client: McpClient },
}

impl McpClientConnection {
    /// Get the MCP client for issuing requests.
    pub fn client(&self) -> Option<&McpClient> {
        match self {
            Self::Stdio { client } => Some(client),
            Self::Http { client } => Some(client),
        }
    }

    /// Cancel the MCP service and wait, bounded by [`CLIENT_SHUTDOWN_TIMEOUT`],
    /// for its transport to release its child process.
    ///
    /// On timeout the service is dropped: its task finishes cleanup in the
    /// background, and on Windows the kill-on-close job still takes the whole
    /// process tree down when the transport (or McpMux itself) goes away.
    pub async fn shutdown(self) {
        let mut client = match self {
            Self::Stdio { client } | Self::Http { client } => client,
        };

        match client.close_with_timeout(CLIENT_SHUTDOWN_TIMEOUT).await {
            Ok(Some(_)) => {}
            Ok(None) => warn!(
                "MCP client service did not shut down within {:?}; dropping it",
                CLIENT_SHUTDOWN_TIMEOUT
            ),
            Err(error) => warn!(%error, "Failed to shut down MCP client service"),
        }
    }
}

impl ServerInstance {
    /// Create a new disconnected instance.
    pub fn new(key: InstanceKey, server_id: String, transport_type: TransportType) -> Self {
        Self {
            key,
            server_id,
            transport_type,
            stats: RwLock::new(InstanceStats::default()),
            features: RwLock::new(None),
            client: RwLock::new(None),
        }
    }

    /// Get the current state.
    pub fn state(&self) -> InstanceState {
        self.stats.read().state
    }

    /// Check if connected and healthy.
    pub fn is_healthy(&self) -> bool {
        self.stats.read().state == InstanceState::Connected && self.client.read().is_some()
    }

    /// Update state to connecting.
    pub fn mark_connecting(&self) {
        let mut stats = self.stats.write();
        stats.state = InstanceState::Connecting;
        stats.last_attempt = Some(Instant::now());
    }

    /// Update state to connected with discovered features.
    pub fn mark_connected(&self, features: DiscoveredFeatures, connection: McpClientConnection) {
        let mut stats = self.stats.write();
        stats.state = InstanceState::Connected;
        stats.connected_at = Some(Instant::now());
        stats.consecutive_failures = 0;
        stats.last_error = None;

        *self.features.write() = Some(features);
        *self.client.write() = Some(connection);
    }

    /// Explicitly close the client before this instance is released.
    ///
    /// A dropped `RunningService` cancels in the background. On Windows that
    /// can race application exit, leaving a stdio server and its console alive.
    pub async fn shutdown(&self) {
        let connection = self.client.write().take();
        if let Some(connection) = connection {
            connection.shutdown().await;
        }
    }

    /// Update state to failed.
    pub fn mark_failed(&self, error: String) {
        let mut stats = self.stats.write();
        stats.state = InstanceState::Failed;
        stats.consecutive_failures += 1;
        stats.last_error = Some(error);
    }

    /// Update state to OAuth pending.
    pub fn mark_oauth_pending(&self) {
        let mut stats = self.stats.write();
        stats.state = InstanceState::OAuthPending;
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        self.stats.write().requests_served += 1;
    }

    /// Record a failed request.
    pub fn record_failure(&self, error: &str) {
        let mut stats = self.stats.write();
        stats.consecutive_failures += 1;
        stats.last_error = Some(error.to_string());
    }

    /// Get discovered features.
    pub fn get_features(&self) -> Option<DiscoveredFeatures> {
        self.features.read().clone()
    }

    /// Execute an operation with the MCP client.
    ///
    /// This is the primary API for accessing the MCP client. The closure receives
    /// a reference to the `McpClient` and can perform operations like calling tools
    /// or reading resources.
    ///
    /// Returns `None` if the client is not connected.
    ///
    /// # Example
    /// ```ignore
    /// let result = instance.with_client(|client| {
    ///     client.peer().clone()  // Get client handle for async operations
    /// });
    /// ```
    pub fn with_client<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&McpClient) -> R,
    {
        let guard = self.client.read();
        guard.as_ref().and_then(|conn| conn.client().map(f))
    }

    /// Get the server URL from the instance key (for HTTP/SSE transports).
    /// Returns None for STDIO transports.
    pub fn get_url(&self) -> Option<String> {
        // The description format is "transport:url" (e.g., "sse:https://mcp.atlassian.com/v1/sse")
        let desc = &self.key.description;
        if desc.starts_with("http:") {
            Some(desc.strip_prefix("http:").unwrap_or(desc).to_string())
        } else if desc.starts_with("sse:") {
            Some(desc.strip_prefix("sse:").unwrap_or(desc).to_string())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools_changed() -> DomainEvent {
        DomainEvent::ToolsChanged {
            server_id: "srv".into(),
            space_id: Uuid::nil(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_list_changed_is_forwarded_once() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(64);
        let handler = McpClientHandler::new("srv", Uuid::nil(), Some(tx), None);

        for _ in 0..50 {
            handler.forward_list_changed(ListKind::Tools, tools_changed());
        }
        // A different kind is tracked separately.
        handler.forward_list_changed(
            ListKind::Prompts,
            DomainEvent::PromptsChanged {
                server_id: "srv".into(),
                space_id: Uuid::nil(),
            },
        );
        assert!(
            rx.try_recv().is_err(),
            "nothing is sent before the window ends"
        );

        tokio::time::sleep(LIST_CHANGED_COALESCE * 2).await;
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        assert_eq!(
            events.len(),
            2,
            "one tools and one prompts event: {events:?}"
        );

        // A later change is still forwarded.
        handler.forward_list_changed(ListKind::Tools, tools_changed());
        tokio::time::sleep(LIST_CHANGED_COALESCE * 2).await;
        assert!(matches!(
            rx.try_recv(),
            Ok(DomainEvent::ToolsChanged { .. })
        ));
    }
}
