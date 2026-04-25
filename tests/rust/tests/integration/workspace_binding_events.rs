//! Integration tests for the workspace-binding domain event flow.
//!
//! These tests exercise the parts the gateway relies on at the domain layer
//! — the full `on_initialized` → `list_roots` → resolver → event emission
//! path in `handler.rs` needs a live rmcp peer to drive and is covered by the
//! desktop E2E suite. What we can reach here is:
//!
//! 1. `WorkspaceBindingChanged` + `WorkspaceNeedsBinding` round-trip through
//!    JSON with the shape the Tauri bridge and the frontend consumers expect.
//! 2. The resolver's decision table: roots + no binding → `source = Default`
//!    (the trigger the gateway uses to decide whether to emit the event).
//! 3. Creating / updating a binding flips the next resolution from Default to
//!    WorkspaceBinding — the behaviour that justifies firing list_changed.

use std::sync::Arc;

use mcpmux_core::{
    normalize_workspace_root, DomainEvent, FeatureSet, FeatureSetRepository, SpaceRepository,
    WorkspaceBinding, WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, ResolutionSource, SessionRootsRegistry};
use mcpmux_storage::{
    Database, SqliteFeatureSetRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
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

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        let custom = FeatureSet::new_custom("Custom", space_id.to_string());
        fs_repo.create(&custom).await.unwrap();

        let session_roots = SessionRootsRegistry::new();
        let resolver = FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            fs_repo.clone(),
            session_roots.clone(),
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

/// Session with roots, no binding → resolver returns `source = Default`.
/// This is the exact condition `handler.rs::log_and_notify_resolution`
/// turns into a `WorkspaceNeedsBinding` emission.
#[tokio::test(flavor = "multi_thread")]
async fn session_with_unbound_root_resolves_via_default() {
    let ctx = Ctx::new().await;
    ctx.session_roots.set("sess-1", ["/proj/unbound"]);

    let resolved = ctx.resolver.resolve(Some("sess-1")).await.unwrap();
    assert_eq!(resolved.source, ResolutionSource::Default);
    assert_eq!(resolved.space_id, Some(ctx.space_id));
    // Default always hands back the Space's "All" FS so the client gets a
    // working toolset even before the user binds the folder.
    assert!(resolved.feature_set_id.is_some());
}

/// After creating a binding for the root the next resolve flips to
/// `source = WorkspaceBinding`. In production that's what triggers the
/// `WorkspaceBindingChanged` → `list_changed` broadcast.
#[tokio::test(flavor = "multi_thread")]
async fn creating_binding_flips_next_resolution_source() {
    let ctx = Ctx::new().await;

    // Normalize both sides so the longest-prefix lookup matches — the
    // resolver compares already-normalized strings from both stores.
    let raw = if cfg!(windows) {
        "d:\\proj\\bind-me"
    } else {
        "/proj/bind-me"
    };
    let root = normalize_workspace_root(raw);
    ctx.session_roots.set("sess-1", [raw]);

    let before = ctx.resolver.resolve(Some("sess-1")).await.unwrap();
    assert_eq!(before.source, ResolutionSource::Default);

    let binding = WorkspaceBinding::new(root, ctx.space_id, ctx.fs_custom_id.clone());
    ctx.binding_repo.create(&binding).await.unwrap();

    let after = ctx.resolver.resolve(Some("sess-1")).await.unwrap();
    assert_eq!(after.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(after.feature_set_id, Some(ctx.fs_custom_id.clone()));
}

/// Rootless session never resolves via a binding — stays at Default and
/// should never produce a `WorkspaceNeedsBinding` event. This test pins the
/// rootless-silence contract; if it ever fails, the notifier would start
/// prompting users with no folder context.
#[tokio::test(flavor = "multi_thread")]
async fn rootless_session_stays_default_no_prompt() {
    let ctx = Ctx::new().await;
    // Deliberately no call to session_roots.set — simulates a rootless
    // (CLI-ish) client.
    let resolved = ctx.resolver.resolve(Some("rootless")).await.unwrap();
    assert_eq!(resolved.source, ResolutionSource::Default);
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
        fs_repo.clone(),
        session_roots.clone(),
    );

    let raw = if cfg!(windows) {
        "d:\\other\\work"
    } else {
        "/other/work"
    };
    let root = normalize_workspace_root(raw);
    session_roots.set("sess-X", [raw]);

    // Before binding: Default tier — lands in the *default* space with its
    // Default FS.
    let before = resolver.resolve(Some("sess-X")).await.unwrap();
    assert_eq!(before.source, ResolutionSource::Default);
    assert_eq!(before.space_id, Some(default_space.id));

    // Create a binding targeting `other` space's Custom FS.
    let b = WorkspaceBinding::new(root, other_id, other_fs.id.clone());
    binding_repo.create(&b).await.unwrap();

    let after = resolver.resolve(Some("sess-X")).await.unwrap();
    assert_eq!(after.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(after.space_id, Some(other_id));
    assert_eq!(after.feature_set_id, Some(other_fs.id));
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
