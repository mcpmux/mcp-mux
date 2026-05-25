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
    normalize_workspace_root, Client, DomainEvent, FeatureSet, FeatureSetMember,
    FeatureSetRepository, InboundMcpClientRepository, InstalledServer, InstalledServerRepository,
    MemberMode, MemberType, ServerFeature, ServerFeatureRepository, SpaceRepository,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::pool::FeatureService;
use mcpmux_gateway::services::{
    meta_tools, ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher,
    FeatureSetResolverService, MetaToolRegistry, PrefixCacheService, SessionOverrideRegistry,
    SessionRootsRegistry,
};
use mcpmux_storage::{
    generate_master_key, Database, FieldEncryptor, InboundClientRepository,
    SqliteFeatureSetRepository, SqliteInboundMcpClientRepository, SqliteInstalledServerRepository,
    SqliteServerFeatureRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

struct Fixture {
    registry: Arc<MetaToolRegistry>,
    broker: Arc<ApprovalBroker>,
    #[allow(dead_code)]
    client_repo: Arc<dyn InboundMcpClientRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    installed_server_repo: Arc<dyn InstalledServerRepository>,
    session_roots: Arc<SessionRootsRegistry>,
    session_overrides: Arc<SessionOverrideRegistry>,
    feature_service: Arc<FeatureService>,
    space_id: Uuid,
    /// Opaque client identity (UUID-as-string here; in production for DCR
    /// clients this can be a `client_metadata` URL).
    client_id: String,
    session_id: String,
    fs_android_id: Uuid,
    github_tool_id: Uuid,
    event_rx: broadcast::Receiver<DomainEvent>,
}

fn test_encryptor() -> Arc<FieldEncryptor> {
    let key = generate_master_key().expect("generate key");
    Arc::new(FieldEncryptor::new(&key).expect("create encryptor"))
}

impl Fixture {
    async fn new() -> Self {
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
        let session_overrides = SessionOverrideRegistry::new();
        let session_id = "sess-meta".to_string();

        let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));
        let resolver = Arc::new(FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
            inbound_client_repo.clone(),
        ));

        let prefix_cache = Arc::new(PrefixCacheService::new());
        let feature_service = Arc::new(FeatureService::new(
            server_feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache,
            session_overrides.clone(),
        ));

        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let (tx, event_rx) = broadcast::channel::<DomainEvent>(32);

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
            session_roots.clone(),
            session_overrides.clone(),
            broker.clone(),
            tx,
            None,
        );

        Self {
            registry,
            broker,
            client_repo,
            feature_set_repo,
            binding_repo,
            installed_server_repo,
            session_roots,
            session_overrides,
            feature_service,
            space_id,
            client_id,
            session_id,
            fs_android_id,
            github_tool_id,
            event_rx,
        }
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

    fn result_json(result: &rmcp::model::CallToolResult) -> Value {
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
async fn list_all_tools_returns_unfiltered_across_servers() {
    let f = Fixture::new().await;
    let result = f
        .registry
        .call(
            "mcpmux_list_all_tools",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    let tools = body.get("tools").unwrap().as_array().unwrap();
    // Both seeded tools show up regardless of FS.
    assert_eq!(tools.len(), 2);
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

fn server_status(body: &Value, server_id: &str) -> String {
    body.get("servers")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(server_id))
        .unwrap()
        .get("status")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
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

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_marks_unbound_servers_inactive() {
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
    assert_eq!(server_status(&body, "github"), "inactive");
    assert_eq!(server_status(&body, "firebase"), "inactive");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_shows_enabled_via_binding() {
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
    assert_eq!(server_status(&body, "github"), "enabled_via_binding");
    assert_eq!(server_status(&body, "firebase"), "inactive");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_servers_shows_session_override_statuses() {
    let f = Fixture::new().await;
    bind_github_only_to_session_root(&f).await;
    f.session_overrides.enable(&f.session_id, "firebase");
    f.session_overrides.disable(&f.session_id, "github");

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
    assert_eq!(server_status(&body, "github"), "disabled_via_session");
    assert_eq!(server_status(&body, "firebase"), "enabled_via_session");
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
async fn enable_server_adds_tools_on_next_list() {
    let f = Fixture::new().await;
    let result = f
        .registry
        .call(
            "mcpmux_enable_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "github" }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));

    let tools = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &[], Some(&f.session_id))
        .await
        .unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server_id, "github");
}

#[tokio::test(flavor = "multi_thread")]
async fn disable_server_removes_tools_from_list() {
    let f = Fixture::new().await;
    f.session_overrides.enable(&f.session_id, "github");

    f.registry
        .call(
            "mcpmux_disable_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "github" }),
        )
        .await
        .unwrap();

    let tools = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &[], Some(&f.session_id))
        .await
        .unwrap();
    assert!(tools.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn enable_server_workspace_persists_on_binding() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    bind_github_only_to_session_root(&f).await;

    let result = f
        .registry
        .call(
            "mcpmux_enable_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "firebase", "scope": "workspace" }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(body.get("scope").unwrap().as_str().unwrap(), "workspace");

    let root = normalize_workspace_root("/tmp/mcpmux-list-servers-test");
    let binding = f
        .binding_repo
        .find_longest_prefix_match(&f.space_id, &[root.clone()])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.feature_set_ids.len(), 2);

    let new_session = "sess-restart-sim";
    let tools = f
        .feature_service
        .get_tools_for_grants(
            &f.space_id.to_string(),
            &binding.feature_set_ids,
            Some(new_session),
        )
        .await
        .unwrap();
    let servers: std::collections::HashSet<_> =
        tools.iter().map(|t| t.server_id.as_str()).collect();
    assert!(servers.contains("github"));
    assert!(servers.contains("firebase"));
}

#[tokio::test(flavor = "multi_thread")]
async fn disable_server_workspace_removes_server_all_from_binding() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    bind_github_only_to_session_root(&f).await;

    f.registry
        .call(
            "mcpmux_enable_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "firebase", "scope": "workspace" }),
        )
        .await
        .unwrap();

    f.registry
        .call(
            "mcpmux_disable_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "firebase", "scope": "workspace" }),
        )
        .await
        .unwrap();

    let root = normalize_workspace_root("/tmp/mcpmux-list-servers-test");
    let binding = f
        .binding_repo
        .find_longest_prefix_match(&f.space_id, &[root.clone()])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.feature_set_ids.len(), 1);

    let tools = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &binding.feature_set_ids, None)
        .await
        .unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server_id, "github");
}

#[tokio::test(flavor = "multi_thread")]
async fn enable_server_workspace_requires_binding() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    f.session_roots.set_roots_capable(&f.session_id, true);
    f.session_roots
        .set(&f.session_id, ["/tmp/unbound-workspace"]);

    let result = f
        .call_tool_as_handler_would(
            "mcpmux_enable_server",
            json!({ "server_id": "github", "scope": "workspace" }),
        )
        .await;
    assert!(Fixture::is_error(&result));
}

