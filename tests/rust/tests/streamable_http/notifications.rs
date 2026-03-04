//! Test: Stateful Streamable HTTP with list_changed notifications
//!
//! Validates that:
//! 1. Stateful mode creates sessions with Mcp-Session-Id
//! 2. Server can send list_changed notifications to connected clients
//! 3. Clients receive notifications via SSE stream
//! 4. All notification types (tools, prompts, resources) are delivered
//! 5. Multiple clients can receive notifications simultaneously
//! 6. Protocol version negotiation works correctly

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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Simple test handler that supports list_changed notifications.
/// Stores the peer on initialization so we can send notifications externally.
/// Supports multiple peers for multi-client tests.
#[derive(Clone)]
struct TestNotificationHandler {
    /// Signal when peer is ready (on_initialized called)
    peer_ready: Arc<Notify>,
    /// Shared peer storage for sending notifications from outside
    peer_store: Arc<tokio::sync::RwLock<Option<rmcp::service::Peer<RoleServer>>>>,
    /// All peers (for multi-client tests)
    all_peers: Arc<tokio::sync::RwLock<Vec<rmcp::service::Peer<RoleServer>>>>,
    /// Count of initialized peers
    peer_count: Arc<AtomicUsize>,
}

impl TestNotificationHandler {
    fn new() -> Self {
        Self {
            peer_ready: Arc::new(Notify::new()),
            peer_store: Arc::new(tokio::sync::RwLock::new(None)),
            all_peers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            peer_count: Arc::new(AtomicUsize::new(0)),
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
        let peer = context.peer;
        {
            let mut store = self.peer_store.write().await;
            *store = Some(peer.clone());
        }
        {
            let mut all = self.all_peers.write().await;
            all.push(peer);
        }
        self.peer_count.fetch_add(1, Ordering::SeqCst);
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
            json_response: false,
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
    let client_handler = {
        let mut ch = NotificationTrackingClient::new();
        ch.notification_received = notification_received_clone.clone();
        ch.tools_changed = notification_received_clone;
        ch
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

/// Client handler that tracks all list_changed notification types
#[derive(Clone)]
struct NotificationTrackingClient {
    /// Legacy: signals when any tool notification is received
    notification_received: Arc<Notify>,
    /// Signals for each notification type
    tools_changed: Arc<Notify>,
    prompts_changed: Arc<Notify>,
    resources_changed: Arc<Notify>,
    /// Counters for each notification type
    tools_count: Arc<AtomicUsize>,
    prompts_count: Arc<AtomicUsize>,
    resources_count: Arc<AtomicUsize>,
}

impl NotificationTrackingClient {
    fn new() -> Self {
        let tools_changed = Arc::new(Notify::new());
        Self {
            notification_received: tools_changed.clone(),
            tools_changed,
            prompts_changed: Arc::new(Notify::new()),
            resources_changed: Arc::new(Notify::new()),
            tools_count: Arc::new(AtomicUsize::new(0)),
            prompts_count: Arc::new(AtomicUsize::new(0)),
            resources_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[allow(dead_code)]
    fn total_notifications(&self) -> usize {
        self.tools_count.load(Ordering::SeqCst)
            + self.prompts_count.load(Ordering::SeqCst)
            + self.resources_count.load(Ordering::SeqCst)
    }
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
        self.tools_count.fetch_add(1, Ordering::SeqCst);
        self.tools_changed.notify_one();
        async {}
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<rmcp::RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.prompts_count.fetch_add(1, Ordering::SeqCst);
        self.prompts_changed.notify_one();
        async {}
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<rmcp::RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.resources_count.fetch_add(1, Ordering::SeqCst);
        self.resources_changed.notify_one();
        async {}
    }
}

// ============================================================================
// A1: Prompts list_changed notification delivery
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_prompts_list_changed_notification_delivery() {
    let handler = TestNotificationHandler::new();
    let peer_store = handler.peer_store.clone();
    let (url, ct) = start_test_server(handler.clone()).await;

    let client_handler = NotificationTrackingClient::new();
    let prompts_changed = client_handler.prompts_changed.clone();

    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client = client_handler
        .serve(transport)
        .await
        .expect("client should connect");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready within 5s");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send prompts/list_changed
    {
        let peer = peer_store.read().await;
        let peer = peer.as_ref().expect("peer should exist");
        peer.notify_prompt_list_changed()
            .await
            .expect("notification should send");
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        prompts_changed.notified(),
    )
    .await;

    assert!(
        result.is_ok(),
        "Client should receive prompts/list_changed notification within 5s"
    );

    client.cancel().await.ok();
    ct.cancel();
}

// ============================================================================
// A2: Resources list_changed notification delivery
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_resources_list_changed_notification_delivery() {
    let handler = TestNotificationHandler::new();
    let peer_store = handler.peer_store.clone();
    let (url, ct) = start_test_server(handler.clone()).await;

    let client_handler = NotificationTrackingClient::new();
    let resources_changed = client_handler.resources_changed.clone();

    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client = client_handler
        .serve(transport)
        .await
        .expect("client should connect");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready within 5s");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send resources/list_changed
    {
        let peer = peer_store.read().await;
        let peer = peer.as_ref().expect("peer should exist");
        peer.notify_resource_list_changed()
            .await
            .expect("notification should send");
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        resources_changed.notified(),
    )
    .await;

    assert!(
        result.is_ok(),
        "Client should receive resources/list_changed notification within 5s"
    );

    client.cancel().await.ok();
    ct.cancel();
}

// ============================================================================
// A3: All notification types in a single session
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_all_notification_types_in_single_session() {
    let handler = TestNotificationHandler::new();
    let peer_store = handler.peer_store.clone();
    let (url, ct) = start_test_server(handler.clone()).await;

    let client_handler = NotificationTrackingClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let prompts_changed = client_handler.prompts_changed.clone();
    let resources_changed = client_handler.resources_changed.clone();

    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client = client_handler
        .serve(transport)
        .await
        .expect("client should connect");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready within 5s");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send all 3 notification types sequentially
    {
        let peer = peer_store.read().await;
        let peer = peer.as_ref().expect("peer should exist");

        peer.notify_tool_list_changed()
            .await
            .expect("tools notification should send");

        peer.notify_prompt_list_changed()
            .await
            .expect("prompts notification should send");

        peer.notify_resource_list_changed()
            .await
            .expect("resources notification should send");
    }

    // Wait for all 3 notifications
    let timeout = std::time::Duration::from_secs(5);

    let tools_result = tokio::time::timeout(timeout, tools_changed.notified()).await;
    assert!(
        tools_result.is_ok(),
        "Client should receive tools/list_changed"
    );

    let prompts_result = tokio::time::timeout(timeout, prompts_changed.notified()).await;
    assert!(
        prompts_result.is_ok(),
        "Client should receive prompts/list_changed"
    );

    let resources_result = tokio::time::timeout(timeout, resources_changed.notified()).await;
    assert!(
        resources_result.is_ok(),
        "Client should receive resources/list_changed"
    );

    client.cancel().await.ok();
    ct.cancel();
}

// ============================================================================
// A4: Multiple clients receive notifications
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_multiple_clients_receive_notifications() {
    let handler = TestNotificationHandler::new();
    let all_peers = handler.all_peers.clone();
    let peer_count = handler.peer_count.clone();
    let (url, ct) = start_test_server(handler.clone()).await;

    // Connect client 1
    let client1_handler = NotificationTrackingClient::new();
    let client1_tools = client1_handler.tools_changed.clone();
    let client1_tools_count = client1_handler.tools_count.clone();

    let transport1 = StreamableHttpClientTransport::from_uri(url.as_str());
    let client1 = client1_handler
        .serve(transport1)
        .await
        .expect("client 1 should connect");

    // Wait for client 1 peer
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer 1 should be ready");

    // Connect client 2
    let client2_handler = NotificationTrackingClient::new();
    let client2_tools = client2_handler.tools_changed.clone();
    let client2_tools_count = client2_handler.tools_count.clone();

    let transport2 = StreamableHttpClientTransport::from_uri(url.as_str());
    let client2 = client2_handler
        .serve(transport2)
        .await
        .expect("client 2 should connect");

    // Wait for client 2 peer
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer 2 should be ready");

    assert_eq!(
        peer_count.load(Ordering::SeqCst),
        2,
        "Should have 2 connected peers"
    );

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send notification to ALL peers
    {
        let peers = all_peers.read().await;
        for peer in peers.iter() {
            peer.notify_tool_list_changed()
                .await
                .expect("notification should send");
        }
    }

    // Both clients should receive the notification
    let timeout = std::time::Duration::from_secs(5);

    let r1 = tokio::time::timeout(timeout, client1_tools.notified()).await;
    assert!(r1.is_ok(), "Client 1 should receive tools/list_changed");

    let r2 = tokio::time::timeout(timeout, client2_tools.notified()).await;
    assert!(r2.is_ok(), "Client 2 should receive tools/list_changed");

    assert_eq!(client1_tools_count.load(Ordering::SeqCst), 1);
    assert_eq!(client2_tools_count.load(Ordering::SeqCst), 1);

    client1.cancel().await.ok();
    client2.cancel().await.ok();
    ct.cancel();
}

// ============================================================================
// A5: Session persists across multiple requests
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_session_persists_across_requests() {
    let handler = TestNotificationHandler::new();
    let (url, ct) = start_test_server(handler.clone()).await;

    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "session-test-client".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
    .serve(transport)
    .await
    .expect("client should connect");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready");

    // Make multiple requests through the same session
    let tools1 = client
        .list_tools(Default::default())
        .await
        .expect("first list_tools");
    assert_eq!(tools1.tools.len(), 1);

    let tools2 = client
        .list_tools(Default::default())
        .await
        .expect("second list_tools");
    assert_eq!(tools2.tools.len(), 1);

    // Call a tool
    let result = client
        .call_tool(CallToolRequestParams {
            name: "test_tool".into(),
            arguments: None,
            meta: None,
            task: None,
        })
        .await
        .expect("call_tool");
    assert!(!result.content.is_empty());

    // Third list should still work (same session)
    let tools3 = client
        .list_tools(Default::default())
        .await
        .expect("third list_tools");
    assert_eq!(tools3.tools.len(), 1);

    client.cancel().await.ok();
    ct.cancel();
}

// ============================================================================
// A6: Protocol version negotiation
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_protocol_version_negotiation() {
    let handler = TestNotificationHandler::new();
    let (url, ct) = start_test_server(handler.clone()).await;

    // Connect with default (latest) protocol version
    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "protocol-test-client".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
    .serve(transport)
    .await
    .expect("client should connect with default protocol version");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handler.peer_ready.notified(),
    )
    .await
    .expect("peer should be ready");

    // Verify the client is functional (protocol was negotiated)
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list_tools should work after negotiation");
    assert_eq!(tools.tools.len(), 1, "Should see test_tool");

    client.cancel().await.ok();
    ct.cancel();
}
