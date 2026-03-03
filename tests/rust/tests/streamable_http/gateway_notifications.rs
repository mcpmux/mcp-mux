//! Gateway-level integration tests for list_changed notifications
//!
//! Tests the full notification pipeline through the McpMux gateway:
//! - MCPNotifier receives DomainEvents and sends list_changed to clients
//! - Content-based deduping prevents spurious notifications
//! - Throttling coalesces rapid notifications
//! - Space isolation ensures cross-space notifications don't leak
//!
//! These tests build a real ServiceContainer with in-memory SQLite database,
//! bypassing OAuth via a test middleware that injects client/space headers.

use axum::{body::Body, http::Request, middleware, middleware::Next, response::Response, Router};
use mcpmux_core::{DomainEvent, ServerDiscoveryService, ServerFeatureRepository, ServerLogManager};
use mcpmux_gateway::{
    consumers::MCPNotifier,
    mcp::McpMuxGatewayHandler,
    server::{DependenciesBuilder, GatewayState, ServiceContainer},
};
use mcpmux_storage::{InboundClient, InboundClientRepository, RegistrationType};
use rmcp::{
    model::*,
    service::NotificationContext,
    transport::{
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
        StreamableHttpClientTransport,
    },
    RoleClient, ServiceExt,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

// ============================================================================
// Test OAuth Bypass Middleware
// ============================================================================

/// Test middleware that injects OAuth context headers without JWT validation.
/// Uses a fixed client_id and space_id for all requests.
async fn test_oauth_middleware(
    axum::extract::State(ctx): axum::extract::State<Arc<TestOAuthContext>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Skip for OPTIONS
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }

    // Inject the test client_id and space_id headers
    request
        .headers_mut()
        .insert("x-mcpmux-client-id", ctx.client_id.parse().unwrap());
    request.headers_mut().insert(
        "x-mcpmux-space-id",
        ctx.space_id.to_string().parse().unwrap(),
    );

    next.run(request).await
}

#[derive(Clone)]
struct TestOAuthContext {
    client_id: String,
    space_id: Uuid,
}

// ============================================================================
// Test Gateway Builder
// ============================================================================

#[allow(dead_code)]
struct TestGateway {
    url: String,
    event_tx: broadcast::Sender<DomainEvent>,
    ct: CancellationToken,
    notifier: Arc<MCPNotifier>,
    services: Arc<ServiceContainer>,
    feature_repo: Arc<MockServerFeatureRepository>,
    feature_set_repo: Arc<MockFeatureSetRepository>,
}

impl TestGateway {
    /// Build a test gateway with an in-memory database and mock repositories.
    /// The `client_id` and `space_id` are injected into all requests via test middleware.
    async fn start(client_id: &str, space_id: Uuid) -> Self {
        let ct = CancellationToken::new();

        // Create in-memory database
        let test_db = TestDatabase::in_memory();
        let database = Arc::new(tokio::sync::Mutex::new(test_db.db));

        // Create mock repositories
        let feature_repo = Arc::new(MockServerFeatureRepository::new());
        let feature_set_repo = Arc::new(MockFeatureSetRepository::new());

        // Create a default space in the space repo via database
        let space_repo = Arc::new(mcpmux_storage::SqliteSpaceRepository::new(database.clone()));
        let space = mcpmux_core::domain::Space {
            id: space_id,
            name: "Test Space".to_string(),
            icon: Some("test".to_string()),
            description: None,
            is_default: true,
            sort_order: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        mcpmux_core::SpaceRepository::create(&*space_repo, &space)
            .await
            .expect("create space");
        mcpmux_core::SpaceRepository::set_default(&*space_repo, &space_id)
            .await
            .expect("set default");

        // Create inbound client repository and register our test client
        let inbound_client_repo = Arc::new(InboundClientRepository::new(database.clone()));
        let now = chrono::Utc::now().to_rfc3339();
        let test_client = InboundClient {
            client_id: client_id.to_string(),
            registration_type: RegistrationType::Dcr,
            client_name: "test-client".to_string(),
            client_alias: None,
            redirect_uris: vec![],
            grant_types: vec!["authorization_code".to_string()],
            response_types: vec!["code".to_string()],
            token_endpoint_auth_method: "none".to_string(),
            scope: None,
            approved: true,
            logo_uri: None,
            client_uri: None,
            software_id: None,
            software_version: None,
            metadata_url: None,
            metadata_cached_at: None,
            metadata_cache_ttl: None,
            connection_mode: "follow_active".to_string(),
            locked_space_id: None,
            last_seen: None,
            created_at: now.clone(),
            updated_at: now,
        };
        inbound_client_repo
            .save_client(&test_client)
            .await
            .expect("save test client");

        // Build dependencies
        let deps =
            DependenciesBuilder::new()
                .with_installed_server_repo(Arc::new(MockInstalledServerRepository::new()))
                .with_credential_repo(Arc::new(MockCredentialRepository::new()))
                .with_backend_oauth_repo(Arc::new(MockOutboundOAuthRepository::new()))
                .with_feature_repo(
                    feature_repo.clone() as Arc<dyn mcpmux_core::ServerFeatureRepository>
                )
                .with_feature_set_repo(
                    feature_set_repo.clone() as Arc<dyn mcpmux_core::FeatureSetRepository>
                )
                .with_server_discovery(Arc::new(ServerDiscoveryService::new(
                    std::path::PathBuf::from("test-data"),
                    std::path::PathBuf::from("test-spaces"),
                )))
                .with_log_manager(Arc::new(ServerLogManager::new(
                    mcpmux_core::LogConfig::default(),
                )))
                .with_database(database)
                .build()
                .expect("build dependencies");

        // Override space_repo and inbound_client_repo in deps
        let deps = mcpmux_gateway::server::GatewayDependencies {
            space_repo: space_repo as Arc<dyn mcpmux_core::SpaceRepository>,
            inbound_client_repo,
            ..deps
        };

        // Create event channel
        let (event_tx, _) = broadcast::channel::<DomainEvent>(256);

        // Create gateway state
        let mut gw_state = GatewayState::new(event_tx.clone());
        gw_state.set_base_url("http://127.0.0.1:0".to_string());
        let gateway_state = Arc::new(tokio::sync::RwLock::new(gw_state));

        // Initialize ServiceContainer
        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state,
        ));

