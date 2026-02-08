//! Test: Stateful Streamable HTTP with list_changed notifications
//!
//! Validates that:
//! 1. Stateful mode creates sessions with Mcp-Session-Id
//! 2. Server can send list_changed notifications to connected clients
//! 3. Clients receive notifications via SSE stream

use rmcp::{
    model::*,
    service::{NotificationContext, RequestContext},
    transport::{
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
        StreamableHttpClientTransport,
    },
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Simple test handler that supports list_changed notifications.
/// Stores the peer on initialization so we can send notifications externally.
#[derive(Clone)]
struct TestNotificationHandler {
    /// Signal when peer is ready (on_initialized called)
    peer_ready: Arc<Notify>,
    /// Shared peer storage for sending notifications from outside
    peer_store: Arc<tokio::sync::RwLock<Option<rmcp::service::Peer<RoleServer>>>>,
}

impl TestNotificationHandler {
    fn new() -> Self {
        Self {
            peer_ready: Arc::new(Notify::new()),
            peer_store: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }
}

impl ServerHandler for TestNotificationHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools_with(ToolsCapability {
                    list_changed: Some(true), // Key: advertise notification support
                })
                .enable_prompts_with(PromptsCapability {
                    list_changed: Some(true),
                })
                .enable_resources_with(ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(true),
                })
                .build(),
            server_info: Implementation {
                name: "test-notification-server".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            instructions: None,
        }
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        // Store the peer so we can send notifications later
        let mut store = self.peer_store.write().await;
        *store = Some(context.peer);
        self.peer_ready.notify_one();
    }

    async fn list_tools(
        &self,
        _params: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let schema: Arc<serde_json::Map<String, serde_json::Value>> = Arc::new(
            serde_json::from_value(serde_json::json!({"type": "object", "properties": {}}))
                .unwrap(),
        );
        Ok(ListToolsResult::with_all_items(vec![Tool::new(
            "test_tool",
            "A test tool",
            schema,
        )]))
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Called: {}",
            params.name
        ))]))
    }
}

/// Start a test server and return the URL and cancellation token
async fn start_test_server(handler: TestNotificationHandler) -> (String, CancellationToken) {
    let ct = CancellationToken::new();

    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig {
            stateful_mode: true,
            sse_keep_alive: Some(std::time::Duration::from_secs(15)),
            sse_retry: Some(std::time::Duration::from_secs(3)),
            cancellation_token: ct.child_token(),
        },
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to random port");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", addr.port());

    let ct_clone = ct.clone();
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { ct_clone.cancelled().await })
            .await
            .unwrap();
    });

    // Give server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (url, ct)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stateful_session_management() {
    // Start server
    let handler = TestNotificationHandler::new();
    let (url, ct) = start_test_server(handler.clone()).await;

    // Connect client
    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test-client".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
    .serve(transport)
    .await
    .expect("client should connect");

    // Wait for on_initialized to fire
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready within 5s");

    // Verify peer is stored
    let peer = handler.peer_store.read().await;
    assert!(peer.is_some(), "Peer should be stored after on_initialized");

    // Verify we can list tools
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list_tools should work");
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "test_tool");

    // Cleanup
    client.cancel().await.ok();
    ct.cancel();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_changed_notification_delivery() {
    // Start server
    let handler = TestNotificationHandler::new();
    let peer_store = handler.peer_store.clone();
    let (url, ct) = start_test_server(handler.clone()).await;

    // Connect client with a handler that tracks notifications
    let notification_received = Arc::new(Notify::new());
    let notification_received_clone = notification_received.clone();

    let transport = StreamableHttpClientTransport::from_uri(url.as_str());

    // Use a custom client handler that detects tool_list_changed notifications
    let client_handler = NotificationTrackingClient {
        notification_received: notification_received_clone,
    };

    let client = client_handler
        .serve(transport)
        .await
        .expect("client should connect");

    // Wait for on_initialized to fire (server-side peer is ready)
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready within 5s");

    // Small delay to let the SSE stream establish
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send tools/list_changed notification from server to client
    {
        let peer = peer_store.read().await;
        let peer = peer.as_ref().expect("peer should exist");
        peer.notify_tool_list_changed()
            .await
            .expect("notification should send successfully");
    }

    // Wait for client to receive the notification
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        notification_received.notified(),
    )
    .await;

    assert!(
        result.is_ok(),
        "Client should receive tools/list_changed notification within 5s"
    );

    // Cleanup
    client.cancel().await.ok();
    ct.cancel();
}

/// Client handler that tracks when tool_list_changed notifications are received
#[derive(Clone)]
struct NotificationTrackingClient {
    notification_received: Arc<Notify>,
}

impl rmcp::ClientHandler for NotificationTrackingClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "notification-tracking-client".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<rmcp::RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.notification_received.notify_one();
        async {}
    }
}
