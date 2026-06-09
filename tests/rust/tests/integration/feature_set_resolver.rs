//! Decision-table tests for the FeatureSet resolver (capability-branched v3).
//!
//! Outcomes:
//!   1. **WorkspaceBinding** — session reported roots AND a binding matched
//!      one of them. `space_id` + `feature_set_ids[0]` come from the binding.
//!   2. **PendingRoots** — session declared MCP `roots` capability but the
//!      list hasn't arrived yet. Empty FS list; resolver fires
//!      `list_changed` later when roots populate.
//!   3. **ClientGrant** — rootless-by-design client. Per-client grants
//!      from the `client_grants` table apply.
//!   4. **Deny** — every other case (roots reported but no binding; no
//!      session id and no grants; etc.). Empty FS list.

use std::sync::Arc;

use mcpmux_core::{
    normalize_workspace_root, FeatureSet, FeatureSetRepository, SpaceRepository, WorkspaceBinding,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, ResolutionSource, SessionRootsRegistry};
use mcpmux_storage::{
    Database, InboundClient, InboundClientRepository, RegistrationType, SqliteFeatureSetRepository,
    SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Fixture {
    resolver: FeatureSetResolverService,
    session_roots: Arc<SessionRootsRegistry>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    client_repo: Arc<InboundClientRepository>,
    space_id: Uuid,
    fs_a_id: String,
    fs_b_id: String,
}

impl Fixture {
    async fn new() -> Self {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
        let fs_repo: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));
        let binding_repo: Arc<dyn WorkspaceBindingRepository> =
            Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
        let client_repo = Arc::new(InboundClientRepository::new(db.clone()));

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        let a = FeatureSet::new_custom("A", space_id.to_string());
        let b = FeatureSet::new_custom("B", space_id.to_string());
        fs_repo.create(&a).await.unwrap();
        fs_repo.create(&b).await.unwrap();
        let fs_a_id = a.id.clone();
        let fs_b_id = b.id.clone();

        let session_roots = SessionRootsRegistry::new();
        let resolver = FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
            client_repo.clone(),
        );

        Self {
            resolver,
            session_roots,
            binding_repo,
            client_repo,
            space_id,
            fs_a_id,
            fs_b_id,
        }
    }

    /// Insert an inbound client row so we can attach grants to it (the
    /// `client_grants` FK requires the row to exist).
    async fn make_client(&self, client_id: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        let c = InboundClient {
            client_id: client_id.to_string(),
            registration_type: RegistrationType::Dcr,
            client_name: "test-client".to_string(),
            client_alias: None,
            redirect_uris: vec!["http://localhost/cb".to_string()],
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
        };
        self.client_repo.save_client(&c).await.unwrap();
    }
}

fn test_root() -> &'static str {
    if cfg!(windows) {
        "d:\\work\\proj"
    } else {
        "/work/proj"
    }
}

// ---------------------------------------------------------------------------
// Deny tier
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deny_when_no_session_id_and_no_grants() {
    let f = Fixture::new().await;
    let r = f.resolver.resolve(None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Deny);
    assert!(r.feature_set_ids.is_empty());
    assert_eq!(r.space_id, Some(f.space_id));
}

#[tokio::test]
async fn pending_when_session_has_no_roots_and_capability_unknown() {
    // Default capability state for a session we've never seen
    // `notifications/initialized` for is `None` (unknown). The resolver
    // treats unknown like roots-capable: returns `PendingRoots` so the
    // *next* request retries via the on-demand probe instead of being
    // permanently denied. This was the bug where a tools/list racing
    // on_initialized resolved to "no roots + no grants — deny" and the
    // user saw only meta tools until reconnect.
    let f = Fixture::new().await;
    let r = f.resolver.resolve(Some("orphan"), None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::PendingRoots);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn deny_when_session_explicitly_rootless_and_no_grants() {
    // Explicit Some(false) capability — client told us it doesn't
    // support roots — and no client grants. This is the only path where
    // the resolver legitimately lands on Deny without a session id.
    let f = Fixture::new().await;
    f.session_roots.set_roots_capable("rootless", false);
    let r = f.resolver.resolve(Some("rootless"), None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Deny);
}

#[tokio::test]
async fn deny_when_roots_reported_but_no_binding_matches() {
    let f = Fixture::new().await;
    let other = if cfg!(windows) { "d:\\tmp" } else { "/tmp" };
    f.session_roots.set("sess", [other]);
    let r = f.resolver.resolve(Some("sess"), None).await.unwrap();
    // Roots present but no binding → upstream emits WorkspaceNeedsBinding;
    // resolver itself reports Deny (no FS to apply).
    assert_eq!(r.source, ResolutionSource::Deny);
    assert!(r.feature_set_ids.is_empty());
}

// ---------------------------------------------------------------------------
// PendingRoots tier
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pending_when_capable_but_roots_havent_arrived() {
    let f = Fixture::new().await;
    f.session_roots.set_roots_capable("sess", true);
    // No roots set in the registry yet.
    let r = f.resolver.resolve(Some("sess"), None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::PendingRoots);
    assert!(r.feature_set_ids.is_empty());
}

// ---------------------------------------------------------------------------
// WorkspaceBinding tier
// ---------------------------------------------------------------------------

#[tokio::test]
async fn binding_routes_to_its_target_space_and_fs() {
    let f = Fixture::new().await;
    let binding = WorkspaceBinding::new(
        normalize_workspace_root(test_root()),
        f.space_id,
        f.fs_a_id.clone(),
    );
    f.binding_repo.create(&binding).await.unwrap();
    f.session_roots.set("s", [test_root()]);
    f.session_roots.set_roots_capable("s", true);

    let r = f.resolver.resolve(Some("s"), None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.space_id, Some(f.space_id));
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn longest_prefix_wins_across_nested_bindings() {
    let f = Fixture::new().await;
    let (outer, inner) = if cfg!(windows) {
        ("d:\\work", "d:\\work\\proj")
    } else {
        ("/work", "/work/proj")
    };
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(outer),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(inner),
            f.space_id,
            f.fs_b_id.clone(),
        ))
        .await
        .unwrap();

    let deep = if cfg!(windows) {
        "d:\\work\\proj\\src"
    } else {
        "/work/proj/src"
    };
    f.session_roots.set("s", [deep]);
    f.session_roots.set_roots_capable("s", true);

    let r = f.resolver.resolve(Some("s"), None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.feature_set_ids, vec![f.fs_b_id]);
}

