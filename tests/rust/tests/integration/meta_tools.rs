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
use mcpmux_core::{SpaceBuiltinConfigRepository, TOOL_OPTIMIZATION_SERVER_ID};
use mcpmux_gateway::pool::FeatureService;
use mcpmux_gateway::services::{
    meta_tools, ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher,
    FeatureSetResolverService, MetaToolRegistry, PrefixCacheService, SessionRootsRegistry,
};
use mcpmux_storage::{
    Database, InboundClientRepository, SqliteFeatureSetRepository,
    SqliteInboundMcpClientRepository, SqliteServerFeatureRepository,
    SqliteSpaceBuiltinConfigRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
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
    session_roots: Arc<SessionRootsRegistry>,
    /// Domain-event sender the registry writes to; tests subscribe to assert
    /// the events the desktop UI / MCPNotifier react to are actually emitted.
    event_tx: broadcast::Sender<DomainEvent>,
    space_id: Uuid,
    /// Opaque client identity (UUID-as-string here; in production for DCR
    /// clients this can be a `client_metadata` URL).
    client_id: String,
    session_id: String,
    fs_android_id: Uuid,
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
        ));

        let prefix_cache = Arc::new(PrefixCacheService::new());
        let feature_service = Arc::new(FeatureService::new(
            server_feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache,
        ));

        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let (tx, _rx) = broadcast::channel::<DomainEvent>(32);
        let event_tx = tx.clone();

        let builtin_config_repo: Arc<dyn SpaceBuiltinConfigRepository> =
            Arc::new(SqliteSpaceBuiltinConfigRepository::new(db.clone()));

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
            Some(builtin_config_repo),
        );

        Self {
            registry,
            broker,
            client_repo,
            feature_set_repo,
            binding_repo,
            session_roots,
            event_tx,
            space_id,
            client_id,
            session_id,
            fs_android_id,
        }
    }

    /// Subscribe to the registry's domain-event stream. Subscribe BEFORE the
    /// call under test — broadcast only delivers messages sent after subscribe.
    fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_tx.subscribe()
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
async fn manage_feature_set_create_persists_members_on_approval() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);

    let result = f
        .registry
        .call(
            "mcpmux_manage_feature_set",
            &f.client_id,
            Some(&f.session_id),
            json!({
                "action": "create",
                "name": "Tiny Set",
                "add": ["github_create_issue"],
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

/// create → update (add + remove + rename) → delete, all on approval.
#[tokio::test(flavor = "multi_thread")]
async fn manage_feature_set_update_and_delete() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);

    // create with one tool
    let created = Fixture::result_json(
        &f.registry
            .call(
                "mcpmux_manage_feature_set",
                &f.client_id,
                Some(&f.session_id),
                json!({ "action": "create", "name": "Set A", "add": ["github_create_issue"] }),
            )
            .await
            .unwrap(),
    );
    let fs_id = created
        .get("feature_set_id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    // update: add the firebase tool, remove the github tool, rename
    let res = f
        .call_tool_as_handler_would(
            "mcpmux_manage_feature_set",
            json!({
                "action": "update",
                "feature_set_id": fs_id,
                "name": "Set B",
                "add": ["firebase_deploy"],
                "remove": ["github_create_issue"],
            }),
        )
        .await;
    assert!(!Fixture::is_error(&res), "update should succeed: {res:?}");
    let fs = f
        .feature_set_repo
        .get_with_members(&fs_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fs.name, "Set B", "renamed");
    let member_ids: Vec<String> = fs.members.iter().map(|m| m.member_id.clone()).collect();
    assert_eq!(member_ids.len(), 1, "github removed, firebase added");

    // delete
    let res = f
        .call_tool_as_handler_would(
            "mcpmux_manage_feature_set",
            json!({ "action": "delete", "feature_set_id": fs_id }),
        )
        .await;
    assert!(!Fixture::is_error(&res), "delete should succeed: {res:?}");
    let after = f.feature_set_repo.get(&fs_id).await.unwrap();
    assert!(
        after.map(|fs| fs.is_deleted).unwrap_or(true),
        "FS should be soft-deleted"
    );
}

/// Built-in (Starter) FeatureSets are not mutable via MCP.
#[tokio::test(flavor = "multi_thread")]
async fn manage_feature_set_rejects_builtin() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);

    // Ensure the auto-seeded Starter exists, then try to delete it.
    f.feature_set_repo
        .ensure_builtin_for_space(&f.space_id.to_string())
        .await
        .unwrap();
    let starter = f
        .feature_set_repo
        .get_starter_for_space(&f.space_id.to_string())
        .await
        .unwrap()
        .expect("starter exists");

    let res = f
        .call_tool_as_handler_would(
            "mcpmux_manage_feature_set",
            json!({ "action": "delete", "feature_set_id": starter.id }),
        )
        .await;
    assert!(Fixture::is_error(&res));
    let body = Fixture::result_json(&res);
    assert_eq!(
        body.get("error").unwrap().as_str().unwrap(),
        "invalid_argument"
    );
}