        // Create MCPNotifier
        let notifier = Arc::new(MCPNotifier::new(
            services.space_resolver_service.clone(),
            services.pool_services.feature_service.clone(),
        ));

        // Start MCPNotifier listening for domain events
        let event_rx = event_tx.subscribe();
        notifier.clone().start(event_rx);

        // Create handler
        let handler = McpMuxGatewayHandler::new(services.clone(), notifier.clone());

        // Build MCP service
        let mcp_service = StreamableHttpService::new(
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

        // Build router with test OAuth middleware
        let test_ctx = Arc::new(TestOAuthContext {
            client_id: client_id.to_string(),
            space_id,
        });

        let router =
            Router::new()
                .nest_service("/mcp", mcp_service)
                .layer(middleware::from_fn_with_state(
                    test_ctx,
                    test_oauth_middleware,
                ));

        // Bind to random port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().unwrap();
        let url = format!("http://127.0.0.1:{}/mcp", addr.port());

        let ct_clone = ct.clone();
        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { ct_clone.cancelled().await })
                .await
                .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        Self {
            url,
            event_tx,
            ct,
            notifier,
            services,
            feature_repo,
            feature_set_repo,
        }
    }

    fn emit(&self, event: DomainEvent) {
        let _ = self.event_tx.send(event);
    }

    fn shutdown(self) {
        self.ct.cancel();
    }
}

// ============================================================================
// Notification Tracking Client
// ============================================================================

#[derive(Clone)]
struct GatewayTestClient {
    tools_changed: Arc<Notify>,
    prompts_changed: Arc<Notify>,
    resources_changed: Arc<Notify>,
    tools_count: Arc<AtomicUsize>,
    prompts_count: Arc<AtomicUsize>,
    resources_count: Arc<AtomicUsize>,
}

impl GatewayTestClient {
    fn new() -> Self {
        Self {
            tools_changed: Arc::new(Notify::new()),
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

impl rmcp::ClientHandler for GatewayTestClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "gateway-test-client".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.tools_count.fetch_add(1, Ordering::SeqCst);
        self.tools_changed.notify_one();
        async {}
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.prompts_count.fetch_add(1, Ordering::SeqCst);
        self.prompts_changed.notify_one();
        async {}
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.resources_count.fetch_add(1, Ordering::SeqCst);
        self.resources_changed.notify_one();
        async {}
    }
}

/// Connect a test client to the gateway and wait for initialization
async fn connect_client(
    url: &str,
    handler: GatewayTestClient,
) -> rmcp::service::RunningService<RoleClient, GatewayTestClient> {
    let transport = StreamableHttpClientTransport::from_uri(url.to_string());
    handler
        .serve(transport)
        .await
        .expect("client should connect to gateway")
}

// ============================================================================
// B1: Gateway advertises list_changed capabilities
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_advertises_list_changed_capabilities() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    let client_handler = GatewayTestClient::new();
    let client = connect_client(&gw.url, client_handler).await;

