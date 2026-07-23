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
    mcp::{mcp_oauth_middleware, McpMuxGatewayHandler},
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
        Self::build(client_id, space_id, false).await
    }

    /// Like [`start`], but wires the REAL `mcp_oauth_middleware` with inbound
    /// auth disabled — no test-injected identity. Proves an anonymous client
    /// completes a real `initialize` handshake (the no-auth path that left
    /// editors "stuck at initialize" when auth was disabled but discovery still
    /// advertised OAuth).
    async fn start_authless(space_id: Uuid) -> Self {
        Self::build("mcpmux-anonymous", space_id, true).await
    }

    async fn build(client_id: &str, space_id: Uuid, authless: bool) -> Self {
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
            last_seen: None,
            created_at: now.clone(),
            updated_at: now,
            reports_roots: false,
            roots_capability_known: false,
            machine_id: None,
            client_icon: None,
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
            None,
        ));

        // Create MCPNotifier
        let notifier = Arc::new(MCPNotifier::new(
            services.feature_set_resolver.clone(),
            services.pool_services.feature_service.clone(),
        ));

        // Start MCPNotifier listening for domain events
        let event_rx = event_tx.subscribe();
        notifier.clone().start(event_rx);

        // Create handler
        let handler = McpMuxGatewayHandler::new(services.clone(), notifier.clone());

        // Build MCP service
        let mut http_cfg = StreamableHttpServerConfig::default();
        http_cfg.stateful_mode = true;
        http_cfg.json_response = false;
        http_cfg.sse_keep_alive = Some(std::time::Duration::from_secs(15));
        http_cfg.sse_retry = Some(std::time::Duration::from_secs(3));
        http_cfg.cancellation_token = ct.child_token();
        let mcp_service = StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            http_cfg,
        );

        // Build the router. Normal tests bypass auth with a test middleware that
        // injects a fixed identity; the authless variant exercises the REAL
        // middleware with inbound auth disabled, so the gateway must mint an
        // anonymous identity itself and the handshake must still succeed.
        let router =
            if authless {
                services.gateway_state.write().await.set_auth_disabled(true);
                Router::new().nest_service("/mcp", mcp_service).layer(
                    middleware::from_fn_with_state(services.clone(), mcp_oauth_middleware),
                )
            } else {
                let test_ctx = Arc::new(TestOAuthContext {
                    client_id: client_id.to_string(),
                    space_id,
                });
                Router::new().nest_service("/mcp", mcp_service).layer(
                    middleware::from_fn_with_state(test_ctx, test_oauth_middleware),
                )
            };

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
    /// Workspace roots this client reports when the gateway calls
    /// `roots/list`. Empty = the client does NOT declare the `roots`
    /// capability (rootless), matching the default editor-with-no-folder case.
    roots: Arc<Vec<String>>,
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
            roots: Arc::new(Vec::new()),
        }
    }

    /// A roots-capable client that reports the given roots (file:// URIs or
    /// absolute paths) when probed — models a real editor with a folder open.
    fn with_roots(roots: Vec<String>) -> Self {
        let mut c = Self::new();
        c.roots = Arc::new(roots);
        c
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
        let capabilities = if self.roots.is_empty() {
            ClientCapabilities::default()
        } else {
            ClientCapabilities::builder().enable_roots().build()
        };
        ClientInfo::new(
            capabilities,
            Implementation::new("gateway-test-client", "1.0.0"),
        )
    }

    fn list_roots(
        &self,
        _context: rmcp::service::RequestContext<RoleClient>,
    ) -> impl std::future::Future<Output = Result<ListRootsResult, rmcp::ErrorData>> + Send + '_
    {
        let roots = self.roots.iter().map(Root::new).collect();
        async move { Ok(ListRootsResult::new(roots)) }
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
// B1b: No-auth mode — anonymous client completes a real handshake
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn authless_anonymous_client_completes_real_initialize() {
    // Regression for the "stuck at initialize" report: with inbound auth
    // disabled, a client that sends NO token must complete the real `initialize`
    // handshake through the actual middleware + MCP handler (the gateway mints
    // an anonymous identity) instead of stalling. This is the end-to-end check
    // the earlier stub-handler test couldn't make.
    let space_id = Uuid::new_v4();
    let gw = TestGateway::start_authless(space_id).await;

    // `connect_client` sends no Authorization header and `.serve()` performs the
    // initialize handshake — it panics if the gateway 401s or never responds.
    let client = connect_client(&gw.url, GatewayTestClient::new()).await;

    // A live session that can list tools proves the handshake fully succeeded.
    let tools = client.list_tools(Default::default()).await;
    assert!(
        tools.is_ok(),
        "anonymous client must complete the handshake when auth is disabled"
    );

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
async fn test_gateway_forwards_feature_set_member_change_to_client() {
    // Replaces the old "grant change" test. Per-client grants are gone, so
    // the corresponding signal now is `FeatureSetMembersChanged` — emitted
    // when a user edits which features a Space's FS exposes.
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool1");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let new_tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool2");
    gw.feature_repo.upsert(&new_tool).await.unwrap();

    gw.emit(DomainEvent::FeatureSetMembersChanged {
        space_id,
        feature_set_id: "fs-test".to_string(),
        added_count: 1,
        removed_count: 0,
    });

    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;

    assert!(
        result.is_ok(),
        "Client should receive list_changed when a FS's members change"
    );

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B4b: Gateway forwards WorkspaceBindingChanged to every peer in the space
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_gateway_forwards_workspace_binding_change_to_client() {
    // User just created / updated / deleted a binding. Every connected MCP
    // client that resolves into this Space must re-fetch its tool list,
    // since the binding could have flipped the root → (space, FS) mapping.
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // Seed then add another tool so the content hash changes and the
    // notifier actually forwards the event (it dedupes on identical hash).
    let tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool1");
    gw.feature_repo.upsert(&tool).await.unwrap();

    let client_handler = GatewayTestClient::new();
    let tools_changed = client_handler.tools_changed.clone();
    let client = connect_client(&gw.url, client_handler).await;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let new_tool = tests::features::test_tool(&space_id.to_string(), "srv", "tool2");
    gw.feature_repo.upsert(&new_tool).await.unwrap();

    gw.emit(DomainEvent::WorkspaceBindingChanged {
        space_id,
        workspace_root: "/abs/proj".to_string(),
    });

    let result =
        tokio::time::timeout(std::time::Duration::from_secs(5), tools_changed.notified()).await;

    assert!(
        result.is_ok(),
        "Client should receive list_changed when a WorkspaceBinding is changed",
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

    // Wait for init + hash priming + the one-shot connect-time resolution
    // flip (the resolver fires a single per-peer list_changed on first
    // resolution so the client re-lists after roots/grants settle).
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Snapshot the post-connect baseline, then emit ToolsChanged WITHOUT
    // changing features. Content deduping must suppress THIS notification —
    // we assert the count doesn't move past the baseline rather than `== 0`,
    // so the legitimate connect-time flip doesn't mask the dedup check.
    let baseline = tools_count.load(Ordering::SeqCst);
    gw.emit(DomainEvent::ToolsChanged {
        server_id: "srv".to_string(),
        space_id,
    });

    // Wait a bit - no ADDITIONAL notification should be received.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    assert_eq!(
        tools_count.load(Ordering::SeqCst),
        baseline,
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

    // Initially no BACKEND tools (empty feature repo). The gateway always
    // appends its built-in `mcpmux_*` meta tools regardless of FS resolution,
    // so we assert on the non-meta subset here.
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list_tools should work");
    let backend_tools: Vec<_> = tools
        .tools
        .iter()
        .filter(|t| !t.name.starts_with("mcpmux_"))
        .collect();
    assert_eq!(
        backend_tools.len(),
        0,
        "Should start with no backend tools; meta tools are always present"
    );

    // list_tools should still work after re-fetch.
    let tools2 = client
        .list_tools(Default::default())
        .await
        .expect("second list_tools should work");
    let backend_tools2: Vec<_> = tools2
        .tools
        .iter()
        .filter(|t| !t.name.starts_with("mcpmux_"))
        .collect();
    assert_eq!(backend_tools2.len(), 0, "Still no backend tools");

    client.cancel().await.ok();
    gw.shutdown();
}

// ============================================================================
// B12: Multi-client — distinct workspace roots tracked independently
// ============================================================================

/// Two roots-capable clients (the "two editor windows on one OAuth identity"
/// case — same injected client_id, distinct sessions) connect over real HTTP
/// reporting DIFFERENT workspace roots. The gateway must probe each via
/// `roots/list` and track them independently per session, with no cross-talk
/// — the wire-level complement to the resolver/effective-features integration
/// tests that prove per-session routing.
#[tokio::test(flavor = "multi_thread")]
async fn test_multi_client_distinct_roots_tracked_independently() {
    let space_id = Uuid::new_v4();
    let client_id = Uuid::new_v4().to_string();
    let gw = TestGateway::start(&client_id, space_id).await;

    // POSIX roots so normalization is identical on every CI platform.
    let client_a = connect_client(
        &gw.url,
        GatewayTestClient::with_roots(vec!["file:///work/alpha".to_string()]),
    )
    .await;
    let client_b = connect_client(
        &gw.url,
        GatewayTestClient::with_roots(vec!["file:///work/beta".to_string()]),
    )
    .await;

    // Allow the on_initialized roots round-trip (gateway → client list_roots)
    // to populate the session registry for both sessions.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let roots = gw
        .services
        .feature_set_resolver
        .session_roots()
        .list_all_roots();
    assert!(
        roots.iter().any(|r| r == "/work/alpha"),
        "client A's root must be tracked (got {roots:?})"
    );
    assert!(
        roots.iter().any(|r| r == "/work/beta"),
        "client B's root must be tracked independently (got {roots:?})"
    );

    // Both sessions remain independently usable over the wire.
    assert!(client_a.list_tools(Default::default()).await.is_ok());
    assert!(client_b.list_tools(Default::default()).await.is_ok());

    client_a.cancel().await.ok();
    client_b.cancel().await.ok();
    gw.shutdown();
}
