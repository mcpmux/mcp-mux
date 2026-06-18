//! Integration tests for the workspace-binding domain event flow.
//!
//! These tests exercise the parts the gateway relies on at the domain layer
//! — the full `on_initialized` → `list_roots` → resolver → event emission
//! path in `handler.rs` needs a live rmcp peer to drive and is covered by the
//! desktop E2E suite. What we can reach here is:
//!
//! 1. `WorkspaceBindingChanged` + `WorkspaceNeedsBinding` round-trip through
//!    JSON with the shape the Tauri bridge and the frontend consumers expect.
//! 2. The resolver's decision table: roots + no binding → `source =
//!    SpaceDefault` (the folder falls back to the default Starter FS, and the
//!    same condition still triggers the `WorkspaceNeedsBinding` prompt).
//! 3. Creating / updating a binding flips the next resolution from
//!    SpaceDefault to WorkspaceBinding — the behaviour that justifies firing
//!    list_changed.

use std::sync::Arc;

use mcpmux_core::{
    normalize_workspace_root, DomainEvent, FeatureSet, FeatureSetRepository, SpaceRepository,
    WorkspaceBinding, WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, ResolutionSource, SessionRootsRegistry};
use mcpmux_storage::{
    Database, InboundClientRepository, SqliteFeatureSetRepository, SqliteSpaceBaseDirRepository,
    SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Ctx {
    resolver: FeatureSetResolverService,
    session_roots: Arc<SessionRootsRegistry>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    space_id: Uuid,
    fs_custom_id: String,
}

impl Ctx {
    async fn new() -> Self {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
        let fs_repo: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));
        let binding_repo: Arc<dyn WorkspaceBindingRepository> =
            Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
        let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        let custom = FeatureSet::new_custom("Custom", space_id.to_string());
        fs_repo.create(&custom).await.unwrap();

        let session_roots = SessionRootsRegistry::new();
        let resolver = FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
            inbound_client_repo.clone(),
            fs_repo.clone(),
            Arc::new(SqliteSpaceBaseDirRepository::new(db.clone())),
        );

        Self {
            resolver,
            session_roots,
            binding_repo,
            space_id,
            fs_custom_id: custom.id,
        }
    }
}

