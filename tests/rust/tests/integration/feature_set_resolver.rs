//! Decision-table tests for the FeatureSet resolver.
//!
//! Post-simplification the resolver has exactly two outcomes:
//!
//!   1. **WorkspaceBinding** — session reports roots AND a binding matches.
//!      Both `space_id` and `feature_set_id` are pulled directly from the
//!      binding row — no "active FS" indirection.
//!   2. **Default** — no roots / no match. Returns the default Space's
//!      auto-seeded `fs_default_<space>` FeatureSet.

use std::sync::Arc;

use mcpmux_core::{
    normalize_workspace_root, FeatureSet, FeatureSetRepository, SpaceRepository, WorkspaceBinding,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, ResolutionSource, SessionRootsRegistry};
use mcpmux_storage::{
    Database, SqliteFeatureSetRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Fixture {
    resolver: FeatureSetResolverService,
    session_roots: Arc<SessionRootsRegistry>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    space_id: Uuid,
    default_fs_id: String,
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

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        // Migration seeds exactly one builtin per space: Default.
        let default_fs_id = fs_repo
            .get_default_for_space(&space_id.to_string())
            .await
            .unwrap()
            .expect("Default FS seeded by migration")
            .id;

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
            fs_repo.clone(),
            session_roots.clone(),
        );

        Self {
            resolver,
            session_roots,
            binding_repo,
            space_id,
            default_fs_id,
            fs_a_id,
            fs_b_id,
        }
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
// Tier 2: Default fallback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_when_no_session_id() {
    let f = Fixture::new().await;
    let r = f.resolver.resolve(None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Default);
    assert_eq!(r.space_id, Some(f.space_id));
    assert_eq!(r.feature_set_id, Some(f.default_fs_id));
}

#[tokio::test]
async fn default_when_session_has_no_roots() {
    let f = Fixture::new().await;
    let r = f.resolver.resolve(Some("orphan")).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Default);
    assert_eq!(r.feature_set_id, Some(f.default_fs_id));
}

#[tokio::test]
async fn default_when_no_binding_matches_reported_root() {
    let f = Fixture::new().await;
    let other = if cfg!(windows) { "d:\\tmp" } else { "/tmp" };
    f.session_roots.set("sess", [other]);
    let r = f.resolver.resolve(Some("sess")).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Default);
    assert_eq!(r.feature_set_id, Some(f.default_fs_id));
}

// ---------------------------------------------------------------------------
// Tier 1: WorkspaceBinding — concrete (space_id, feature_set_id) pointers
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

    let r = f.resolver.resolve(Some("s")).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.space_id, Some(f.space_id));
    assert_eq!(r.feature_set_id, Some(f.fs_a_id));
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

    let r = f.resolver.resolve(Some("s")).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.feature_set_id, Some(f.fs_b_id));
}