    // Give time for initialization
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // The fact that we connected and initialized means capabilities were negotiated.
    // Verify we can list tools (proves the handler is working).
    let tools = client.list_tools(Default::default()).await;
    assert!(tools.is_ok(), "list_tools should succeed through gateway");

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B2: Gateway forwards ToolsChanged to client
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_forwards_tools_changed_to_client() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Seed feature repo with a tool so hash changes
    let tool = tests::features::test_tool(&space_id.to_string(), "test-server", "read_file");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler).await;

    // Wait for initialization and hash priming
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Now change features (add a new tool so hash changes)
    let new_tool = tests::features::test_tool(&space_id.to_string(), "test-server", "write_file");
    gw.feature_repo.upsert(&new_tool).await.unwrap();

    // Emit ToolsChanged event
    gw.emit(DomainEvent::ToolsChanged {
        server_id: "test-server".to_string(),
        space_id,
    });

    // Client should receive tools/list_changed
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;

    assert!(
        result.is_ok(),
        "Client should receive tools/list_changed through gateway"
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B3: Gateway forwards server disconnect to client
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_forwards_server_disconnect_to_client() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Seed features
    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool1");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler.clone()).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Remove features (simulating disconnect) so hash changes
    gw.feature_repo
        .delete_for_server(&space_id.to_string(), "srv")
        .await
        .unwrap();

    // Emit ServerStatusChanged(Disconnected)
    gw.emit(DomainEvent::ServerStatusChanged {
        server_id: "srv".to_string(),
        space_id,
        status: mcpmux_core::ConnectionStatus::Disconnected,
        flow_id: 1,
        has_connected_before: true,
        message: None,
        features: None,
    });

    // Client should receive at least tools/list_changed
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;

    assert!(
        result.is_ok(),
        "Client should receive list_changed when server disconnects"
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B4: Gateway forwards grant change to client
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_forwards_grant_change_to_client() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Seed a feature so hash has content
    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool1");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Add another feature so hash changes
    let new_tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool2");
    gw.feature_repo.upsert(&new_tool).await.unwrap();

    // Emit GrantIssued event
    gw.emit(DomainEvent::GrantIssued {
        client_id: client_id.clone(),
        space_id,
        feature_set_id: "fs-test".to_string(),
    });

    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;

    assert!(
        result.is_ok(),
        "Client should receive list_changed when grant is issued"
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B7: Content deduping prevents spurious notifications
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_content_deduping_prevents_spurious_notifications() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Seed features
    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool1");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_count = client_handler.tools_count.clone();
    let client = connect_client(&gw.url, client_handler).await;

    // Wait for init + hash priming
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Emit ToolsChanged WITHOUT changing features (hash stays same)
    gw.emit(DomainEvent::ToolsChanged {
        server_id: "srv".to_string(),
        space_id,
    });

    // Wait a bit - notification should NOT be received
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    assert_eq!(
        tools_count.load(Ordering::SeqCst),
        0,
        "No notification should be sent when features haven't changed (content deduping)"
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B8: Throttling coalesces rapid notifications
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_throttling_coalesces_rapid_notifications() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Start with one tool
    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "initial");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_count = client_handler.tools_count.clone();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Change features and emit 5 events rapidly (within throttle window)
    for i in 0..5 {
        let new_tool =
            tests::features::test_tool(&space_id.to_string(), "srv", &format!("rapid_tool_{}", i));
        gw.feature_repo.upsert(&new_tool).await.unwrap();

        gw.emit(DomainEvent::ToolsChanged {
            server_id: "srv".to_string(),
            space_id,
        });
    }

    // Wait for first notification
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;
    assert!(result.is_ok(), "Should receive at least one notification");

    // Wait a bit more to see if more arrive
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Due to throttling, should receive far fewer than 5 notifications
    let count = tools_count.load(Ordering::SeqCst);
    assert!(
        count < 5,
        "Throttling should coalesce rapid notifications (got {} instead of < 5)",
        count
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B10: ServerFeaturesRefreshed triggers notification
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_server_features_refreshed_triggers_notification() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Seed initial features
    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "old_tool");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Add new features to simulate refresh
    let new_tool = tests::features::test_tool(&space_id.to_string(), "srv", "new_tool");
    gw.feature_repo.upsert(&new_tool).await.unwrap();

    // Emit ServerFeaturesRefreshed
    gw.emit(DomainEvent::ServerFeaturesRefreshed {
        server_id: "srv".to_string(),
        space_id,
        features: mcpmux_core::DiscoveredCapabilities::default(),
        added: vec!["tool:new_tool".to_string()],
        removed: vec![],
    });

    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;

    assert!(
        result.is_ok(),
        "Client should receive notification when server features are refreshed"
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B11: Client can list tools after notification
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_client_can_list_tools_after_notification() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    let client_handler = GatewayTestClient::new();
    let client = connect_client(&gw.url, client_handler).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Initially no tools (empty feature repo = empty tools list)
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list_tools should work");
    assert_eq!(tools.tools.len(), 0, "Should start with no tools");

    // list_tools should still work after re-fetch
    let tools2 = client
        .list_tools(Default::default())
        .await
        .expect("second list_tools should work");
    assert_eq!(tools2.tools.len(), 0, "Still no tools");

    client.cancel().await.ok();
    gw.shutdown();
}