#[tokio::test(flavor = "multi_thread")]
async fn enable_server_emits_session_override_audit_decision() {
    let mut f = Fixture::new().await;
    f.registry
        .call(
            "mcpmux_enable_server",
            &f.client_id,
            Some(&f.session_id),
            json!({ "server_id": "github" }),
        )
        .await
        .unwrap();

    let evt = tokio::time::timeout(Duration::from_millis(200), f.event_rx.recv())
        .await
        .expect("receive within 200ms")
        .expect("event");
    match evt {
        DomainEvent::MetaToolInvoked {
            tool_name,
            decision,
            ..
        } => {
            assert_eq!(tool_name, "mcpmux_enable_server");
            assert_eq!(decision, "session_override");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

// `describe_resolution` and `describe_workspace` were both removed at the
// user's request — the read surface is now just `list_all_tools` and
// `list_feature_sets`. Behavior previously asserted here is covered by
// `FeatureSetResolverService`'s own tests in
// `tests/rust/tests/integration/feature_set_resolver.rs`.

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
async fn create_feature_set_persists_members_on_approval() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);

    let result = f
        .registry
        .call(
            "mcpmux_create_feature_set",
            &f.client_id,
            Some(&f.session_id),
            json!({
                "name": "Tiny Set",
                "tool_qualified_names": ["github_create_issue"],
            }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));

    let body = Fixture::result_json(&result);
    let new_fs_id = body.get("feature_set_id").unwrap().as_str().unwrap();

    let fs = f
        .feature_set_repo
        .get_with_members(new_fs_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fs.name, "Tiny Set");
    assert_eq!(fs.members.len(), 1);
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
async fn registry_advertises_every_default_tool_with_annotations() {
    let f = Fixture::new().await;
    let tools = f.registry.list_as_tools();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    for expected in [
        "mcpmux_list_all_tools",
        "mcpmux_list_feature_sets",
        "mcpmux_list_servers",
        "mcpmux_enable_server",
        "mcpmux_disable_server",
        "mcpmux_create_feature_set",
        "mcpmux_bind_current_workspace",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }
    // Both describe_* tools were removed — they must NOT be advertised.
    for removed in ["mcpmux_describe_resolution", "mcpmux_describe_workspace"] {
        assert!(
            !names.iter().any(|n| n == removed),
            "{removed} should be removed; got {names:?}"
        );
    }
    // Writes carry the destructive_hint annotation.
    let bind = tools
        .iter()
        .find(|t| t.name == "mcpmux_bind_current_workspace")
        .unwrap();
    assert_eq!(
        bind.annotations.as_ref().and_then(|a| a.destructive_hint),
        Some(true)
    );
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
    ));
    let prefix_cache = Arc::new(PrefixCacheService::new());
    let feature_service = Arc::new(FeatureService::new(
        server_feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache,
        SessionOverrideRegistry::new(),
    ));
    let (tx, rx) = broadcast::channel::<DomainEvent>(32);
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
        SessionRootsRegistry::new(),
        SessionOverrideRegistry::new(),
        Arc::new(ApprovalBroker::new()),
        tx.clone(),
        settings_repo,
    );
    (registry, client_id, tx, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn read_tool_emits_meta_tool_invoked_with_decision_read() {
    let (registry, client_id, _tx, mut rx) = bare_registry(None).await;

    registry
        .call("mcpmux_list_all_tools", &client_id, Some("s"), json!({}))
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
            assert_eq!(tool_name, "mcpmux_list_all_tools");
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
    ));
    let prefix_cache = Arc::new(PrefixCacheService::new());
    let feature_service = Arc::new(FeatureService::new(
        server_feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache,
        SessionOverrideRegistry::new(),
    ));
    let (tx, _) = broadcast::channel::<DomainEvent>(16);
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
        SessionRootsRegistry::new(),
        SessionOverrideRegistry::new(),
        Arc::new(ApprovalBroker::new()),
        tx,
        Some(settings_repo.clone()),
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