/// Unknown action is rejected with an actionable error.
#[tokio::test(flavor = "multi_thread")]
async fn manage_feature_set_unknown_action_rejected() {
    let f = Fixture::new().await;
    let res = f
        .call_tool_as_handler_would(
            "mcpmux_manage_feature_set",
            json!({ "action": "frobnicate" }),
        )
        .await;
    assert!(Fixture::is_error(&res));
    assert_eq!(
        Fixture::result_json(&res)
            .get("error")
            .unwrap()
            .as_str()
            .unwrap(),
        "invalid_argument"
    );
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

/// A successful bind must emit `WorkspaceBindingChanged` (not a generic
/// FeatureSet-members event) — that's the event the desktop Workspaces tab
/// refreshes on, and the one MCPNotifier turns into a list_changed push.
/// Regression guard for "workspace mapping didn't refresh in the UI".
#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_emits_workspace_binding_changed() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let mut rx = f.subscribe();

    let input = if cfg!(windows) {
        "D:\\Projects\\Notify\\"
    } else {
        "/proj/notify"
    };
    f.session_roots.set(&f.session_id, [input]);

    f.registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();

    // The stream also carries the central MetaToolInvoked audit event, so scan
    // for the binding-changed signal specifically rather than asserting on the
    // first event received.
    let expected_root = normalize_workspace_root(input);
    let mut found = false;
    for _ in 0..8 {
        match tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
            Ok(Ok(DomainEvent::WorkspaceBindingChanged {
                space_id,
                workspace_root,
            })) => {
                assert_eq!(space_id, f.space_id);
                assert_eq!(workspace_root, expected_root);
                found = true;
                break;
            }
            Ok(Ok(_other)) => continue,   // e.g. MetaToolInvoked — skip
            Ok(Err(_)) | Err(_) => break, // channel closed/lagged or timed out
        }
    }
    assert!(
        found,
        "bind must emit WorkspaceBindingChanged so the Workspaces UI refreshes"
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

/// Binding the same workspace twice REBINDS (upsert) instead of erroring —
/// no separate unbind needed.
#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_rebinds_on_second_call() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\Rebind"
    } else {
        "/proj/rebind"
    };
    f.session_roots.set(&f.session_id, [input]);

    // Make a second FS to rebind to.
    let other = FeatureSet::new_custom("Other", f.space_id.to_string());
    f.feature_set_repo.create(&other).await.unwrap();

    // First bind → fs_android.
    f.registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": f.fs_android_id.to_string() }),
        )
        .await
        .unwrap();
    // Rebind same root → other FS.
    f.registry
        .call(
            "mcpmux_bind_current_workspace",
            &f.client_id,
            Some(&f.session_id),
            json!({ "feature_set_id": other.id }),
        )
        .await
        .unwrap();

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1, "still one binding for the root (upsert)");
    assert_eq!(bindings[0].feature_set_ids, vec![other.id]);
}

