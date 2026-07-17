//! Decision-table tests for the FeatureSet resolver (capability-branched).
//!
//! Outcomes:
//!   1. **WorkspaceBinding** — session reported roots AND a binding matched
//!      one of them. `space_id` + `feature_set_ids[0]` come from the binding.
//!   2. **PendingRoots** — session declared MCP `roots` capability but the
//!      list hasn't arrived yet and the grace window hasn't lapsed. Empty FS
//!      list; resolver fires `list_changed` later when roots populate.
//!   3. **ClientGrant** — rootless-by-design client. Per-client grants
//!      from the `client_grants` table apply.
//!   4. **Unbound** — no binding matched; deny by default (empty FS list).
//!      Applies to an unmapped folder (roots reported, no binding), a rootless
//!      client with no grants, or a roots-capable client that never reported a
//!      folder once the grace window lapsed.
//!   5. **Deny** — defensive only: no default Space, or (degenerately) the
//!      default Space has no Starter FS. The Starter is builtin/seeded so this
//!      is normally unreachable. Empty FS list.

use std::sync::Arc;
use std::time::Duration;

use mcpmux_core::{
    normalize_workspace_root, FeatureSet, FeatureSetRepository, Machine, MachineRepository, Space,
    SpaceBaseDirRepository, SpaceRepository, WorkspaceBinding, WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, ResolutionSource, SessionRootsRegistry};