/// After creating a binding for the root the next resolve flips from
/// `SpaceDefault` (roots reported, nothing bound — the unmapped folder falls
/// back to the default Starter FS, and `handler.rs::log_and_notify_resolution`
/// still turns this into a `WorkspaceNeedsBinding` prompt) to
/// `WorkspaceBinding`. In production the flip is what triggers the
/// `WorkspaceBindingChanged` → `list_changed` broadcast. (The standalone
/// "unbound → SpaceDefault" case is covered by the resolver decision-table in
/// `feature_set_resolver.rs`.)
#[tokio::test(flavor = "multi_thread")]
async fn creating_binding_flips_next_resolution_source() {
    let ctx = Ctx::new().await;

    // Normalize both sides so the exact-match lookup matches — the resolver
    // compares already-normalized strings from both stores (no inheritance).
    let raw = if cfg!(windows) {
        "d:\\proj\\bind-me"
    } else {
        "/proj/bind-me"
    };
    let root = normalize_workspace_root(raw);
    ctx.session_roots.set("sess-1", [raw]);
    ctx.session_roots.set_roots_capable("sess-1", true);

    let before = ctx.resolver.resolve(Some("sess-1"), None).await.unwrap();
    assert_eq!(before.source, ResolutionSource::SpaceDefault);
    // Unmapped folder gets a non-empty fallback FS (the Starter), so the
    // fingerprint is `Some(..)` and flips to the bound FS below — that
    // change is exactly what fires the per-peer `list_changed`.
    assert!(!before.feature_set_ids.is_empty());

    let binding = WorkspaceBinding::new(root, ctx.space_id, ctx.fs_custom_id.clone());
    ctx.binding_repo.create(&binding).await.unwrap();

    let after = ctx.resolver.resolve(Some("sess-1"), None).await.unwrap();
    assert_eq!(after.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(after.feature_set_ids, vec![ctx.fs_custom_id.clone()]);
}

/// Rootless session without client grants resolves to `SpaceDefault` (the
/// default Starter FS) — but *silently*: no `WorkspaceNeedsBinding` is
/// appropriate here (rootless = nothing to bind). The handler enforces that
/// silence by passing `root_for_prompt = None` for rootless sessions; this
/// test pins the resolver half — a rootless session never lands on a
/// folder-bearing source.
#[tokio::test(flavor = "multi_thread")]
async fn rootless_session_without_grants_defaults_silently() {
    let ctx = Ctx::new().await;
    // Deliberately no roots set; capability stamped as false (rootless).
    ctx.session_roots.set_roots_capable("rootless", false);
    let resolved = ctx
        .resolver
        .resolve(Some("rootless"), Some("unknown-client"))
        .await
        .unwrap();
    assert_eq!(resolved.source, ResolutionSource::SpaceDefault);
}

/// Binding → different Space should actually route the session to that
/// Space, regardless of which Space the caller was "in" before. Pins the
/// contract that bindings carry concrete pointers (not "follow active").
#[tokio::test(flavor = "multi_thread")]
async fn binding_to_non_default_space_reroutes_session() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
    let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
    let fs_repo: Arc<dyn FeatureSetRepository> =
        Arc::new(SqliteFeatureSetRepository::new(db.clone()));
    let binding_repo: Arc<dyn WorkspaceBindingRepository> =
        Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
    let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));

    let default_space = space_repo.get_default().await.unwrap().unwrap();

    // A second Space, with its own Custom FS. The binding below will route
    // the reported root here even though the default Space is still the
    // "default".
    let other = mcpmux_core::Space::new("Other");
    let other_id = other.id;
    space_repo.create(&other).await.unwrap();
    let other_fs = FeatureSet::new_custom("Other Custom", other_id.to_string());
    fs_repo.create(&other_fs).await.unwrap();

    let session_roots = SessionRootsRegistry::new();
    let resolver = FeatureSetResolverService::new(
        space_repo.clone(),
        binding_repo.clone(),
        session_roots.clone(),
        inbound_client_repo.clone(),
        fs_repo.clone(),
        Arc::new(SqliteSpaceBaseDirRepository::new(db.clone())),
    );

    let raw = if cfg!(windows) {
        "d:\\other\\work"
    } else {
        "/other/work"
    };
    let root = normalize_workspace_root(raw);
    session_roots.set("sess-X", [raw]);
    session_roots.set_roots_capable("sess-X", true);

    // Before binding: roots reported, no binding → SpaceDefault scoped to the
    // default space (the resolver still reports a space_id so the upstream
    // prompt knows where to scope the binding sheet).
    let before = resolver.resolve(Some("sess-X"), None).await.unwrap();
    assert_eq!(before.source, ResolutionSource::SpaceDefault);
    assert_eq!(before.space_id, Some(default_space.id));

    // Create a binding targeting `other` space's Custom FS.
    let b = WorkspaceBinding::new(root, other_id, other_fs.id.clone());
    binding_repo.create(&b).await.unwrap();

    let after = resolver.resolve(Some("sess-X"), None).await.unwrap();
    assert_eq!(after.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(after.space_id, Some(other_id));
    assert_eq!(after.feature_set_ids, vec![other_fs.id]);
}

/// Minimal "is the Tauri bridge payload the shape the webview expects?"
/// sanity check. If the serde tag or field names change, both the
/// `workspace-needs-binding` Tauri channel consumer and the
/// `WorkspaceBindingSheet` component's TypeScript payload type break.
#[test]
fn event_json_payloads_are_stable() {
    let changed = DomainEvent::WorkspaceBindingChanged {
        space_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        workspace_root: "/abs/path".to_string(),
    };
    let v: serde_json::Value = serde_json::to_value(&changed).unwrap();
    assert_eq!(v["type"], "workspace_binding_changed");
    assert_eq!(v["workspace_root"], "/abs/path");
    assert_eq!(v["space_id"], "00000000-0000-0000-0000-000000000001");

    let needs = DomainEvent::WorkspaceNeedsBinding {
        client_id: "vscode".to_string(),
        session_id: "s-9".to_string(),
        space_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        workspace_root: "/abs/path".to_string(),
    };
    let v: serde_json::Value = serde_json::to_value(&needs).unwrap();
    assert_eq!(v["type"], "workspace_needs_binding");
    assert_eq!(v["client_id"], "vscode");
    assert_eq!(v["session_id"], "s-9");
    assert_eq!(v["workspace_root"], "/abs/path");
}