/// Omitting `feature_set_id` binds the workspace to NO Space tools (a valid
/// empty mapping), without erroring.
#[tokio::test(flavor = "multi_thread")]
async fn bind_current_workspace_to_empty_is_allowed() {
    let f = Fixture::new().await;
    f.attach_auto_publisher(ApprovalDecision::AllowOnce);
    let input = if cfg!(windows) {
        "D:\\Projects\\Empty"
    } else {
        "/proj/empty"
    };
    f.session_roots.set(&f.session_id, [input]);

    let res = f
        .call_tool_as_handler_would("mcpmux_bind_current_workspace", json!({}))
        .await;
    assert!(!Fixture::is_error(&res), "empty bind allowed: {res:?}");

    let bindings = f.binding_repo.list_for_space(&f.space_id).await.unwrap();
    assert_eq!(bindings.len(), 1);
    assert!(
        bindings[0].feature_set_ids.is_empty(),
        "empty FeatureSet list = no Space tools"
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
        "mcpmux_manage_feature_set",
        "mcpmux_bind_current_workspace",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }
    // The old single-purpose create tool was consolidated into manage.
    assert!(
        !names.iter().any(|n| n == "mcpmux_create_feature_set"),
        "create_feature_set should be gone (folded into manage): {names:?}"
    );
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
        None,
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
async fn per_space_config_controls_registry_visibility() {
    // Same DB so the per-Space config repo and the registry see one another.
    let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
    let builtin_config_repo: Arc<dyn SpaceBuiltinConfigRepository> =
        Arc::new(SqliteSpaceBuiltinConfigRepository::new(db.clone()));

    let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
    let feature_set_repo: Arc<dyn FeatureSetRepository> =
        Arc::new(SqliteFeatureSetRepository::new(db.clone()));
    let client_repo: Arc<dyn InboundMcpClientRepository> =
        Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));
    let binding_repo: Arc<dyn WorkspaceBindingRepository> =
        Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
    let server_feature_repo: Arc<dyn ServerFeatureRepository> =
        Arc::new(SqliteServerFeatureRepository::new(db.clone()));
    let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));

    let space_id = space_repo.get_default().await.unwrap().unwrap().id;

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
        None,
        Some(builtin_config_repo.clone()),
    );

    let sid = space_id.to_string();

    // Default: the Tool Optimization server is enabled for the Space.
    assert!(
        registry.is_server_enabled_for_space(&space_id).await,
        "enabled by default"
    );
    assert!(
        !registry.list_as_tools_for_space(&space_id).await.is_empty(),
        "tools advertised by default"
    );

    // Disable the whole server for this Space → no tools advertised.
    builtin_config_repo
        .set_server_enabled(&sid, TOOL_OPTIMIZATION_SERVER_ID, false)
        .await
        .unwrap();
    assert!(!registry.is_server_enabled_for_space(&space_id).await);
    assert!(
        registry.list_as_tools_for_space(&space_id).await.is_empty(),
        "no tools when the server is disabled for the Space"
    );

    // Re-enable, then disable a single tool → that tool drops, others remain.
    builtin_config_repo
        .set_server_enabled(&sid, TOOL_OPTIMIZATION_SERVER_ID, true)
        .await
        .unwrap();
    builtin_config_repo
        .set_tool_enabled(
            &sid,
            TOOL_OPTIMIZATION_SERVER_ID,
            "mcpmux_list_all_tools",
            false,
        )
        .await
        .unwrap();
    let names: Vec<String> = registry
        .list_as_tools_for_space(&space_id)
        .await
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !names.iter().any(|n| n == "mcpmux_list_all_tools"),
        "the disabled tool is hidden: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "mcpmux_list_feature_sets"),
        "other tools remain: {names:?}"
    );
    assert!(
        !registry
            .is_tool_enabled_for_space(&space_id, "mcpmux_list_all_tools")
            .await
    );
    assert!(
        registry
            .is_tool_enabled_for_space(&space_id, "mcpmux_list_feature_sets")
            .await
    );
}

// Silence unused-import warnings from helper imports that only some tests exercise.
#[allow(dead_code)]
fn _unused(_: ApprovalPayload) {}