// ============================================================================
// Session override composition (Phase 1)
// ============================================================================

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

#[tokio::test]
async fn session_override_deny_bootstrap_enables_server() {
    let f = Fixture::new().await;
    f.session_overrides.enable(&f.session_id, "github");

    let tools = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &[], Some(&f.session_id))
        .await
        .unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server_id, "github");
    assert_eq!(tools[0].feature_name, "create_issue");
}

#[tokio::test]
async fn session_override_disable_mutes_bound_server() {
    let f = Fixture::new().await;
    let fs_id = github_only_fs(&f).await;

    let before = f
        .feature_service
        .get_tools_for_grants(
            &f.space_id.to_string(),
            &[fs_id.clone()],
            Some(&f.session_id),
        )
        .await
        .unwrap();
    assert_eq!(before.len(), 1);

    f.session_overrides.disable(&f.session_id, "github");

    let after = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &[fs_id], Some(&f.session_id))
        .await
        .unwrap();
    assert!(after.is_empty());
}

#[tokio::test]
async fn session_override_additive_over_binding() {
    let f = Fixture::new().await;
    let fs_id = github_only_fs(&f).await;

    f.session_overrides.enable(&f.session_id, "firebase");

    let tools = f
        .feature_service
        .get_tools_for_grants(&f.space_id.to_string(), &[fs_id], Some(&f.session_id))
        .await
        .unwrap();

    assert_eq!(tools.len(), 2);
    let servers: std::collections::HashSet<_> =
        tools.iter().map(|t| t.server_id.as_str()).collect();
    assert!(servers.contains("github"));
    assert!(servers.contains("firebase"));
}