// ---------------------------------------------------------------------------
// ClientGrant tier — rootless fallback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rootless_client_uses_grants() {
    let f = Fixture::new().await;
    let client_id = "rootless.example/client";
    f.make_client(client_id).await;
    f.client_repo
        .grant_feature_set(client_id, &f.space_id.to_string(), &f.fs_a_id)
        .await
        .unwrap();

    // Session declared no roots capability — Tier-2 grant lookup applies.
    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id))
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::ClientGrant);
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn rootless_client_without_grants_denies() {
    let f = Fixture::new().await;
    let client_id = "rootless.example/no-grants";
    f.make_client(client_id).await;
    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id))
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::Deny);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn capable_session_does_not_fall_through_to_grants() {
    // Critical: the leak we set out to fix. A roots-capable session whose
    // roots haven't arrived yet must NOT pick up any client grants. It
    // returns PendingRoots and only resolves once the roots actually land.
    let f = Fixture::new().await;
    let client_id = "permissive.example/client";
    f.make_client(client_id).await;
    f.client_repo
        .grant_feature_set(client_id, &f.space_id.to_string(), &f.fs_a_id)
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", true);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id))
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::PendingRoots);
    assert!(r.feature_set_ids.is_empty());
}

// ---------------------------------------------------------------------------
// Session-keyed routing — one client, many concurrent sessions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_sessions_on_different_roots_resolve_independently() {
    // A single client (e.g. two editor windows) holds two sessions on
    // different folders. Routing keys on session_id → that session's reported
    // roots → its own binding, so the two must NOT cross-talk: each resolves
    // to its own FeatureSet.
    let f = Fixture::new().await;
    let (root_a, root_b) = if cfg!(windows) {
        ("d:\\work\\a", "d:\\work\\b")
    } else {
        ("/work/a", "/work/b")
    };
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(root_a),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(root_b),
            f.space_id,
            f.fs_b_id.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set("sess-a", [root_a]);
    f.session_roots.set_roots_capable("sess-a", true);
    f.session_roots.set("sess-b", [root_b]);
    f.session_roots.set_roots_capable("sess-b", true);

    let ra = f.resolver.resolve(Some("sess-a"), None).await.unwrap();
    let rb = f.resolver.resolve(Some("sess-b"), None).await.unwrap();
    assert_eq!(ra.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(rb.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(ra.feature_set_ids, vec![f.fs_a_id.clone()]);
    assert_eq!(rb.feature_set_ids, vec![f.fs_b_id.clone()]);
}

#[tokio::test]
async fn two_sessions_on_same_root_resolve_to_the_same_binding() {
    // Two clients open the SAME folder. The (globally-unique) root is the
    // routing key, so both sessions resolve to the same binding — same Space
    // and FeatureSet. They're distinguished only by session_id for
    // *notification* delivery (see MCPNotifier), never for routing.
    let f = Fixture::new().await;
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(test_root()),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();
    for s in ["sess-1", "sess-2"] {
        f.session_roots.set(s, [test_root()]);
        f.session_roots.set_roots_capable(s, true);
    }

    let r1 = f.resolver.resolve(Some("sess-1"), None).await.unwrap();
    let r2 = f.resolver.resolve(Some("sess-2"), None).await.unwrap();
    assert_eq!(r1.feature_set_ids, vec![f.fs_a_id.clone()]);
    assert_eq!(r2.feature_set_ids, vec![f.fs_a_id.clone()]);
    assert_eq!(r1.space_id, r2.space_id);
}