use mcpmux_storage::{
    Database, InboundClient, InboundClientRepository, RegistrationType, SqliteFeatureSetRepository,
    SqliteMachineRepository, SqliteSpaceBaseDirRepository, SqliteSpaceRepository,
    SqliteWorkspaceBindingRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Fixture {
    resolver: FeatureSetResolverService,
    session_roots: Arc<SessionRootsRegistry>,
    space_repo: Arc<dyn SpaceRepository>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    fs_repo: Arc<dyn FeatureSetRepository>,
    client_repo: Arc<InboundClientRepository>,
    machine_repo: SqliteMachineRepository,
    base_dir_repo: Arc<dyn SpaceBaseDirRepository>,
    space_id: Uuid,
    /// The default Space's auto-seeded Starter FS — used by tests that need a
    /// known FS id (e.g. ClientGrant / WorkspaceBinding paths).
    starter_fs_id: String,
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
        let machine_repo = SqliteMachineRepository::new(db.clone());
        let base_dir_repo: Arc<dyn SpaceBaseDirRepository> =
            Arc::new(SqliteSpaceBaseDirRepository::new(db.clone()));

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        // The default Space is seeded with its Starter FS by migrations.
        fs_repo
            .ensure_builtin_for_space(&space_id.to_string())
            .await
            .unwrap();
        let starter_fs_id = fs_repo
            .get_starter_for_space(&space_id.to_string())
            .await
            .unwrap()
            .expect("default space should have a Starter FS")
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
            session_roots.clone(),
            client_repo.clone(),
            fs_repo.clone(),
            base_dir_repo.clone(),
            None,
        );

        Self {
            resolver,
            session_roots,
            space_repo,
            binding_repo,
            fs_repo,
            client_repo,
            machine_repo,
            base_dir_repo,
            space_id,
            starter_fs_id,
            fs_a_id,
            fs_b_id,
        }
    }

    /// Build a second resolver over the same repos with a custom grace
    /// window — used to exercise the post-grace `Unbound` fallback
    /// deterministically (grace = 0) without sleeping.
    fn resolver_with_grace(&self, grace: Duration) -> FeatureSetResolverService {
        FeatureSetResolverService::new(
            self.space_repo.clone(),
            self.binding_repo.clone(),
            self.session_roots.clone(),
            self.client_repo.clone(),
            self.fs_repo.clone(),
            self.base_dir_repo.clone(),
            None,
        )
        .with_pending_grace(grace)
    }

    /// Build a resolver with this install's `local_machine_id` set.
    fn resolver_with_local_machine(&self, local_machine_id: Uuid) -> FeatureSetResolverService {
        FeatureSetResolverService::new(
            self.space_repo.clone(),
            self.binding_repo.clone(),
            self.session_roots.clone(),
            self.client_repo.clone(),
            self.fs_repo.clone(),
            self.base_dir_repo.clone(),
            Some(local_machine_id),
        )
    }

    /// Insert a machine catalog row and return its id.
    async fn make_machine(&self, name: &str) -> Uuid {
        let machine = Machine::new(name);
        let id = machine.id;
        self.machine_repo.create(&machine).await.unwrap();
        id
    }

    /// Create a second Space with its own Starter and a base directory, so
    /// base-dir scoping can be exercised. Returns `(space_id, starter_fs_id)`.
    async fn make_space_with_base_dir(&self, name: &str, base_dir: &str) -> (Uuid, String) {
        let space = Space::new(name);
        let space_id = space.id;
        self.space_repo.create(&space).await.unwrap();
        let starter_id = self
            .fs_repo
            .get_starter_for_space(&space_id.to_string())
            .await
            .unwrap()
            .expect("new space is seeded with a Starter")
            .id;
        self.base_dir_repo
            .add(&space_id, &normalize_workspace_root(base_dir))
            .await
            .unwrap();
        (space_id, starter_id)
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
            machine_id: None,
        };
        self.client_repo.save_client(&c).await.unwrap();
    }

    /// Create a non-default Space with a custom FeatureSet for lock tests.
    async fn make_alt_space_with_fs(&self, name: &str) -> (Uuid, String) {
        let space = Space::new(name);
        let space_id = space.id;
        self.space_repo.create(&space).await.unwrap();
        let fs = FeatureSet::new_custom("Alt", space_id.to_string());
        self.fs_repo.create(&fs).await.unwrap();
        (space_id, fs.id)
    }

    /// Pin a client to one Space (Tier 0 narrowing filter).
    async fn lock_client_to_space(&self, client_id: &str, space_id: Uuid) {
        self.client_repo
            .set_locked_space(client_id, Some(space_id))
            .await
            .unwrap();
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
// Unbound tier — deny by default
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_when_no_session_id_and_no_grants() {
    let f = Fixture::new().await;
    let r = f.resolver.resolve(None, None, None).await.unwrap();
    // No session, no grants → Unbound (deny by default).
    assert_eq!(r.source, ResolutionSource::Unbound);
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
    let r = f.resolver.resolve(Some("orphan"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::PendingRoots);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn default_when_session_explicitly_rootless_and_no_grants() {
    // Explicit Some(false) capability — client told us it doesn't
    // support roots — and no client grants. It told us it has no folder,
    // so settle straight on Unbound (no grace wait needed).
    let f = Fixture::new().await;
    f.session_roots.set_roots_capable("rootless", false);
    let r = f.resolver.resolve(Some("rootless"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn default_when_roots_reported_but_no_binding_matches() {
    let f = Fixture::new().await;
    let other = if cfg!(windows) { "d:\\tmp" } else { "/tmp" };
    f.session_roots.set("sess", [other]);
    let r = f.resolver.resolve(Some("sess"), None, None).await.unwrap();
    // Roots present but no binding → the folder is unmapped, so Unbound
    // (deny by default). Upstream still emits WorkspaceNeedsBinding so the
    // user can attach an explicit binding.
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_eq!(r.space_id, Some(f.space_id));
}

// Note: the "no Starter FS → Deny" branch is purely defensive — the Starter
// is builtin and seeded with every Space, so it can't be removed through the
// public API. Unbound sessions always get empty ids regardless of Starter
// membership; that off-switch is proven end-to-end in `effective_features.rs`.

// ---------------------------------------------------------------------------
// PendingRoots tier
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pending_when_capable_but_roots_havent_arrived() {
    let f = Fixture::new().await;
    f.session_roots.set_roots_capable("sess", true);
    // No roots set in the registry yet.
    let r = f.resolver.resolve(Some("sess"), None, None).await.unwrap();
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

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.space_id, Some(f.space_id));
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn deleting_a_bound_feature_set_drops_it_and_resolution_survives() {
    // Repro of the "Feature set not found" report: a folder mapped to two
    // FeatureSets; deleting one must drop it from the binding (FeatureSets are
    // soft-deleted, so the FK ON DELETE CASCADE can't fire) and resolution must
    // keep working — routing to the survivor, not erroring on the missing one.
    let f = Fixture::new().await;
    let binding = WorkspaceBinding::new_multi(
        normalize_workspace_root(test_root()),
        f.space_id,
        vec![f.fs_a_id.clone(), f.fs_b_id.clone()],
    );
    f.binding_repo.create(&binding).await.unwrap();

    f.fs_repo.delete(&f.fs_a_id).await.unwrap();

    // The binding no longer references the deleted FS...
    let reloaded = f.binding_repo.get(&binding.id).await.unwrap().unwrap();
    assert_eq!(reloaded.feature_set_ids, vec![f.fs_b_id.clone()]);

    // ...and the resolver routes via the binding to just the survivor.
    f.session_roots.set("s", [test_root()]);
    f.session_roots.set_roots_capable("s", true);
    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.feature_set_ids, vec![f.fs_b_id.clone()]);
}

#[tokio::test]
async fn no_inheritance_child_of_bound_parent_falls_back_to_default() {
    // Inheritance is intentionally NOT supported: a session whose reported root
    // is a CHILD of a bound parent does not pick up the parent's binding. With
    // no exact binding of its own it's an unmapped folder → Unbound (the
    // child does NOT inherit the parent's FS A).
    let f = Fixture::new().await;
    let (parent, child) = if cfg!(windows) {
        ("d:\\work", "d:\\work\\proj")
    } else {
        ("/work", "/work/proj")
    };
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(parent),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();

    // Child reports its root, no exact binding for it → Unbound (no
    // inheritance of the parent's FS A).
    f.session_roots.set("child", [child]);
    f.session_roots.set_roots_capable("child", true);
    let r = f.resolver.resolve(Some("child"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_ne!(r.feature_set_ids, vec![f.fs_a_id.clone()]);

    // The parent's own exact root still resolves to its binding.
    f.session_roots.set("parent", [parent]);
    f.session_roots.set_roots_capable("parent", true);
    let rp = f.resolver.resolve(Some("parent"), None, None).await.unwrap();
    assert_eq!(rp.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(rp.feature_set_ids, vec![f.fs_a_id]);
}

// ---------------------------------------------------------------------------
// Unbound tier — base-directory scoping (space_id only, empty ids)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unmapped_root_under_base_dir_scopes_to_that_space() {
    let f = Fixture::new().await;
    let (base, root) = if cfg!(windows) {
        ("d:\\work", "d:\\work\\proj")
    } else {
        ("/work", "/work/proj")
    };
    let (work_space, _work_starter) = f.make_space_with_base_dir("Work", base).await;

    // Session reports a folder UNDER Work's base dir, with no explicit binding.
    f.session_roots.set("s", [root]);
    f.session_roots.set_roots_capable("s", true);

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    // Scoped to the Work space — NOT the global default space.
    assert_eq!(r.space_id, Some(work_space));
    assert!(r.feature_set_ids.is_empty());
    assert_ne!(r.space_id, Some(f.space_id));
}

#[tokio::test]
async fn unmapped_root_outside_base_dirs_uses_default_space() {
    let f = Fixture::new().await;
    let base = if cfg!(windows) { "d:\\work" } else { "/work" };
    f.make_space_with_base_dir("Work", base).await;

    let other = if cfg!(windows) {
        "d:\\elsewhere"
    } else {
        "/elsewhere"
    };
    f.session_roots.set("s", [other]);
    f.session_roots.set_roots_capable("s", true);

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    // No base dir claims it → global default space.
    assert_eq!(r.space_id, Some(f.space_id));
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn nested_base_dir_most_specific_space_wins() {
    let f = Fixture::new().await;
    let (work_base, client_base, root) = if cfg!(windows) {
        ("d:\\work", "d:\\work\\client", "d:\\work\\client\\app")
    } else {
        ("/work", "/work/client", "/work/client/app")
    };
    f.make_space_with_base_dir("Work", work_base).await;
    let (client_space, _client_starter) = f.make_space_with_base_dir("Client", client_base).await;

    f.session_roots.set("s", [root]);
    f.session_roots.set_roots_capable("s", true);

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(
        r.space_id,
        Some(client_space),
        "the most-specific (longest) base dir wins"
    );
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn exact_binding_overrides_base_dir_scope() {
    // A WorkspaceBinding is more specific than a base-dir scope: even though
    // the root is under Work's base dir, an explicit binding wins.
    let f = Fixture::new().await;
    let (base, root) = if cfg!(windows) {
        ("d:\\work", "d:\\work\\proj")
    } else {
        ("/work", "/work/proj")
    };
    f.make_space_with_base_dir("Work", base).await;

    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(root),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();
    f.session_roots.set("s", [root]);
    f.session_roots.set_roots_capable("s", true);

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.space_id, Some(f.space_id));
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id.clone()]);
}

#[tokio::test]
async fn scoped_space_for_session_reports_base_dir_match() {
    let f = Fixture::new().await;
    let (base, root, outside) = if cfg!(windows) {
        ("d:\\work", "d:\\work\\proj", "d:\\elsewhere")
    } else {
        ("/work", "/work/proj", "/elsewhere")
    };
    let (work_space, _) = f.make_space_with_base_dir("Work", base).await;

    // A session whose root is under a base dir IS scoped (the meta-tools use
    // this to restrict to that one Space).
    f.session_roots.set("s", [root]);
    assert_eq!(
        f.resolver
            .scoped_space_for_session(Some("s"))
            .await
            .unwrap(),
        Some(work_space)
    );

    // A root outside every base dir is NOT scoped.
    f.session_roots.set("s2", [outside]);
    assert_eq!(
        f.resolver
            .scoped_space_for_session(Some("s2"))
            .await
            .unwrap(),
        None
    );

    // No session / no roots → not scoped.
    assert_eq!(
        f.resolver.scoped_space_for_session(None).await.unwrap(),
        None
    );
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
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::ClientGrant);
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn rootless_client_without_grants_falls_back_to_default() {
    let f = Fixture::new().await;
    let client_id = "rootless.example/no-grants";
    f.make_client(client_id).await;
    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    // Rootless + no grants → Unbound (deny by default).
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn roots_arrived_empty_falls_through_to_grants() {
    // Regression (resolver 3.1): a roots-capable client whose roots arrived
    // EMPTY (no folder open — Claude Desktop chat, empty editor window) is a
    // SETTLED rootless answer and must fall through to its client grants, not
    // hang forever in PendingRoots. Before the fix, `Some([])` was conflated
    // with `None` (not-yet-arrived) and stranded granted clients on
    // meta-tools-only with no recovery short of opening a folder.
    let f = Fixture::new().await;
    let client_id = "folderless.example/client";
    f.make_client(client_id).await;
    f.client_repo
        .grant_feature_set(client_id, &f.space_id.to_string(), &f.fs_a_id)
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", true);
    f.session_roots.set("s", Vec::<String>::new()); // roots ARRIVED, but empty
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::ClientGrant);
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn roots_arrived_empty_without_grants_falls_back_to_default() {
    // Same arrived-empty state but no grants → Unbound, NOT PendingRoots,
    // so the session settles (deny by default) instead of re-probing
    // `roots/list` forever.
    let f = Fixture::new().await;
    f.session_roots.set_roots_capable("s", true);
    f.session_roots.set("s", Vec::<String>::new());
    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
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
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::PendingRoots);
    assert!(r.feature_set_ids.is_empty());
}

#[tokio::test]
async fn pending_roots_grace_lapse_falls_back_to_space_default_not_grants() {
    // After the grace window lapses with no root reported, a roots-capable
    // session settles on Unbound — never on another client's grants. This
    // proves both halves of the grace design:
    //   1. it stops waiting (→ Unbound, not a perpetual PendingRoots), and
    //   2. it preserves per-session isolation (→ NOT ClientGrant, even though
    //      this client has a grant).
    let f = Fixture::new().await;
    let resolver = f.resolver_with_grace(Duration::ZERO);
    let client_id = "slow.example/client";
    f.make_client(client_id).await;
    f.client_repo
        .grant_feature_set(client_id, &f.space_id.to_string(), &f.fs_a_id)
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", true); // capable, but no roots ever arrive
    let r = resolver.resolve(Some("s"), Some(client_id), None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
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

    let ra = f.resolver.resolve(Some("sess-a"), None, None).await.unwrap();
    let rb = f.resolver.resolve(Some("sess-b"), None, None).await.unwrap();
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

    let r1 = f.resolver.resolve(Some("sess-1"), None, None).await.unwrap();
    let r2 = f.resolver.resolve(Some("sess-2"), None, None).await.unwrap();
    assert_eq!(r1.feature_set_ids, vec![f.fs_a_id.clone()]);
    assert_eq!(r2.feature_set_ids, vec![f.fs_a_id.clone()]);
    assert_eq!(r1.space_id, r2.space_id);
}

// ---------------------------------------------------------------------------
// Explicit workspace root via the X-Mcpmux-Workspace header (pinned root)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pinned_header_root_routes_to_binding_without_any_reported_roots() {
    // The deterministic fix for clients that don't report MCP roots reliably
    // (e.g. Cursor multiplexing one MCP host across windows): a session flagged
    // explicitly rootless, with no reported roots, still routes to its
    // workspace binding purely from the X-Mcpmux-Workspace header the gateway
    // pinned.
    let f = Fixture::new().await;
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(test_root()),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", false); // client says it has no roots
    f.session_roots.set_pinned("s", test_root()); // ...but the header pins one

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.space_id, Some(f.space_id));
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn pinned_header_root_overrides_a_conflicting_reported_root() {
    // The header is authoritative. When the client reports a stale/wrong root
    // AND a header root is pinned, the pinned one wins — exactly the Cursor
    // "reported the wrong window's root" failure, now corrected.
    let f = Fixture::new().await;
    let (reported, pinned) = if cfg!(windows) {
        ("d:\\work\\reported", "d:\\work\\pinned")
    } else {
        ("/work/reported", "/work/pinned")
    };
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(reported),
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();
    f.binding_repo
        .create(&WorkspaceBinding::new(
            normalize_workspace_root(pinned),
            f.space_id,
            f.fs_b_id.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set("s", [reported]);
    f.session_roots.set_roots_capable("s", true);
    f.session_roots.set_pinned("s", pinned);

    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    // Resolved to the PINNED root's FS (B), not the reported root's FS (A).
    assert_eq!(r.feature_set_ids, vec![f.fs_b_id]);
    assert_ne!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn header_takes_priority_but_reported_roots_still_map_without_one() {
    // The two mechanisms coexist by design: a session that only reports MCP
    // roots (no header) keeps mapping via those roots; pinning a header root
    // then overrides them. This guards against the pin ever becoming
    // unconditional and breaking roots-reporting clients (VS Code, Claude Code).
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

    // No header pinned → the reported root drives resolution (FS A).
    f.session_roots.set("s", [root_a]);
    f.session_roots.set_roots_capable("s", true);
    let reported = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(reported.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(reported.feature_set_ids, vec![f.fs_a_id.clone()]);

    // Pin a header root for a different folder → it takes priority (FS B).
    f.session_roots.set_pinned("s", root_b);
    let pinned = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(pinned.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(pinned.feature_set_ids, vec![f.fs_b_id.clone()]);
}

#[tokio::test]
async fn pinned_header_root_without_binding_is_unbound() {
    // A pinned header root with no binding is Tier 1b — deny by default.
    // Upstream emits WorkspaceNeedsBinding so the user can attach a mapping.
    let f = Fixture::new().await;
    f.session_roots.set_pinned("s", test_root());
    let r = f.resolver.resolve(Some("s"), None, None).await.unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_eq!(r.space_id, Some(f.space_id));
}

// ---------------------------------------------------------------------------
// Id-type binding tier (Tier 2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn id_binding_routes_rootless_client_without_grants() {
    let f = Fixture::new().await;
    let client_id = "api-key.example/headless";
    f.make_client(client_id).await;
    f.binding_repo
        .create(&WorkspaceBinding::new_id(
            client_id,
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn rootless_client_without_id_binding_is_unbound_not_starter() {
    let f = Fixture::new().await;
    let client_id = "api-key.example/unmapped";
    f.make_client(client_id).await;
    f.session_roots.set_roots_capable("s", false);

    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_ne!(r.source, ResolutionSource::SpaceDefault);
}

#[tokio::test]
async fn id_binding_beats_client_grant() {
    let f = Fixture::new().await;
    let client_id = "api-key.example/both";
    f.make_client(client_id).await;
    f.client_repo
        .grant_feature_set(client_id, &f.space_id.to_string(), &f.fs_b_id)
        .await
        .unwrap();
    f.binding_repo
        .create(&WorkspaceBinding::new_id(
            client_id,
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.feature_set_ids, vec![f.fs_a_id]);
}

#[tokio::test]
async fn reported_roots_without_path_binding_stays_unbound_skips_id_binding() {
    // Tier 1b: roots reported but no path binding → Unbound immediately;
    // Tier 2 id binding must NOT run for roots-capable sessions with folders.
    let f = Fixture::new().await;
    let client_id = "cursor.example/window";
    f.make_client(client_id).await;
    f.binding_repo
        .create(&WorkspaceBinding::new_id(
            client_id,
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();

    let other = if cfg!(windows) { "d:\\tmp" } else { "/tmp" };
    f.session_roots.set("s", [other]);
    f.session_roots.set_roots_capable("s", true);

    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_ne!(r.source, ResolutionSource::SpaceDefault);
}

#[tokio::test]
async fn request_machine_header_selects_machine_scoped_id_binding() {
    let f = Fixture::new().await;
    let gondor_id = f.make_machine("Gondor").await;
    let rohan_id = f.make_machine("Rohan").await;
    let client_id = "api-key.example/shared";

    f.make_client(client_id).await;

    let global = WorkspaceBinding::new_id(client_id, f.space_id, f.fs_a_id.clone());
    f.binding_repo.create(&global).await.unwrap();

    let mut rohan = WorkspaceBinding::new_id(client_id, f.space_id, f.fs_b_id.clone());
    rohan.machine_id = Some(rohan_id);
    f.binding_repo.create(&rohan).await.unwrap();

    f.session_roots.set_roots_capable("s", false);
    let resolver = f.resolver_with_local_machine(gondor_id);

    let without_header = resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(without_header.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(without_header.feature_set_ids, vec![f.fs_a_id]);

    let with_rohan_header = resolver
        .resolve(Some("s"), Some(client_id), Some(rohan_id))
        .await
        .unwrap();
    assert_eq!(with_rohan_header.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(with_rohan_header.feature_set_ids, vec![f.fs_b_id]);
}

// ---------------------------------------------------------------------------
// Space lock narrowing filter (Tier 0)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn locked_client_in_space_id_binding_resolves() {
    let f = Fixture::new().await;
    let (locked_space, locked_fs) = f.make_alt_space_with_fs("Locked").await;
    let client_id = "api-key.example/locked-in-space";
    f.make_client(client_id).await;
    f.lock_client_to_space(client_id, locked_space).await;
    f.binding_repo
        .create(&WorkspaceBinding::new_id(
            client_id,
            locked_space,
            locked_fs.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(r.space_id, Some(locked_space));
    assert_eq!(r.feature_set_ids, vec![locked_fs]);
}

#[tokio::test]
async fn locked_client_id_binding_in_other_space_is_unbound() {
    let f = Fixture::new().await;
    let (locked_space, _locked_fs) = f.make_alt_space_with_fs("Locked").await;
    let client_id = "api-key.example/wrong-space-binding";
    f.make_client(client_id).await;
    f.lock_client_to_space(client_id, locked_space).await;
    f.binding_repo
        .create(&WorkspaceBinding::new_id(
            client_id,
            f.space_id,
            f.fs_a_id.clone(),
        ))
        .await
        .unwrap();

    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_eq!(r.space_id, Some(locked_space));
    assert_ne!(r.space_id, Some(f.space_id));
    assert_ne!(r.feature_set_ids, vec![f.fs_a_id]);
    assert_ne!(r.source, ResolutionSource::SpaceDefault);
}

#[tokio::test]
async fn locked_client_without_any_binding_is_unbound() {
    let f = Fixture::new().await;
    let (locked_space, _locked_fs) = f.make_alt_space_with_fs("Locked").await;
    let client_id = "api-key.example/locked-unmapped";
    f.make_client(client_id).await;
    f.lock_client_to_space(client_id, locked_space).await;

    f.session_roots.set_roots_capable("s", false);
    let r = f
        .resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
    assert_eq!(r.space_id, Some(locked_space));
    assert_ne!(r.source, ResolutionSource::SpaceDefault);
}

// ---------------------------------------------------------------------------
// Request machine header — per-device identity over shared tunnel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn request_machine_header_outranks_client_and_local_machine() {
    let f = Fixture::new().await;
    let gondor_id = f.make_machine("Gondor").await;
    let rohan_id = f.make_machine("Rohan").await;
    let root = normalize_workspace_root(test_root());
    let client_id = "cursor.example/shared";

    let mut gondor_binding = WorkspaceBinding::new(root.clone(), f.space_id, f.fs_a_id.clone());
    gondor_binding.machine_id = Some(gondor_id);
    f.binding_repo.create(&gondor_binding).await.unwrap();

    let mut rohan_binding = WorkspaceBinding::new(root.clone(), f.space_id, f.fs_b_id.clone());
    rohan_binding.machine_id = Some(rohan_id);
    f.binding_repo.create(&rohan_binding).await.unwrap();

    f.make_client(client_id).await;
    f.client_repo
        .set_machine_id(client_id, Some(gondor_id))
        .await
        .unwrap();

    f.session_roots.set("s", [root.as_str()]);
    f.session_roots.set_roots_capable("s", true);

    let resolver = f.resolver_with_local_machine(gondor_id);

    let without_header = resolver
        .resolve(Some("s"), Some(client_id), None)
        .await
        .unwrap();
    assert_eq!(without_header.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(without_header.feature_set_ids, vec![f.fs_a_id]);

    let with_rohan_header = resolver
        .resolve(Some("s"), Some(client_id), Some(rohan_id))
        .await
        .unwrap();
    assert_eq!(with_rohan_header.source, ResolutionSource::WorkspaceBinding);
    assert_eq!(with_rohan_header.feature_set_ids, vec![f.fs_b_id]);
}

#[tokio::test]
async fn request_machine_header_enables_deny_when_only_other_machine_bound() {
    let f = Fixture::new().await;
    let gondor_id = f.make_machine("Gondor").await;
    let rohan_id = f.make_machine("Rohan").await;
    let root = normalize_workspace_root(test_root());

    let mut gondor_binding = WorkspaceBinding::new(root.clone(), f.space_id, f.fs_a_id.clone());
    gondor_binding.machine_id = Some(gondor_id);
    f.binding_repo.create(&gondor_binding).await.unwrap();

    f.session_roots.set("s", [root.as_str()]);
    f.session_roots.set_roots_capable("s", true);

    let resolver = f.resolver_with_local_machine(gondor_id);
    let r = resolver
        .resolve(Some("s"), None, Some(rohan_id))
        .await
        .unwrap();
    assert_eq!(r.source, ResolutionSource::Unbound);
    assert!(r.feature_set_ids.is_empty());
}

// ---------------------------------------------------------------------------
// effective_machine_id — shared bind-write / resolve-read priority
// ---------------------------------------------------------------------------

#[tokio::test]
async fn effective_machine_id_prefers_header_then_client_then_local() {
    let f = Fixture::new().await;
    let gondor_id = f.make_machine("Gondor").await;
    let rohan_id = f.make_machine("Rohan").await;
    let client_id = "cursor.example/shared";

    f.make_client(client_id).await;
    f.client_repo
        .set_machine_id(client_id, Some(gondor_id))
        .await
        .unwrap();

    let resolver = f.resolver_with_local_machine(gondor_id);

    assert_eq!(
        resolver
            .effective_machine_id(Some(client_id), Some(rohan_id))
            .await
            .unwrap(),
        Some(rohan_id),
    );
    assert_eq!(
        resolver
            .effective_machine_id(Some(client_id), None)
            .await
            .unwrap(),
        Some(gondor_id),
    );
    assert_eq!(
        resolver.effective_machine_id(None, None).await.unwrap(),
        Some(gondor_id),
    );
    assert_eq!(
        f.resolver
            .effective_machine_id(Some(client_id), None)
            .await
            .unwrap(),
        Some(gondor_id),
    );
    assert_eq!(
        f.resolver.effective_machine_id(None, None).await.unwrap(),
        None,
    );
}
