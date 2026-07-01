//! End-to-end tests for the `mcpmux_*` self-management meta tools.
//!
//! Exercises the full path through the [`MetaToolRegistry`]:
//!   * read tools return structured payloads
//!   * write tools gate on the [`ApprovalBroker`] and only mutate state on Allow
//!   * denial / timeout / no-publisher surface as `CallToolResult::error`
//!   * "always-allow" persists for subsequent calls in the same session

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use mcpmux_core::{
    normalize_workspace_root, Client, DomainEvent, EmbeddingRecord, EmbeddingRepository,
    FeatureSet, FeatureSetMember, FeatureSetRepository, InboundMcpClientRepository,
    InputDefinition, InstalledServer, InstalledServerRepository, LogConfig, Machine, MachineRepository,
    MemberMode, MemberType, ServerDefinition, ServerFeature, ServerFeatureRepository, ServerLogManager,
    ServerSource, SpaceRepository, TransportConfig, TransportMetadata, WorkspaceBinding,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::pool::{
    CachedFeatures, ConnectionService, FeatureService, OutboundOAuthManager, ServerKey,
    ServerManager, TokenService,
};
use mcpmux_gateway::services::{
    meta_tools, ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher,
    EmbeddingWarmer, FeatureSetResolverService, MetaToolRegistry, PrefixCacheService,
    SessionRootsRegistry, META_TOOL_APPROVAL_EVENT,
};
use mcpmux_gateway::MCPNotifier;
use mcpmux_storage::{
    generate_master_key, Database, FieldEncryptor, InboundClientRepository,
    SqliteEmbeddingRepository, SqliteFeatureSetRepository, SqliteInboundMcpClientRepository,
    SqliteInstalledServerRepository, SqliteMachineRepository, SqliteServerFeatureRepository,
    SqliteSpaceBaseDirRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use serde_json::{json, Value};
use tests::mocks::{MockCredentialRepository, MockOutboundOAuthRepository};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

pub(crate) struct Fixture {
    pub(crate) registry: Arc<MetaToolRegistry>,
    broker: Arc<ApprovalBroker>,
    db: Arc<Mutex<Database>>,
    #[allow(dead_code)]
    client_repo: Arc<dyn InboundMcpClientRepository>,
    pub(crate) feature_set_repo: Arc<dyn FeatureSetRepository>,
    pub(crate) server_feature_repo: Arc<dyn ServerFeatureRepository>,
    pub(crate) binding_repo: Arc<dyn WorkspaceBindingRepository>,
    installed_server_repo: Arc<dyn InstalledServerRepository>,
    pub(crate) session_roots: Arc<SessionRootsRegistry>,
    feature_service: Arc<FeatureService>,
    pub(crate) space_id: Uuid,
    /// Opaque client identity (UUID-as-string here; in production for DCR
    /// clients this can be a `client_metadata` URL).
    pub(crate) client_id: String,
    pub(crate) session_id: String,
    fs_android_id: Uuid,
    github_tool_id: Uuid,
    event_rx: broadcast::Receiver<DomainEvent>,
    server_manager: Arc<ServerManager>,
}

fn test_encryptor() -> Arc<FieldEncryptor> {
    let key = generate_master_key().expect("generate key");
    Arc::new(FieldEncryptor::new(&key).expect("create encryptor"))
}

fn test_log_manager() -> Arc<ServerLogManager> {
    let base_dir = std::env::temp_dir().join(format!("mcpmux-meta-tools-logs-{}", Uuid::new_v4()));
    Arc::new(ServerLogManager::new(LogConfig {
        base_dir,
        max_file_size: 1024 * 1024,
        max_files: 5,
        compress: false,
    }))
}

fn test_server_manager(
    event_tx: broadcast::Sender<DomainEvent>,
    feature_service: Arc<FeatureService>,
    prefix_cache: Arc<PrefixCacheService>,
) -> Arc<ServerManager> {
    let credential_repo = Arc::new(MockCredentialRepository::new());
    let oauth_repo = Arc::new(MockOutboundOAuthRepository::new());
    let token_service = Arc::new(TokenService::new(
        credential_repo.clone(),
        oauth_repo.clone(),
    ));
    let oauth_manager = Arc::new(OutboundOAuthManager::new());
    let connection_service = Arc::new(ConnectionService::new(
        token_service,
        oauth_manager,
        credential_repo,
        oauth_repo,
        prefix_cache.clone(),
    ));
    Arc::new(ServerManager::new(
        event_tx,
        feature_service,
        connection_service,
        prefix_cache,
    ))
}

fn stdio_definition_with_required_input(server_id: &str, input_id: &str) -> ServerDefinition {
    ServerDefinition {
        id: server_id.to_string(),
        name: server_id.to_string(),
        description: None,
        alias: None,
        auth: None,
        icon: None,
        transport: TransportConfig::Stdio {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "pkg".to_string()],
            env: Default::default(),
            metadata: TransportMetadata {
                inputs: vec![InputDefinition {
                    id: input_id.to_string(),
                    label: input_id.to_string(),
                    r#type: "text".to_string(),
                    required: true,
                    secret: true,
                    description: None,
                    default: None,
                    placeholder: None,
                    obtain_url: None,
                    obtain_instructions: None,
                }],
            },
        },
        categories: vec![],
        publisher: None,
        source: ServerSource::Bundled,
        badges: vec![],
        hosting_type: Default::default(),
        license: None,
        license_url: None,
        installation: None,
        capabilities: None,
        sponsored: None,
        media: None,
        changelog_url: None,
    }
}

async fn seed_diagnose_servers(f: &Fixture) {
    let space_id = f.space_id.to_string();

    let github_def = stdio_definition_with_required_input("github", "github_token");
    let github = InstalledServer::new(&space_id, "github")
        .with_definition(&github_def)
        .with_input("github_token", "secret");
    f.installed_server_repo.install(&github).await.unwrap();
    f.server_manager
        .set_connected(
            &ServerKey::new(f.space_id, "github"),
            CachedFeatures::default(),
        )
        .await;

    let firebase_def = stdio_definition_with_required_input("firebase", "api_key");
    let firebase = InstalledServer::new(&space_id, "firebase")
        .with_definition(&firebase_def)
        .with_input("api_key", "key");
    f.installed_server_repo.install(&firebase).await.unwrap();
    f.server_manager
        .set_error(
            &ServerKey::new(f.space_id, "firebase"),
            "Connection refused".to_string(),
        )
        .await;
}

impl Fixture {
    pub(crate) async fn new() -> Self {
        Self::new_with_db(Arc::new(Mutex::new(Database::open_in_memory().unwrap())), None).await
    }

    /// Gateway fixture with `local_machine_id` set (machine row seeded in DB).
    pub(crate) async fn new_with_local_machine(machine_name: &str) -> Self {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let machine_repo = SqliteMachineRepository::new(db.clone());
        let machine = Machine::new(machine_name);
        let machine_id = machine.id;
        machine_repo.create(&machine).await.unwrap();
        Self::new_with_db(db, Some(machine_id)).await
    }

    pub(crate) async fn new_with_db(
        db: Arc<Mutex<Database>>,
        local_machine_id: Option<Uuid>,
    ) -> Self {
        let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
        let feature_set_repo: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));
        let client_repo: Arc<dyn InboundMcpClientRepository> =
            Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));
        let binding_repo: Arc<dyn WorkspaceBindingRepository> =
            Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
        let server_feature_repo: Arc<dyn ServerFeatureRepository> =
            Arc::new(SqliteServerFeatureRepository::new(db.clone()));
        let installed_server_repo: Arc<dyn InstalledServerRepository> = Arc::new(
            SqliteInstalledServerRepository::new(db.clone(), test_encryptor()),
        );

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        // Two FSes we'll flip between in the tests.
        let fs_android = FeatureSet::new_custom("Android Dev", space_id.to_string());
        let fs_full = FeatureSet::new_custom("Full Access", space_id.to_string());
        feature_set_repo.create(&fs_android).await.unwrap();
        feature_set_repo.create(&fs_full).await.unwrap();
        let fs_android_id = Uuid::parse_str(&fs_android.id).unwrap();
        let fs_full_id = Uuid::parse_str(&fs_full.id).unwrap();

        // Seed two tools in server_features for the tools listing test.
        //
        // Tool names are stored bare; qualified_name() prepends the server
        // prefix, so e.g. ("github", "create_issue") → "github_create_issue".
        let mut feature1 = ServerFeature::tool(space_id, "github", "create_issue");
        feature1.display_name = Some("GitHub".into());
        feature1.description = Some("Create an issue".into());
        let mut feature2 = ServerFeature::tool(space_id, "firebase", "deploy");
        feature2.display_name = Some("Firebase".into());
        feature2.description = Some("Deploy to Firebase".into());
        server_feature_repo.upsert(&feature1).await.unwrap();
        server_feature_repo.upsert(&feature2).await.unwrap();
        let github_tool_id = feature1.id;

        // The space's auto-seeded Default FS is the resolver's baseline
        // when no binding matches — no "set active FS" step needed.
        let _ = fs_full_id;

        // Create test client — routing is per-session-root now, not per-client.
        let client = Client::new("TestClient", "test-type");
        let client_id = client.id.to_string();
        client_repo.create(&client).await.unwrap();

        let session_roots = SessionRootsRegistry::new();
        let session_id = "sess-meta".to_string();

        let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));
        let resolver = Arc::new(FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
            inbound_client_repo.clone(),
            feature_set_repo.clone(),
            Arc::new(SqliteSpaceBaseDirRepository::new(db.clone())),
            local_machine_id,
        ));

        let prefix_cache = Arc::new(PrefixCacheService::new());
        let feature_service = Arc::new(FeatureService::new(
            server_feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache.clone(),
        ));

        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let (tx, event_rx) = broadcast::channel::<DomainEvent>(32);
        let log_manager = test_log_manager();
        let server_manager =
            test_server_manager(tx.clone(), feature_service.clone(), prefix_cache.clone());
        let embedding_repo: Arc<dyn EmbeddingRepository> =
            Arc::new(SqliteEmbeddingRepository::new(db.clone()));

        let registry = meta_tools::build_default_registry(
            client_repo.clone(),
            space_repo.clone(),
            feature_set_repo.clone(),
            binding_repo.clone(),
            server_feature_repo.clone(),
            installed_server_repo.clone(),
            resolver,
            feature_service.clone(),
            None,
            None,
            session_roots.clone(),
            broker.clone(),
            tx,
            None,
            server_manager.clone(),
            log_manager,
            std::env::temp_dir().join(format!("mcpmux-meta-tools-{}", Uuid::new_v4())),
            embedding_repo,
        );

        Self {
            registry,
            broker,
            db,
            client_repo,
            feature_set_repo,
            server_feature_repo,
            binding_repo,
            installed_server_repo,
            session_roots,
            feature_service,
            space_id,
            client_id,
            session_id,
            fs_android_id,
            github_tool_id,
            event_rx,
            server_manager,
        }
    }

    /// Insert a machine catalog row for multi-device bind tests.
    pub(crate) async fn seed_machine(&self, name: &str) -> Uuid {
        let machine_repo = SqliteMachineRepository::new(self.db.clone());
        let machine = Machine::new(name);
        let id = machine.id;
        machine_repo.create(&machine).await.unwrap();
        id
    }

    /// Attach a publisher that always auto-approves with the given decision.
    fn attach_auto_publisher(&self, decision: ApprovalDecision) {
        let broker = self.broker.clone();
        let publisher: ApprovalPublisher = Arc::new(move |req| {
            let b = broker.clone();
            async move {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    b.respond(
                        &req.request_id,
                        &req.client_id,
                        &req.payload.tool_name,
                        decision,
                    );
                });
                true
            }
            .boxed()
        });
        // set_publisher is async; drive it synchronously via a current-runtime block_on
        // is unavailable here, so we spawn and detach — publisher is in place before
        // any request is made because tokio::test is single-threaded by default.
        let b = self.broker.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                b.set_publisher(publisher).await;
            });
        });
    }

    /// Attach a publisher that fans approval requests into the admin SSE bus.
    fn attach_sse_publisher(&self, ui_bus: Arc<mcpmux_gateway::admin::AdminUiEventBus>) {
        let publisher: ApprovalPublisher = Arc::new(move |req| {
            let bus = ui_bus.clone();
            async move {
                if let Ok(payload) = serde_json::to_value(&req) {
                    bus.publish(META_TOOL_APPROVAL_EVENT, payload);
                }
                true
            }
            .boxed()
        });
        let b = self.broker.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                b.set_publisher(publisher).await;
            });
        });
    }

    pub(crate) fn result_json(result: &rmcp::model::CallToolResult) -> Value {
        // CallToolResult's Content is opaque; round-trip through JSON and
        // pluck out the first text payload.
        let raw = serde_json::to_value(result).unwrap();
        raw.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("text"))
            .and_then(|t| t.as_str())
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or(raw)
    }

    fn is_error(result: &rmcp::model::CallToolResult) -> bool {
        result.is_error.unwrap_or(false)
    }

    /// Call the registry and normalize errors to `CallToolResult::error` the
    /// same way [`McpMuxGatewayHandler::call_tool`] does, so tests can assert
    /// the wire behaviour uniformly.
    async fn call_tool_as_handler_would(
        &self,
        name: &str,
        args: Value,
    ) -> rmcp::model::CallToolResult {
        match self
            .registry
            .call(name, &self.client_id, Some(&self.session_id), args)
            .await
        {
            Ok(r) => r,
            Err(e) => e.into_call_tool_result(),
        }
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn list_all_tools_not_in_agent_registry() {
    let f = Fixture::new().await;
    let names: Vec<_> = f
        .registry
        .list_as_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !names.iter().any(|n| n == "mcpmux_list_all_tools"),
        "catalog firehose removed from agent surface: {names:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_feature_sets_returns_space_contents() {
    let f = Fixture::new().await;
    let result = f
        .registry
        .call(
            "mcpmux_list_feature_sets",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let sets = body.get("feature_sets").unwrap().as_array().unwrap();
    // Seed created 2 custom FSes + the auto-seeded Default.
    assert_eq!(sets.len(), 3, "Default + 2 custom expected");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_feature_sets_marks_bound_vs_inactive() {
    let f = Fixture::new().await;
    let fs_id = bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_list_feature_sets",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let sets = body.get("feature_sets").unwrap().as_array().unwrap();
    let github_fs = sets
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(fs_id.as_str()))
        .unwrap();
    assert_eq!(github_fs.get("status"), Some(&json!("active")));
    let android = sets
        .iter()
        .find(|s| s.get("name").and_then(|v| v.as_str()) == Some("Android Dev"))
        .unwrap();
    assert_eq!(android.get("status"), Some(&json!("inactive")));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_default_empty_suggests_widen_or_bind() {
    let f = Fixture::new().await;
    let _fs_id = github_only_fs(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("total"), Some(&json!(0)));
    assert_eq!(body.get("scope"), Some(&json!("active_only")));
    let hint = body.get("hint").and_then(|v| v.as_str()).unwrap_or("");
    assert!(hint.contains("mcpmux_list_servers"));
    assert!(hint.contains("include_inactive"));
    assert!(body.get("inactive_preview").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn search_zero_result_surfaces_ready_inactive_preview() {
    let f = Fixture::new().await;
    let inactive_fs_id = bind_ready_github_with_inactive_create_issue(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "create issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("total"), Some(&json!(0)));
    assert_eq!(body.get("scope"), Some(&json!("active_only")));

    let preview = body
        .get("inactive_preview")
        .and_then(|v| v.as_array())
        .expect("expected inactive_preview for ready-but-inactive tools");
    assert!(!preview.is_empty());
    assert!(preview.len() <= 3);
    let tool = preview
        .iter()
        .find(|t| t.get("qualified_name") == Some(&json!("github_create_issue")))
        .expect("expected inactive github_create_issue in preview");
    assert_eq!(tool.get("status"), Some(&json!("inactive")));
    assert_eq!(
        tool.get("bindable_feature_set_id"),
        Some(&json!(inactive_fs_id))
    );
    assert_eq!(tool.get("server_readiness"), Some(&json!("ready")));

    let hint = body.get("hint").and_then(|v| v.as_str()).unwrap_or("");
    assert!(hint.contains("inactive_preview"));
    assert!(hint.contains("mcpmux_bind_current_workspace"));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_zero_result_generic_hint_when_no_ready_inactive() {
    let f = Fixture::new().await;
    let _fs_id = github_only_fs(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "create issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("total"), Some(&json!(0)));
    assert!(body.get("inactive_preview").is_none());
    let hint = body.get("hint").and_then(|v| v.as_str()).unwrap_or("");
    assert!(hint.contains("mcpmux_list_servers"));
    assert!(hint.contains("include_inactive"));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_first_meta_call_resolves_bound_workspace() {
    let f = Fixture::new().await;
    let root = "/tmp/mcpmux-root-race-first-search";
    let fs_id = github_only_fs(&f).await;

    f.session_roots.set_roots_capable(&f.session_id, true);
    let binding = WorkspaceBinding::new(normalize_workspace_root(root), f.space_id, fs_id.clone());
    f.binding_repo.create(&binding).await.unwrap();
    // Outcome of ensure_roots_probed before meta-tool dispatch (no prior tools/list).
    f.session_roots.set(&f.session_id, [root]);

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("scope"), Some(&json!("active_only")));
    assert!(
        body.get("total").and_then(|v| v.as_u64()).unwrap_or(0) >= 1,
        "bound workspace should surface active tools on first search: {body}"
    );
    let tools = body.get("tools").unwrap().as_array().unwrap();
    assert!(
        tools
            .iter()
            .any(|t| t.get("qualified_name") == Some(&json!("github_create_issue"))),
        "expected bound github tool in first search_tools result: {tools:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_surfaces_display_name_and_prefilled_required_params() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let mut github = InstalledServer::new(&f.space_id.to_string(), "github")
        .with_definition(&stdio_definition_with_required_input(
            "github",
            "github_token",
        ))
        .with_input("github_token", "secret")
        .with_display_name_override(Some("Jira - S2H"));
    github
        .default_params
        .insert("cloudId".to_string(), json!("site-uuid"));
    f.installed_server_repo.install(&github).await.unwrap();

    let mut create_issue = f
        .server_feature_repo
        .list_for_space(&f.space_id.to_string())
        .await
        .unwrap()
        .into_iter()
        .find(|feature| feature.feature_name == "create_issue")
        .expect("github create_issue feature");
    create_issue.raw_json = Some(json!({
        "name": "create_issue",
        "inputSchema": {
            "type": "object",
            "properties": {
                "cloudId": { "type": "string" },
                "title": { "type": "string" }
            },
            "required": ["cloudId", "title"]
        }
    }));
    f.server_feature_repo.upsert(&create_issue).await.unwrap();

    f.server_manager
        .set_connected(
            &ServerKey::new(f.space_id, "github"),
            CachedFeatures::default(),
        )
        .await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "create issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let tool = body
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t.get("qualified_name") == Some(&json!("github_create_issue")))
        .expect("github create_issue in search results");
    assert_eq!(tool.get("display_name"), Some(&json!("Jira - S2H")));
    let cloud_id = tool
        .get("required_params")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|param| param.get("name") == Some(&json!("cloudId")))
        .expect("cloudId required param");
    assert_eq!(cloud_id.get("prefilled"), Some(&json!(true)));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_pending_roots_returns_empty_active_only() {
    let f = Fixture::new().await;
    let root = "/tmp/mcpmux-root-race-pending";
    let fs_id = github_only_fs(&f).await;

    f.session_roots.set_roots_capable(&f.session_id, true);
    let binding = WorkspaceBinding::new(normalize_workspace_root(root), f.space_id, fs_id);
    f.binding_repo.create(&binding).await.unwrap();
    // Binding exists but roots not probed yet — PendingRoots → empty grants.

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("total"), Some(&json!(0)));
    assert_eq!(body.get("scope"), Some(&json!("active_only")));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_include_inactive_surfaces_bindable_github() {
    let f = Fixture::new().await;
    let fs_id = github_only_fs(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue", "include_inactive": true }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("scope"), Some(&json!("active_and_inactive")));
    assert!(body.get("total").and_then(|v| v.as_u64()).unwrap_or(0) >= 1);
    let tool = body
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t.get("qualified_name") == Some(&json!("github_create_issue")))
        .expect("inactive github tool in results");
    assert_eq!(tool.get("status"), Some(&json!("inactive")));
    assert_eq!(tool.get("bindable_feature_set_id"), Some(&json!(fs_id)));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_include_inactive_no_bundle_suggests_author_in_mux() {
    let f = Fixture::new().await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "deploy", "include_inactive": true }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("total"), Some(&json!(0)));
    let hint = body.get("hint").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        hint.contains("create a bundle") || hint.contains("Feature Sets"),
        "expected author-bundle hint, got: {hint}"
    );
}

/// Seed a PostHog-scale bundle (`tool_count` tools on one server) for inactive-scan perf tests.
async fn seed_large_inactive_bundle(f: &Fixture, server_id: &str, tool_count: usize) -> String {
    let space_id = f.space_id.to_string();
    let features: Vec<ServerFeature> = (0..tool_count)
        .map(|i| ServerFeature::tool(&space_id, server_id, format!("capture_event_{i}")))
        .collect();
    f.server_feature_repo.upsert_many(&features).await.unwrap();

    let mut fs = FeatureSet::new_custom("PostHog clone", space_id.clone());
    for feature in &features {
        fs.members.push(FeatureSetMember {
            id: Uuid::new_v4().to_string(),
            feature_set_id: fs.id.clone(),
            member_type: MemberType::Feature,
            member_id: feature.id.to_string(),
            mode: MemberMode::Include,
            surfaced: false,
        });
    }
    let fs_id = fs.id.clone();
    f.feature_set_repo.create(&fs).await.unwrap();
    fs_id
}

#[tokio::test(flavor = "multi_thread")]
async fn search_include_inactive_large_bundle_completes_under_two_seconds() {
    let f = Fixture::new().await;
    let tool_count = 450;
    seed_large_inactive_bundle(&f, "posthog", tool_count).await;

    let start = std::time::Instant::now();
    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "include_inactive": true, "limit": 100 }),
        )
        .await
        .unwrap();
    let elapsed = start.elapsed();

    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("scope"), Some(&json!("active_and_inactive")));
    assert!(
        body.get("total").and_then(|v| v.as_u64()).unwrap_or(0) >= tool_count as u64,
        "expected at least {tool_count} inactive tools"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "inactive scan took {elapsed:?}, expected < 2s"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_include_inactive_large_set_suggests_server_id_filter() {
    let f = Fixture::new().await;
    seed_large_inactive_bundle(&f, "analytics", 51).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "include_inactive": true, "limit": 10 }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let hint = body.get("hint").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        hint.contains("server_id"),
        "expected server_id filter hint, got: {hint}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_second_call_hits_active_index_cache() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let args = json!({ "query": "issue" });
    let result1 = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            args.clone(),
        )
        .await
        .unwrap();
    let body1 = Fixture::result_json(&result1);
    assert!(
        body1.get("total").and_then(|v| v.as_u64()).unwrap_or(0) >= 1,
        "first search should return active tools: {body1}"
    );
    assert!(f.registry.search_cache_contains(&f.session_id));

    f.server_feature_repo
        .delete(&f.github_tool_id)
        .await
        .unwrap();

    let result2 = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            args,
        )
        .await
        .unwrap();
    let body2 = Fixture::result_json(&result2);
    assert!(
        body2.get("total").and_then(|v| v.as_u64()).unwrap_or(0) >= 1,
        "cache hit should return cached tools despite DB deletion: {body2}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_cache_evicted_on_workspace_binding_changed() {
    let f = Fixture::new().await;
    let root = "/tmp/mcpmux-list-servers-test";
    bind_github_only_to_session_root(&f).await;

    f.registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    assert!(f.registry.search_cache_contains(&f.session_id));

    f.session_roots.evict_search_cache_for_workspace_root(root);

    assert!(!f.registry.search_cache_contains(&f.session_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_cache_evicted_on_session_disconnect() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    f.registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    assert!(f.registry.search_cache_contains(&f.session_id));

    f.session_roots.remove(&f.session_id);

    assert!(!f.registry.search_cache_contains(&f.session_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_ranking_lexical_when_model_absent() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "create issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("ranking").and_then(|v| v.as_str()),
        Some("lexical"),
        "without a ready embedding model search must label itself lexical: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_reuses_global_embeddings_across_sessions_without_reembedding_docs() {
    let f = Fixture::new().await;
    let root = "/tmp/mcpmux-list-servers-test";
    let _ = bind_github_only_to_session_root(&f).await;
    let content_hash = mcpmux_gateway::services::EmbeddingService::content_hash(
        "create_issue",
        Some("Create an issue"),
    );
    f.registry
        .context()
        .embedding_repo
        .upsert_many(&[EmbeddingRecord {
            content_hash: content_hash.clone(),
            model_version: f.registry.context().embeddings.model_version().to_string(),
            vector: vec![1.0, 0.0, 0.0],
        }])
        .await
        .unwrap();
    f.registry.context().embeddings.install_test_vectors(
        [("query: issue".to_string(), vec![1.0, 0.0, 0.0])]
            .into_iter()
            .collect(),
    );

    f.registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    assert_eq!(
        f.registry.context().embedding_store.len(),
        1,
        "first session should hydrate one shared vector"
    );

    let second_session_id = "sess-meta-reuse-2";
    f.session_roots.set_roots_capable(second_session_id, true);
    f.session_roots.set(second_session_id, [root]);
    f.registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(second_session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    assert_eq!(
        f.registry.context().embedding_store.len(),
        1,
        "second session should reuse the shared vector"
    );
    assert!(
        f.registry
            .context()
            .embedding_store
            .contains_key(&content_hash),
        "global embedding store should keep the content hash"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_tools_reuses_persisted_embeddings_after_registry_restart() {
    let shared_db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
    let seeded = Fixture::new_with_db(shared_db.clone(), None).await;
    let _ = bind_github_only_to_session_root(&seeded).await;
    let content_hash = mcpmux_gateway::services::EmbeddingService::content_hash(
        "create_issue",
        Some("Create an issue"),
    );
    seeded
        .registry
        .context()
        .embedding_repo
        .upsert_many(&[EmbeddingRecord {
            content_hash: content_hash.clone(),
            model_version: seeded
                .registry
                .context()
                .embeddings
                .model_version()
                .to_string(),
            vector: vec![1.0, 0.0, 0.0],
        }])
        .await
        .unwrap();

    let restarted = Fixture::new_with_db(shared_db, None).await;
    let root = "/tmp/mcpmux-list-servers-test";
    restarted
        .session_roots
        .set_roots_capable(&restarted.session_id, true);
    restarted.session_roots.set(&restarted.session_id, [root]);
    restarted
        .registry
        .context()
        .embeddings
        .install_test_vectors(
            [("query: issue".to_string(), vec![1.0, 0.0, 0.0])]
                .into_iter()
                .collect(),
        );
    assert_eq!(
        restarted.registry.context().embedding_store.len(),
        0,
        "new registry starts with an empty in-memory embedding store"
    );

    let result = restarted
        .registry
        .call(
            "mcpmux_search_tools",
            &restarted.client_id,
            Some(&restarted.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("ranking").and_then(|value| value.as_str()),
        Some("hybrid"),
        "persisted vectors should be rehydrated after restart: {body}"
    );
    assert!(
        restarted
            .registry
            .context()
            .embedding_store
            .contains_key(&content_hash),
        "restart should hydrate persisted vectors by content hash"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_event_warms_server_catalog_embeddings_before_search() {
    let f = Fixture::new().await;
    let _ = bind_github_only_to_session_root(&f).await;
    let content_hash = mcpmux_gateway::services::EmbeddingService::content_hash(
        "create_issue",
        Some("Create an issue"),
    );
    f.registry.context().embeddings.install_test_vectors(
        [
            (
                "passage: create_issue Create an issue".to_string(),
                vec![1.0, 0.0, 0.0],
            ),
            ("query: issue".to_string(), vec![1.0, 0.0, 0.0]),
        ]
        .into_iter()
        .collect(),
    );

    let warmer = Arc::new(EmbeddingWarmer::new(
        f.server_feature_repo.clone(),
        f.registry.context().embedding_repo.clone(),
        f.registry.context().embedding_store.clone(),
        f.registry.context().embeddings.clone(),
    ));
    let notifier = Arc::new(MCPNotifier::new(
        f.registry.context().resolver.clone(),
        f.feature_service.clone(),
    ));
    notifier.set_embedding_warmer(warmer);
    notifier.clone().start(f.event_rx.resubscribe());

    let server_key = ServerKey::new(f.space_id, "github");
    f.server_manager
        .set_connected(&server_key, CachedFeatures::default())
        .await;

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !f
        .registry
        .context()
        .embedding_store
        .contains_key(&content_hash)
        && std::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        f.registry
            .context()
            .embedding_store
            .contains_key(&content_hash),
        "expected connect warmer to populate in-memory vector map"
    );

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("ranking").and_then(|value| value.as_str()),
        Some("hybrid"),
        "search should find pre-warmed vectors after server connect: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_does_not_promote_tools_into_advertised_list() {
    let f = Fixture::new().await;
    let meta_count = f.registry.list_as_tools().len();
    let fs_id = bind_github_only_to_session_root(&f).await;

    let advertised = f
        .feature_service
        .get_advertised_tools_for_grants(&f.space_id.to_string(), &[fs_id])
        .await
        .unwrap();
    assert!(
        advertised.is_empty(),
        "binding must not surface backend tools into tools/list"
    );
    assert_eq!(f.registry.list_as_tools().len(), meta_count);
}

#[tokio::test(flavor = "multi_thread")]
async fn invokable_but_not_surfaced_tool_excluded_from_advertised_list() {
    let f = Fixture::new().await;
    let fs_id = bind_github_only_to_session_root(&f).await;

    let invokable = f
        .feature_service
        .get_invokable_tools_for_grants(&f.space_id.to_string(), &[fs_id.clone()])
        .await
        .unwrap();
    assert!(
        !invokable.is_empty(),
        "bound github FS should grant invokable tools"
    );

    let advertised = f
        .feature_service
        .get_advertised_tools_for_grants(&f.space_id.to_string(), &[fs_id])
        .await
        .unwrap();
    assert!(
        advertised.is_empty(),
        "non-surfaced tools must stay off tools/list"
    );

    let tool = invokable.first().unwrap();
    let redirect = mcpmux_gateway::pool::format_direct_call_redirect(
        &tool.qualified_name(),
        &tool.server_id,
        &tool.feature_name,
    );
    assert!(redirect.contains("mcpmux_invoke_tool"));
}

fn server_readiness(body: &Value, server_id: &str) -> String {
    body.get("servers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(server_id))
        .unwrap()
        .get("readiness")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

async fn github_only_fs(f: &Fixture) -> String {
    let mut fs = FeatureSet::new_custom("GitHub only", f.space_id.to_string());
    fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: fs.id.clone(),
        member_type: MemberType::Feature,
        member_id: f.github_tool_id.to_string(),
        mode: MemberMode::Include,
        surfaced: false,
    });
    let id = fs.id.clone();
    f.feature_set_repo.create(&fs).await.unwrap();
    id
}

async fn bind_github_only_to_session_root(f: &Fixture) -> String {
    use mcpmux_core::WorkspaceBinding;

    let fs_id = github_only_fs(f).await;
    let root = "/tmp/mcpmux-list-servers-test";
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots.set(&f.session_id, [root]);
    let binding = WorkspaceBinding::new(normalize_workspace_root(root), f.space_id, fs_id.clone());
    f.binding_repo.create(&binding).await.unwrap();
    fs_id
}

/// Bind a workspace to a FeatureSet that includes only `list_repos`, leaving
/// `create_issue` in an unbound FeatureSet so the server is ready but the issue
/// tool stays inactive.
async fn bind_ready_github_with_inactive_create_issue(f: &Fixture) -> String {
    use mcpmux_core::WorkspaceBinding;

    let mut list_repos = ServerFeature::tool(f.space_id, "github", "list_repos");
    list_repos.description = Some("List repositories".into());
    f.server_feature_repo.upsert(&list_repos).await.unwrap();

    let mut bound_fs = FeatureSet::new_custom("GitHub bound slice", f.space_id.to_string());
    bound_fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: bound_fs.id.clone(),
        member_type: MemberType::Feature,
        member_id: list_repos.id.to_string(),
        mode: MemberMode::Include,
        surfaced: false,
    });
    let bound_fs_id = bound_fs.id.clone();
    f.feature_set_repo.create(&bound_fs).await.unwrap();
    let inactive_fs_id = github_only_fs(f).await;

    seed_diagnose_servers(f).await;

    let root = "/tmp/mcpmux-ready-inactive-preview";
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots.set(&f.session_id, [root]);
    let binding = WorkspaceBinding::new(
        normalize_workspace_root(root),
        f.space_id,
        bound_fs_id.clone(),
    );
    f.binding_repo.create(&binding).await.unwrap();
    inactive_fs_id
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_includes_prefilled_params_when_default_params_set() {
    let f = Fixture::new().await;
    let mut github = InstalledServer::new(&f.space_id.to_string(), "github").with_definition(
        &stdio_definition_with_required_input("github", "github_token"),
    );
    github
        .default_params
        .insert("cloudId".to_string(), json!("site-uuid"));
    f.installed_server_repo.install(&github).await.unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let github_entry = body
        .get("servers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s.get("id") == Some(&json!("github")))
        .expect("github server in list");
    assert_eq!(
        github_entry.get("prefilled_params"),
        Some(&json!(["cloudId"]))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_marks_unbound_servers_bindable() {
    let f = Fixture::new().await;
    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    let servers = body.get("servers").unwrap().as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(server_readiness(&body, "github"), "bindable");
    assert_eq!(server_readiness(&body, "firebase"), "bindable");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_inactive_includes_bindable_feature_set_ids() {
    let f = Fixture::new().await;
    let fs_id = github_only_fs(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let github = body
        .get("servers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some("github"))
        .unwrap();
    assert_eq!(github.get("readiness"), Some(&json!("bindable")));
    let bindable = github
        .get("bindable_feature_set_ids")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(bindable.iter().any(|v| v.as_str() == Some(fs_id.as_str())));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_bound_server_reports_bound_when_disconnected() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(server_readiness(&body, "github"), "bound");
    assert_eq!(
        body.get("servers")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s.get("id") == Some(&json!("github")))
            .unwrap()
            .get("blocking_reason"),
        Some(&json!("disconnected"))
    );
    assert_eq!(server_readiness(&body, "firebase"), "bindable");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_includes_cloned_from_for_clone_installs() {
    let f = Fixture::new().await;
    let space_id = f.space_id.to_string();

    let posthog = InstalledServer::new(&space_id, "posthog");
    f.installed_server_repo.install(&posthog).await.unwrap();
    let posthog_work = InstalledServer::new(&space_id, "posthog-work").with_cloned_from("posthog");
    f.installed_server_repo
        .install(&posthog_work)
        .await
        .unwrap();

    let mut clone_tool = ServerFeature::tool(f.space_id, "posthog-work", "capture");
    clone_tool.display_name = Some("PostHog (work)".into());
    f.registry
        .context()
        .server_feature_repo
        .upsert(&clone_tool)
        .await
        .unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let clone_entry = body
        .get("servers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some("posthog-work"))
        .expect("clone server in manifest");
    assert_eq!(
        clone_entry.get("cloned_from").and_then(|v| v.as_str()),
        Some("posthog")
    );

    let github_entry = body
        .get("servers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some("github"))
        .expect("github in manifest");
    assert!(github_entry.get("cloned_from").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_shows_installed_server_with_no_tool_features() {
    let f = Fixture::new().await;
    let space_id = f.space_id.to_string();

    // Install a server that has a required input but no server_feature rows
    // (simulates a freshly installed server whose tool catalog has not been
    // discovered yet, e.g. waiting for the user to supply credentials).
    let def = stdio_definition_with_required_input("brand-new", "api_key");
    let server = InstalledServer::new(&space_id, "brand-new").with_definition(&def);
    f.installed_server_repo.install(&server).await.unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);

    let servers = body.get("servers").unwrap().as_array().unwrap();
    let entry = servers
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some("brand-new"))
        .expect("installed server with no tool features must appear in list_servers");

    assert_eq!(entry.get("tool_count"), Some(&json!(0)));
    assert_eq!(entry.get("health"), Some(&json!("needs_setup")));
    let missing = entry
        .get("missing_inputs")
        .and_then(|v| v.as_array())
        .expect("missing_inputs must be present for a needs_setup server");
    assert!(
        missing.iter().any(|v| v.as_str() == Some("api_key")),
        "api_key must appear in missing_inputs: {missing:?}"
    );
}

// ---------------------------------------------------------------------------
// Writes — gated by ApprovalBroker
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn write_without_publisher_returns_approval_required() {
    let f = Fixture::new().await;
    let input = if cfg!(windows) {
        "D:\\Projects\\Approval\\"
    } else {
        "/proj/approval"
    };
    f.session_roots.set(&f.session_id, [input]);
    let result = f
        .call_tool_as_handler_would(
            "mcpmux_bind_current_workspace",
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await;
    assert!(Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "approval_required"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn write_rejected_on_deny_leaves_state_unchanged() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::Deny);

    let before_bindings = f.binding_repo.list().await.unwrap().len();

    let input = if cfg!(windows) {
        "D:\\Projects\\Denied\\"
    } else {
        "/proj/denied"
    };
    f.session_roots.set(&f.session_id, [input]);
    let result = f
        .call_tool_as_handler_would(
            "mcpmux_bind_current_workspace",
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await;
    assert!(Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "approval_denied"
    );

    let after_bindings = f.binding_repo.list().await.unwrap().len();
    assert_eq!(after_bindings, before_bindings);
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_approval_surfaces_on_admin_sse_and_approve_writes_binding() {
    let f = Fixture::new().await;
    let ui_bus = Arc::new(mcpmux_gateway::admin::AdminUiEventBus::new());
    let mut sse_rx = ui_bus.subscribe();
    f.attach_sse_publisher(ui_bus);

    let input = if cfg!(windows) {
        "D:\\Projects\\WebAdmin\\"
    } else {
        "/proj/web-admin-bind"
    };
    f.session_roots.set(&f.session_id, [input]);

    let registry = f.registry.clone();
    let client_id = f.client_id.clone();
    let session_id = f.session_id.clone();
    let fs_id = f.fs_android_id.to_string();
    let broker = f.broker.clone();

    let bind_task = tokio::spawn(async move {
        registry
            .call(
                "mcpmux_bind_current_workspace",
                &client_id,
                Some(&session_id),
                json!({ "feature_set_id": fs_id }),
            )
            .await
    });

    let ui_event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match sse_rx.recv().await {
                Ok(ev) if ev.channel == META_TOOL_APPROVAL_EVENT => return ev.payload,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("SSE bus closed before approval request");
                }
            }
        }
    })
    .await
    .expect("approval request on admin SSE");

    let request_id = ui_event
        .get("request_id")
        .and_then(|v| v.as_str())
        .expect("request_id in SSE payload");
    broker.respond(
        request_id,
        &f.client_id,
        "mcpmux_bind_current_workspace",
        ApprovalDecision::AllowOnce,
    );

    let result = bind_task.await.expect("bind task").expect("bind call");
    assert!(!Fixture::is_error(&result));

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].workspace_root, normalize_workspace_root(input));
    assert_eq!(
        bindings[0].feature_set_ids,
        vec![f.fs_android_id.to_string()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_deny_via_admin_sse_leaves_state_unchanged() {
    let f = Fixture::new().await;
    let ui_bus = Arc::new(mcpmux_gateway::admin::AdminUiEventBus::new());
    let mut sse_rx = ui_bus.subscribe();
    f.attach_sse_publisher(ui_bus);

    let before_bindings = f.binding_repo.list().await.unwrap().len();
    let input = if cfg!(windows) {
        "D:\\Projects\\WebDenied\\"
    } else {
        "/proj/web-admin-deny"
    };
    f.session_roots.set(&f.session_id, [input]);

    let registry = f.registry.clone();
    let client_id = f.client_id.clone();
    let session_id = f.session_id.clone();
    let fs_id = f.fs_android_id.to_string();
    let broker = f.broker.clone();

    let bind_task = tokio::spawn(async move {
        registry
            .call(
                "mcpmux_bind_current_workspace",
                &client_id,
                Some(&session_id),
                json!({ "feature_set_id": fs_id }),
            )
            .await
    });

    let ui_event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match sse_rx.recv().await {
                Ok(ev) if ev.channel == META_TOOL_APPROVAL_EVENT => return ev.payload,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("SSE bus closed before approval request");
                }
            }
        }
    })
    .await
    .expect("approval request on admin SSE");

    let request_id = ui_event
        .get("request_id")
        .and_then(|v| v.as_str())
        .expect("request_id in SSE payload");
    broker.respond(
        request_id,
        &f.client_id,
        "mcpmux_bind_current_workspace",
        ApprovalDecision::Deny,
    );

    let result = match bind_task.await.expect("bind task") {
        Ok(r) => r,
        Err(e) => e.into_call_tool_result(),
    };
    assert!(Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "approval_denied"
    );

    let after_bindings = f.binding_repo.list().await.unwrap().len();
    assert_eq!(after_bindings, before_bindings);
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_fails_when_no_roots_reported() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    // NOTE: session_roots intentionally NOT populated.

    let result = f
        .call_tool_as_handler_would(
            "mcpmux_bind_current_workspace",
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await;
    assert!(Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "invalid_argument"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_creates_binding_with_normalized_root() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\Android\\MyApp\\"
    } else {
        "/home/me/projects/android/myapp/"
    };
    f.session_roots.set(&f.session_id, [input]);

    let result = f
        .registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1);
    let stored = &bindings[0].workspace_root;
    // Drive-letter lowercased, trailing separator trimmed.
    assert_eq!(stored, &normalize_workspace_root(input));
    assert!(!stored.ends_with('/') && !stored.ends_with('\\'));
    // Binding points at the concrete FS we passed in.
    assert_eq!(bindings[0].space_id, f.space_id);
    assert_eq!(
        bindings[0].feature_set_ids,
        vec![f.fs_android_id.to_string()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_layers_onto_existing_binding() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\Android\\MyApp\\"
    } else {
        "/home/me/projects/android/myapp/"
    };
    let normalized = normalize_workspace_root(input);
    f.session_roots.set(&f.session_id, [input]);

    let fs_full_id = {
        let sets = f
            .feature_set_repo
            .list_by_space(&f.space_id.to_string())
            .await
            .unwrap();
        let full = sets
            .iter()
            .find(|fs| fs.name == "Full Access")
            .expect("Full Access FS");
        Uuid::parse_str(&full.id).unwrap()
    };

    // Seed an existing binding (simulates Workspaces UI or prior bind).
    let starter =
        WorkspaceBinding::new(normalized.clone(), f.space_id, f.fs_android_id.to_string());
    f.binding_repo.create(&starter).await.unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": fs_full_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1, "must not insert a second binding row");
    assert_eq!(bindings[0].id, starter.id, "must reuse existing binding id");
    assert_eq!(bindings[0].workspace_root, normalized);
    assert_eq!(bindings[0].feature_set_ids.len(), 2);
    assert!(bindings[0]
        .feature_set_ids
        .contains(&f.fs_android_id.to_string()));
    assert!(bindings[0]
        .feature_set_ids
        .contains(&fs_full_id.to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_rebind_is_idempotent() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\Android\\Rebind\\"
    } else {
        "/home/me/projects/android/rebind/"
    };
    f.session_roots.set(&f.session_id, [input]);
    let fs_id = github_only_fs(&f).await;
    let args = json!({ "feature_set_id": fs_id });

    f.registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            args.clone(),
        )
        .await
        .unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            args,
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("already_bound"), Some(&json!(true)));

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].feature_set_ids, vec![fs_id]);
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_writes_machine_scoped_when_local_machine_set() {
    let f = Fixture::new_with_local_machine("Gondor").await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\MachineScoped\\"
    } else {
        "/home/me/projects/machine-scoped/"
    };
    f.session_roots.set(&f.session_id, [input]);

    let result = f
        .registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("active"), Some(&json!(true)));
    assert_eq!(body.get("already_bound"), Some(&json!(false)));

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1);
    assert!(bindings[0].client_id.is_none());
    assert!(bindings[0].machine_id.is_some());
    assert_eq!(
        bindings[0].feature_set_ids,
        vec![f.fs_android_id.to_string()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_header_targets_caller_machine_not_gateway_local() {
    let f = Fixture::new_with_local_machine("Gondor").await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let rohan_id = f.seed_machine("Rohan").await;
    let input = if cfg!(windows) {
        "D:\\Projects\\TunnelScoped\\"
    } else {
        "/home/me/projects/tunnel-scoped/"
    };
    f.session_roots.set(&f.session_id, [input]);

    let result = f
        .registry
        .call_from_device(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
            Some(rohan_id),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("machine_id"), Some(&json!(rohan_id)));
    assert_eq!(body.get("active"), Some(&json!(true)));

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].machine_id, Some(rohan_id));
    assert!(bindings[0].client_id.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_second_session_inherits_binding() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\Android\\Persist\\"
    } else {
        "/home/me/projects/android/persist/"
    };
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots.set(&f.session_id, [input]);
    let fs_id = github_only_fs(&f).await;

    f.registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": fs_id }),
        )
        .await
        .unwrap();

    let new_session = "sess-bind-inherit";
    f.session_roots.set_roots_capable(new_session, true);
    f.session_roots.set(new_session, [input]);

    let resolved = f
        .registry
        .context()
        .resolver
        .resolve(Some(new_session), Some(&f.client_id), None)
        .await
        .unwrap();
    assert!(
        resolved.feature_set_ids.iter().any(|id| id == &fs_id),
        "second session should resolve the bound FeatureSet"
    );

    let tools = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &resolved.feature_set_ids)
        .await
        .unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server_id, "github");
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_feature_set_argument_rejected() {
    let f = Fixture::new().await;
    let input = if cfg!(windows) {
        "D:\\Projects\\Invalid\\"
    } else {
        "/proj/invalid"
    };
    f.session_roots.set(&f.session_id, [input]);
    let result = f
        .call_tool_as_handler_would(
            "mcpmux_bind_current_workspace",
            json!({ "feature_set_id": "not-a-uuid" }),
        )
        .await;
    assert!(Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "invalid_argument"
    );
}

// ---------------------------------------------------------------------------
// Registry list-as-tools shape
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn registry_advertises_core_tools_read_only_in_list() {
    let f = Fixture::new().await;
    let tools = f.registry.list_as_tools();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names.len(), meta_tools::CORE_META_TOOLS.len());
    for core in meta_tools::CORE_META_TOOLS {
        assert!(names.iter().any(|n| n == *core), "missing {core}");
    }
    for hidden in [
        "mcpmux_list_feature_sets",
        "mcpmux_search_resources",
        "mcpmux_read_resource",
        "mcpmux_search_prompts",
        "mcpmux_fetch_prompt",
        "mcpmux_diagnose_server",
    ] {
        assert!(
            !names.iter().any(|n| n == hidden),
            "{hidden} must not be advertised; got {names:?}"
        );
    }
    for tool in &tools {
        let destructive = tool
            .annotations
            .as_ref()
            .and_then(|a| a.destructive_hint)
            .unwrap_or(false);
        if tool.name.as_ref() == "mcpmux_bind_current_workspace" {
            assert!(
                destructive,
                "bind must be annotated as a write tool: {:?}",
                tool.name
            );
            continue;
        }
        assert!(
            !destructive,
            "advertised core tools must be read-only hints: {:?}",
            tool.name
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unbound_session_lists_bind_current_workspace() {
    let f = Fixture::new().await;
    let root = "/tmp/mcpmux-unbound-list-tools";
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots.set(&f.session_id, [root]);

    let advertised: Vec<_> = f
        .registry
        .list_as_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        advertised.iter().any(|n| n == "mcpmux_bind_current_workspace"),
        "Unbound sessions must advertise bind in tools/list: {advertised:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unbound_session_invoke_tool_returns_bind_denial_hint() {
    let f = Fixture::new().await;
    let root = "/tmp/mcpmux-unbound-invoke-denial";
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots.set(&f.session_id, [root]);

    let result = f
        .registry
        .call(
            "mcpmux_invoke_tool",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "github", "tool": "create_issue" }),
        )
        .await
        .unwrap();
    assert!(
        Fixture::is_error(&result),
        "backend invoke must be denied for Unbound sessions"
    );
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").and_then(|v| v.as_str()),
        Some("not_ready")
    );
    assert_eq!(
        body.get("reason").and_then(|v| v.as_str()),
        Some("inactive")
    );
    assert_eq!(
        body.get("tool").and_then(|v| v.as_str()),
        Some("mcpmux_bind_current_workspace")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hidden_list_feature_sets_callable_but_not_advertised() {
    let f = Fixture::new().await;
    let advertised: Vec<_> = f
        .registry
        .list_as_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !advertised.iter().any(|n| n == "mcpmux_list_feature_sets"),
        "list_feature_sets stays off tools/list"
    );
    assert!(f.registry.contains("mcpmux_list_feature_sets"));

    let result = f
        .registry
        .call(
            "mcpmux_list_feature_sets",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await;
    assert!(result.is_ok(), "hidden read tool must remain callable");
}

#[tokio::test(flavor = "multi_thread")]
async fn search_scope_all_matches_include_inactive() {
    let f = Fixture::new().await;
    let fs_id = github_only_fs(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue", "scope": "all" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("scope"), Some(&json!("active_and_inactive")));
    let tool = body
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t.get("qualified_name") == Some(&json!("github_create_issue")))
        .expect("inactive github tool in results");
    assert_eq!(tool.get("status"), Some(&json!("inactive")));
    assert_eq!(tool.get("bindable_feature_set_id"), Some(&json!(fs_id)));
}

// ---------------------------------------------------------------------------
// MetaToolInvoked audit emission + master switch
// ---------------------------------------------------------------------------

/// Build a bare registry (no fixture sugar) so tests can subscribe to the
/// event bus before the first call or flip the master-switch setting.
async fn bare_registry(
    settings_repo: Option<Arc<dyn mcpmux_core::AppSettingsRepository>>,
) -> (
    Arc<MetaToolRegistry>,
    String,
    broadcast::Sender<DomainEvent>,
    broadcast::Receiver<DomainEvent>,
) {
    let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
    let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
    let feature_set_repo: Arc<dyn FeatureSetRepository> =
        Arc::new(SqliteFeatureSetRepository::new(db.clone()));
    let client_repo: Arc<dyn InboundMcpClientRepository> =
        Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));
    let binding_repo: Arc<dyn WorkspaceBindingRepository> =
        Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
    let server_feature_repo: Arc<dyn ServerFeatureRepository> =
        Arc::new(SqliteServerFeatureRepository::new(db.clone()));
    let installed_server_repo: Arc<dyn InstalledServerRepository> = Arc::new(
        SqliteInstalledServerRepository::new(db.clone(), test_encryptor()),
    );

    let _space = space_repo.get_default().await.unwrap().unwrap();
    let client = Client::new("c", "t");
    let client_id = client.id.to_string();
    client_repo.create(&client).await.unwrap();

    let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));
    let resolver = Arc::new(FeatureSetResolverService::new(
        space_repo.clone(),
        binding_repo.clone(),
        SessionRootsRegistry::new(),
        inbound_client_repo.clone(),
        feature_set_repo.clone(),
        Arc::new(SqliteSpaceBaseDirRepository::new(db.clone())),
        None,
    ));
    let prefix_cache = Arc::new(PrefixCacheService::new());
    let feature_service = Arc::new(FeatureService::new(
        server_feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache.clone(),
    ));
    let (tx, rx) = broadcast::channel::<DomainEvent>(32);
    let log_manager = test_log_manager();
    let server_manager = test_server_manager(tx.clone(), feature_service.clone(), prefix_cache);
    let registry = meta_tools::build_default_registry(
        client_repo,
        space_repo,
        feature_set_repo,
        binding_repo,
        server_feature_repo,
        installed_server_repo,
        resolver,
        feature_service,
        None,
        None,
        SessionRootsRegistry::new(),
        Arc::new(ApprovalBroker::new()),
        tx.clone(),
        settings_repo,
        server_manager,
        log_manager,
        std::env::temp_dir().join(format!("mcpmux-bare-registry-{}", Uuid::new_v4())),
        Arc::new(SqliteEmbeddingRepository::new(db.clone())),
    );
    (registry, client_id, tx, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn read_tool_emits_meta_tool_invoked_with_decision_read() {
    let (registry, client_id, _tx, mut rx) = bare_registry(None).await;

    registry
        .call("mcpmux_list_servers", &client_id, Some("s"), json!({}))
        .await
        .unwrap();

    let evt = tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("receive within 200ms")
        .expect("event");
    match evt {
        DomainEvent::MetaToolInvoked {
            tool_name,
            decision,
            ..
        } => {
            assert_eq!(tool_name, "mcpmux_list_servers");
            assert_eq!(decision, "read");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn denied_write_emits_meta_tool_invoked_with_decision_deny() {
    let (registry, client_id, _tx, mut rx) = bare_registry(None).await;

    // No publisher → write fails with ApprovalRequiredNoDesktop, which the
    // registry's central audit-logger records as `approval_required`.
    let _ = registry
        .call(
            "mcpmux_bind_current_workspace",
            &client_id,
            Some("s"),
            json!({ "feature_set_id": Uuid::new_v4().to_string() }),
        )
        .await;
    let evt = tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("receive within 200ms")
        .expect("event");
    match evt {
        DomainEvent::MetaToolInvoked {
            decision,
            tool_name,
            ..
        } => {
            assert_eq!(tool_name, "mcpmux_bind_current_workspace");
            // bind_current_workspace bails on "invalid_args" (missing reported
            // roots) before it reaches the approval broker — the audit
            // logger records the bail-out reason, not approval_required.
            assert_eq!(decision, "invalid_args");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn master_switch_toggles_registry_visibility() {
    use mcpmux_storage::SqliteAppSettingsRepository;

    // Same DB so the settings repo and the registry see one another.
    let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
    let settings_repo: Arc<dyn mcpmux_core::AppSettingsRepository> =
        Arc::new(SqliteAppSettingsRepository::new(db.clone()));
    settings_repo
        .set("gateway.meta_tools_enabled", "false")
        .await
        .unwrap();

    let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
    let feature_set_repo: Arc<dyn FeatureSetRepository> =
        Arc::new(SqliteFeatureSetRepository::new(db.clone()));
    let client_repo: Arc<dyn InboundMcpClientRepository> =
        Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));
    let binding_repo: Arc<dyn WorkspaceBindingRepository> =
        Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
    let server_feature_repo: Arc<dyn ServerFeatureRepository> =
        Arc::new(SqliteServerFeatureRepository::new(db.clone()));
    let installed_server_repo: Arc<dyn InstalledServerRepository> = Arc::new(
        SqliteInstalledServerRepository::new(db.clone(), test_encryptor()),
    );
    let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));
    let resolver = Arc::new(FeatureSetResolverService::new(
        space_repo.clone(),
        binding_repo.clone(),
        SessionRootsRegistry::new(),
        inbound_client_repo.clone(),
        feature_set_repo.clone(),
        Arc::new(SqliteSpaceBaseDirRepository::new(db.clone())),
        None,
    ));
    let prefix_cache = Arc::new(PrefixCacheService::new());
    let feature_service = Arc::new(FeatureService::new(
        server_feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache.clone(),
    ));
    let (tx, _) = broadcast::channel::<DomainEvent>(16);
    let log_manager = test_log_manager();
    let server_manager = test_server_manager(tx.clone(), feature_service.clone(), prefix_cache);
    let registry = meta_tools::build_default_registry(
        client_repo,
        space_repo,
        feature_set_repo,
        binding_repo,
        server_feature_repo,
        installed_server_repo,
        resolver,
        feature_service,
        None,
        None,
        SessionRootsRegistry::new(),
        Arc::new(ApprovalBroker::new()),
        tx,
        Some(settings_repo.clone()),
        server_manager,
        log_manager,
        std::env::temp_dir().join(format!("mcpmux-meta-switch-{}", Uuid::new_v4())),
        Arc::new(SqliteEmbeddingRepository::new(db.clone())),
    );

    assert!(!registry.is_enabled().await, "initially disabled");

    settings_repo
        .set("gateway.meta_tools_enabled", "true")
        .await
        .unwrap();
    assert!(registry.is_enabled().await, "flipped back on");

    // Missing key → default on (fresh install).
    settings_repo
        .delete("gateway.meta_tools_enabled")
        .await
        .unwrap();
    assert!(registry.is_enabled().await, "missing key defaults on");
}

// Silence unused-import warnings from helper imports that only some tests exercise.
#[allow(dead_code)]
fn _unused(_: ApprovalPayload) {}

// ---------------------------------------------------------------------------
// Readiness, browse mode, invoke_example (agent UX path)
// ---------------------------------------------------------------------------

/// Seed `tool_count` alphabetically named tools on one server for browse pagination tests.
async fn seed_browse_tools(f: &Fixture, server_id: &str, tool_count: usize) -> String {
    let space_id = f.space_id.to_string();
    let features: Vec<ServerFeature> = (0..tool_count)
        .map(|i| {
            let mut tool = ServerFeature::tool(&space_id, server_id, format!("tool_{i:03}"));
            tool.raw_json = Some(json!({
                "name": format!("tool_{i:03}"),
                "inputSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "integer" } },
                    "required": ["id"]
                }
            }));
            tool
        })
        .collect();
    f.server_feature_repo.upsert_many(&features).await.unwrap();

    let mut fs = FeatureSet::new_custom("Browse bundle", space_id.clone());
    for feature in &features {
        fs.members.push(FeatureSetMember {
            id: Uuid::new_v4().to_string(),
            feature_set_id: fs.id.clone(),
            member_type: MemberType::Feature,
            member_id: feature.id.to_string(),
            mode: MemberMode::Include,
            surfaced: false,
        });
    }
    let fs_id = fs.id.clone();
    f.feature_set_repo.create(&fs).await.unwrap();
    fs_id
}

#[tokio::test(flavor = "multi_thread")]
async fn browse_mode_default_limit_fifty_and_alphabetical() {
    let f = Fixture::new().await;
    let fs_id = seed_browse_tools(&f, "catalog", 55).await;
    let root = "/tmp/mcpmux-browse-limit";
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots.set(&f.session_id, [root]);
    let binding = WorkspaceBinding::new(normalize_workspace_root(root), f.space_id, fs_id);
    f.binding_repo.create(&binding).await.unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "catalog" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("mode"), Some(&json!("browse")));
    assert_eq!(body.get("total"), Some(&json!(55)));
    let tools = body.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 50);
    assert!(body.get("next_cursor").is_some());
    assert_eq!(
        tools[0].get("qualified_name"),
        Some(&json!("catalog_tool_000"))
    );
    assert_eq!(
        tools[1].get("qualified_name"),
        Some(&json!("catalog_tool_001"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn browse_hits_include_invoke_example_and_server_readiness() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "github", "mode": "browse" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let tool = body
        .get("tools")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t.get("qualified_name") == Some(&json!("github_create_issue")))
        .expect("create_issue in browse");
    assert_eq!(tool.get("server_readiness"), Some(&json!("bound")));
    let example = tool
        .get("invoke_example")
        .expect("invoke_example on browse");
    assert_eq!(example.get("server_id"), Some(&json!("github")));
    assert_eq!(example.get("tool"), Some(&json!("create_issue")));
}

#[tokio::test(flavor = "multi_thread")]
async fn browse_mode_without_server_id_lists_whole_space() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "mode": "browse" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("mode"), Some(&json!("browse")));
    let tools = body.get("tools").unwrap().as_array().expect("browse page");
    assert!(
        !tools.is_empty(),
        "whole-space browse must return tools: {body}"
    );
    assert!(
        tools
            .iter()
            .any(|t| t.get("server_id") == Some(&json!("github"))),
        "expected github tools in whole-space browse: {tools:?}"
    );
    let names: Vec<_> = tools
        .iter()
        .filter_map(|t| t.get("qualified_name").and_then(|v| v.as_str()))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "browse must be alphabetical");
}

#[tokio::test(flavor = "multi_thread")]
async fn ranked_search_omits_invoke_example() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_search_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({ "query": "issue", "server_id": "github" }),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert!(body.get("mode").is_none());
    let tool = body.get("tools").unwrap().as_array().unwrap()[0].clone();
    assert!(tool.get("invoke_example").is_none());
    assert!(tool.get("server_readiness").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_ready_when_bound_and_connected() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;
    let space_id = f.space_id.to_string();
    let github = InstalledServer::new(&space_id, "github");
    f.installed_server_repo.install(&github).await.unwrap();
    f.server_manager
        .set_connected(
            &ServerKey::new(f.space_id, "github"),
            CachedFeatures::default(),
        )
        .await;

    let result = f
        .registry
        .call(
            "mcpmux_list_servers",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(server_readiness(&body, "github"), "ready");
}

// ---------------------------------------------------------------------------
// Diagnose server (mcpmux_diagnose_server)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn diagnose_server_callable_but_not_in_tools_list() {
    let f = Fixture::new().await;
    let names: Vec<_> = f
        .registry
        .list_as_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !names.iter().any(|n| n == "mcpmux_diagnose_server"),
        "diagnose_server is hidden from tools/list: {names:?}"
    );
    assert!(f.registry.contains("mcpmux_diagnose_server"));
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnose_no_arg_returns_only_unhealthy_servers() {
    let f = Fixture::new().await;
    seed_diagnose_servers(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_diagnose_server",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    let servers = body.get("servers").unwrap().as_array().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(
        servers[0].get("server_id").unwrap().as_str().unwrap(),
        "firebase"
    );
    assert_eq!(servers[0].get("health").unwrap().as_str().unwrap(), "error");
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnose_explicit_server_id_returns_target_regardless_of_health() {
    let f = Fixture::new().await;
    seed_diagnose_servers(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_diagnose_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "github" }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    let servers = body.get("servers").unwrap().as_array().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(
        servers[0].get("server_id").unwrap().as_str().unwrap(),
        "github"
    );
    assert_eq!(
        servers[0].get("health").unwrap().as_str().unwrap(),
        "healthy"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnose_include_logs_false_omits_logs_block() {
    let f = Fixture::new().await;
    seed_diagnose_servers(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_diagnose_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "firebase", "include_logs": false }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    let entry = &body.get("servers").unwrap().as_array().unwrap()[0];
    assert!(entry.get("logs").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnose_surfaces_missing_required_inputs() {
    let f = Fixture::new().await;
    let space_id = f.space_id.to_string();

    let def = stdio_definition_with_required_input("needs-setup", "github_token");
    let server = InstalledServer::new(&space_id, "needs-setup").with_definition(&def);
    f.installed_server_repo.install(&server).await.unwrap();

    let result = f
        .registry
        .call(
            "mcpmux_diagnose_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "needs-setup" }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    let entry = &body.get("servers").unwrap().as_array().unwrap()[0];
    assert_eq!(
        entry.get("health").unwrap().as_str().unwrap(),
        "needs_setup"
    );
    let missing = entry
        .get("missing_required_inputs")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].as_str().unwrap(), "github_token");
}
