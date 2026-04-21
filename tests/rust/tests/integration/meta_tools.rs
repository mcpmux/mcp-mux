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
    normalize_workspace_root, Client, DomainEvent, FeatureSet, FeatureSetRepository,
    InboundMcpClientRepository, ServerFeature, ServerFeatureRepository, SpaceRepository,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::pool::FeatureService;
use mcpmux_gateway::services::{
    meta_tools, ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher,
    FeatureSetResolverService, MetaToolRegistry, PrefixCacheService, SessionRootsRegistry,
};
use mcpmux_storage::{
    Database, SqliteFeatureSetRepository, SqliteInboundMcpClientRepository,
    SqliteServerFeatureRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

struct Fixture {
    registry: Arc<MetaToolRegistry>,
    broker: Arc<ApprovalBroker>,
    client_repo: Arc<dyn InboundMcpClientRepository>,
    space_repo: Arc<dyn SpaceRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    server_feature_repo: Arc<dyn ServerFeatureRepository>,
    session_roots: Arc<SessionRootsRegistry>,
    space_id: Uuid,
    client_id: Uuid,
    session_id: String,
    fs_android_id: Uuid,
    fs_full_id: Uuid,
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

        // Start the Space with `fs_full` as its active FS — the baseline the
        // caller resolves to before any meta-tool action.
        space_repo
            .set_active_feature_set(&space_id, Some(&fs_full_id))
            .await
            .unwrap();

        // Create test client with `pinned_space_id` set.
        let client = Client::new("TestClient", "test-type");
        let client_id = client.id;
        client_repo.create(&client).await.unwrap();
        client_repo
            .set_pin(&client_id, &space_id, None)
            .await
            .unwrap();

        let session_roots = SessionRootsRegistry::new();
        let session_id = "sess-meta".to_string();

        let resolver = Arc::new(FeatureSetResolverService::new(
            client_repo.clone(),
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
        ));

        let prefix_cache = Arc::new(PrefixCacheService::new());
        let feature_service = Arc::new(FeatureService::new(
            server_feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache,
        ));

        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let (tx, _rx) = broadcast::channel::<DomainEvent>(32);

        let registry = meta_tools::build_default_registry(
            client_repo.clone(),
            space_repo.clone(),
            feature_set_repo.clone(),
            binding_repo.clone(),
            server_feature_repo.clone(),
            resolver,
            feature_service,
            session_roots.clone(),
            broker.clone(),
            tx,
            None,
        );

        Self {
            registry,
            broker,
            client_repo,
            space_repo,
            feature_set_repo,
            binding_repo,
            server_feature_repo,
            session_roots,
            space_id,
            client_id,
            session_id,
            fs_android_id,
            fs_full_id,
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
                        Uuid::parse_str(&req.client_id).unwrap(),
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
async fn list_feature_sets_marks_active_fs() {
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
    let active: Vec<_> = sets
        .iter()
        .filter(|fs| {
            fs.get("is_active")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(active.len(), 1, "exactly one Active FS expected");
    assert_eq!(
        active[0].get("id").unwrap().as_str().unwrap(),
        f.fs_full_id.to_string()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn describe_resolution_reports_space_active_baseline() {
    let f = Fixture::new().await;
    let result = f
        .registry
        .call(
            "mcpmux_describe_resolution",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("source").unwrap().as_str().unwrap(),
        "space_active"
    );
    assert_eq!(
        body.get("feature_set_id").unwrap().as_str().unwrap(),
        f.fs_full_id.to_string()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn describe_workspace_reports_reported_roots() {
    let f = Fixture::new().await;
    let path = if cfg!(windows) {
        "d:\\android\\myapp"
    } else {
        "/android/myapp"
    };
    f.session_roots.set(&f.session_id, [path]);

    let result = f
        .registry
        .call(
            "mcpmux_describe_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({}),
        )
        .await
        .unwrap();
    let body = Fixture::result_json(&result);
    let roots = body.get("reported_roots").unwrap().as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert!(body.get("matched_binding").unwrap().is_null());
}

// ---------------------------------------------------------------------------
// Writes — gated by ApprovalBroker
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn write_without_publisher_returns_approval_required() {
    let f = Fixture::new().await;
    let result = f
        .call_tool_as_handler_would(
            "mcpmux_pin_this_session",
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
async fn pin_this_session_writes_state_on_allow() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);

    let result = f
        .registry
        .call(
            "mcpmux_pin_this_session",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));

    let client = f.client_repo.get(&f.client_id).await.unwrap().unwrap();
    assert_eq!(client.pinned_feature_set_id, Some(f.fs_android_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn pin_this_session_rejected_on_deny_leaves_state_unchanged() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::Deny);

    let result = f
        .call_tool_as_handler_would(
            "mcpmux_pin_this_session",
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await;
    assert!(Fixture::is_error(&result));
    let body = Fixture::result_json(&result);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "approval_denied"
    );

    let client = f.client_repo.get(&f.client_id).await.unwrap().unwrap();
    assert_eq!(client.pinned_feature_set_id, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn always_allow_bypasses_subsequent_dialogs() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AlwaysForThisSessionAndClient);

    // First call pops the dialog and banks the always-allow.
    let r1 = f
        .registry
        .call(
            "mcpmux_pin_this_session",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&r1));

    // Detach publisher — any further prompt would fail. Second call must
    // short-circuit via always-allow.
    let noop_publisher: ApprovalPublisher = Arc::new(move |_req| async move { true }.boxed());
    f.broker.set_publisher(noop_publisher).await;

    let r2 = f
        .registry
        .call(
            "mcpmux_pin_this_session",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_full_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&r2));

    let client = f.client_repo.get(&f.client_id).await.unwrap().unwrap();
    assert_eq!(client.pinned_feature_set_id, Some(f.fs_full_id));
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
}

#[tokio::test(flavor = "multi_thread")]
async fn set_space_active_updates_space_fallback() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);

    let result = f
        .registry
        .call(
            "mcpmux_set_space_active",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();
    assert!(!Fixture::is_error(&result));

    let space = f.space_repo.get(&f.space_id).await.unwrap().unwrap();
    assert_eq!(space.active_feature_set_id, Some(f.fs_android_id));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_feature_set_argument_rejected() {
    let f = Fixture::new().await;
    let result = f
        .call_tool_as_handler_would(
            "mcpmux_pin_this_session",
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
        "mcpmux_describe_resolution",
        "mcpmux_describe_workspace",
        "mcpmux_pin_this_session",
        "mcpmux_create_feature_set",
        "mcpmux_bind_current_workspace",
        "mcpmux_set_space_active",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }
    // Writes carry the destructive_hint annotation.
    let pin = tools
        .iter()
        .find(|t| t.name == "mcpmux_pin_this_session")
        .unwrap();
    assert_eq!(
        pin.annotations.as_ref().and_then(|a| a.destructive_hint),
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
    Uuid,
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

    let space = space_repo.get_default().await.unwrap().unwrap();
    let client = Client::new("c", "t");
    let client_id = client.id;
    client_repo.create(&client).await.unwrap();
    client_repo
        .set_pin(&client_id, &space.id, None)
        .await
        .unwrap();

    let resolver = Arc::new(FeatureSetResolverService::new(
        client_repo.clone(),
        space_repo.clone(),
        binding_repo.clone(),
        SessionRootsRegistry::new(),
    ));
    let prefix_cache = Arc::new(PrefixCacheService::new());
    let feature_service = Arc::new(FeatureService::new(
        server_feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache,
    ));
    let (tx, rx) = broadcast::channel::<DomainEvent>(32);
    let registry = meta_tools::build_default_registry(
        client_repo,
        space_repo,
        feature_set_repo,
        binding_repo,
        server_feature_repo,
        resolver,
        feature_service,
        SessionRootsRegistry::new(),
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
        .call(
            "mcpmux_describe_resolution",
            &client_id,
            Some("s"),
            json!({}),
        )
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
            assert_eq!(tool_name, "mcpmux_describe_resolution");
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
            "mcpmux_pin_this_session",
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
            assert_eq!(tool_name, "mcpmux_pin_this_session");
            assert_eq!(decision, "approval_required");
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
    let resolver = Arc::new(FeatureSetResolverService::new(
        client_repo.clone(),
        space_repo.clone(),
        binding_repo.clone(),
        SessionRootsRegistry::new(),
    ));
    let prefix_cache = Arc::new(PrefixCacheService::new());
    let feature_service = Arc::new(FeatureService::new(
        server_feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache,
    ));
    let (tx, _) = broadcast::channel::<DomainEvent>(16);
    let registry = meta_tools::build_default_registry(
        client_repo,
        space_repo,
        feature_set_repo,
        binding_repo,
        server_feature_repo,
        resolver,
        feature_service,
        SessionRootsRegistry::new(),
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
