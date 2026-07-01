//! End-to-end "effective features" tests.
//!
//! A workspace mapping must yield the *exact* tools the connected client
//! sees. That's a two-step the gateway request handler runs on every
//! `tools/list`:
//!
//!   1. FeatureSetResolver: session_id → reported roots → binding →
//!      `(space_id, feature_set_ids)`.
//!   2. FeatureService.get_tools_for_grants: `(space_id, feature_set_ids)` →
//!      the FS-filtered, availability-gated tool list.
//!
//! The two halves are unit-proven separately (feature_set_resolver.rs and
//! mcp_flows.rs). These tests chain them over real SQLite repos so the whole
//! path is byte-proven: the mapping you choose is the toolset the client gets,
//! per session, with no cross-root leakage.

#![allow(clippy::cloned_ref_to_slice_refs)]

use std::sync::Arc;

use mcpmux_core::{
    normalize_workspace_root, FeatureSet, FeatureSetMember, FeatureSetRepository, MemberMode,
    MemberType, ServerFeature, ServerFeatureRepository, SpaceRepository, WorkspaceBinding,
    WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, ResolutionSource, SessionRootsRegistry};
use mcpmux_gateway::{FeatureService, PrefixCacheService};
use mcpmux_storage::{
    Database, InboundClientRepository, SqliteFeatureSetRepository, SqliteServerFeatureRepository,
    SqliteSpaceBaseDirRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Ctx {
    resolver: FeatureSetResolverService,
    feature_service: FeatureService,
    session_roots: Arc<SessionRootsRegistry>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    fs_repo: Arc<dyn FeatureSetRepository>,
    space_id: Uuid,
    space_id_str: String,
    /// FeatureSet whose members are the two `github` tools.
    fs_github: String,
    /// FeatureSet whose only member is the `firebase` tool.
    fs_firebase: String,
    /// Raw ServerFeature ids for composing custom FeatureSets in tests.
    gh_issue_id: String,
    fb_deploy_id: String,
}

impl Ctx {
    async fn new() -> Self {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
        let fs_repo: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));
        let binding_repo: Arc<dyn WorkspaceBindingRepository> =
            Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
        let feature_repo: Arc<dyn ServerFeatureRepository> =
            Arc::new(SqliteServerFeatureRepository::new(db.clone()));
        let client_repo = Arc::new(InboundClientRepository::new(db.clone()));

        let space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = space.id;
        let space_id_str = space_id.to_string();

        // Prefix assignment so qualified names resolve like production.
        let prefix_cache = Arc::new(PrefixCacheService::new());
        prefix_cache
            .assign_prefix_runtime(&space_id_str, "github", Some("gh"))
            .await;
        prefix_cache
            .assign_prefix_runtime(&space_id_str, "firebase", Some("fb"))
            .await;

        // Seed three tools across two servers.
        let mut gh_issue = ServerFeature::tool(space_id, "github", "create_issue");
        gh_issue.is_available = true;
        let mut gh_repos = ServerFeature::tool(space_id, "github", "list_repos");
        gh_repos.is_available = true;
        let mut fb_deploy = ServerFeature::tool(space_id, "firebase", "deploy");
        fb_deploy.is_available = true;
        feature_repo.upsert(&gh_issue).await.unwrap();
        feature_repo.upsert(&gh_repos).await.unwrap();
        feature_repo.upsert(&fb_deploy).await.unwrap();

        // Two FeatureSets: one for the github tools, one for the firebase tool.
        let fs_github = FeatureSet::new_custom("GitHub", space_id.to_string());
        let fs_firebase = FeatureSet::new_custom("Firebase", space_id.to_string());
        fs_repo.create(&fs_github).await.unwrap();
        fs_repo.create(&fs_firebase).await.unwrap();
        fs_repo
            .add_feature_member(&fs_github.id, &gh_issue.id.to_string(), MemberMode::Include)
            .await
            .unwrap();
        fs_repo
            .add_feature_member(&fs_github.id, &gh_repos.id.to_string(), MemberMode::Include)
            .await
            .unwrap();
        fs_repo
            .add_feature_member(
                &fs_firebase.id,
                &fb_deploy.id.to_string(),
                MemberMode::Include,
            )
            .await
            .unwrap();

        let session_roots = SessionRootsRegistry::new();
        let resolver = FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
            client_repo.clone(),
            fs_repo.clone(),
            Arc::new(SqliteSpaceBaseDirRepository::new(db.clone())),
            None,
        );
        let feature_service =
            FeatureService::new(feature_repo.clone(), fs_repo.clone(), prefix_cache);

        Self {
            resolver,
            feature_service,
            session_roots,
            binding_repo,
            fs_repo,
            space_id,
            space_id_str,
            fs_github: fs_github.id,
            fs_firebase: fs_firebase.id,
            gh_issue_id: gh_issue.id.to_string(),
            fb_deploy_id: fb_deploy.id.to_string(),
        }
    }

    /// Bind a (capable) session to a root and a FeatureSet.
    async fn bind(&self, session_id: &str, root: &str, fs_id: &str) {
        self.binding_repo
            .create(&WorkspaceBinding::new(
                normalize_workspace_root(root),
                self.space_id,
                fs_id.to_string(),
            ))
            .await
            .unwrap();
        self.session_roots.set(session_id, [root]);
        self.session_roots.set_roots_capable(session_id, true);
    }

    /// The tools a session actually sees — the exact two-step the request
    /// handler runs: resolve the mapping, then pull its FS-filtered tools.
    async fn effective_tools(&self, session_id: &str) -> Vec<String> {
        let resolved = self.resolver.resolve(Some(session_id), None, None).await.unwrap();
        let tools = self
            .feature_service
            .get_tools_for_grants(&self.space_id_str, &resolved.feature_set_ids)
            .await
            .unwrap();
        let mut names: Vec<String> = tools.into_iter().map(|t| t.feature_name).collect();
        names.sort();
        names
    }
}

/// The mapping a session resolves to determines exactly which tools it sees,
/// independently per session — two roots, two toolsets, zero leakage.
#[tokio::test(flavor = "multi_thread")]
async fn mapping_determines_effective_tools_per_session() {
    let ctx = Ctx::new().await;
    let (root_gh, root_fb) = if cfg!(windows) {
        ("d:\\work\\gh", "d:\\work\\fb")
    } else {
        ("/work/gh", "/work/fb")
    };
    ctx.bind("sess-gh", root_gh, &ctx.fs_github).await;
    ctx.bind("sess-fb", root_fb, &ctx.fs_firebase).await;

    assert_eq!(
        ctx.effective_tools("sess-gh").await,
        vec!["create_issue".to_string(), "list_repos".to_string()],
    );
    assert_eq!(
        ctx.effective_tools("sess-fb").await,
        vec!["deploy".to_string()],
    );
}

/// Multi-client, SAME workspace root: two distinct client sessions (e.g. two
/// editors opening the same folder) must resolve to the SAME binding and see
/// an identical toolset — the root is the routing key, not the session/client.
#[tokio::test(flavor = "multi_thread")]
async fn two_clients_same_root_see_identical_tools() {
    let ctx = Ctx::new().await;
    let root = if cfg!(windows) {
        "d:\\work\\shared"
    } else {
        "/work/shared"
    };
    // First client opens the folder and binds it.
    ctx.bind("client-a", root, &ctx.fs_github).await;
    // Second client (different session) reports the very same root.
    ctx.session_roots.set("client-b", [root]);
    ctx.session_roots.set_roots_capable("client-b", true);

    let a = ctx.effective_tools("client-a").await;
    let b = ctx.effective_tools("client-b").await;
    assert_eq!(
        a,
        vec!["create_issue".to_string(), "list_repos".to_string()]
    );
    assert_eq!(
        a, b,
        "two clients on the same root must see identical tools"
    );
}

/// Multi-client, DIFFERENT-shaped roots for the SAME folder: a binding created
/// from the canonical Windows drive path must still match a client that
/// reports that folder with a doubled leading slash (`//d:/…`, seen live from
/// a `file:////D:/…` URI). Regression for the normalization bug that stored
/// `\\d:\…` and silently never matched the canonical form.
#[tokio::test(flavor = "multi_thread")]
async fn doubled_slash_root_resolves_to_canonical_binding() {
    let ctx = Ctx::new().await;
    // Binding created from the canonical drive form.
    ctx.bind("canon", "d:\\work\\proj", &ctx.fs_github).await;

    // A second client reports the SAME folder via the doubled-slash form;
    // SessionRootsRegistry::set normalizes it, and it must collapse to the
    // canonical key so the resolver matches the existing binding.
    ctx.session_roots.set("dslash", ["//d:/work/proj"]);
    ctx.session_roots.set_roots_capable("dslash", true);

    assert_eq!(
        ctx.effective_tools("dslash").await,
        vec!["create_issue".to_string(), "list_repos".to_string()],
        "doubled-slash root must route to the binding created from the canonical form"
    );
}

/// An *empty* mapping (a binding with zero feature sets) is valid: the session
/// routes to the Space (source = WorkspaceBinding) but sees zero Space tools.
/// Built-in servers (gated per Space) are layered on by the request handler and
/// aren't part of get_tools_for_grants.
#[tokio::test(flavor = "multi_thread")]
async fn empty_mapping_yields_zero_effective_tools() {
    let ctx = Ctx::new().await;
    let root = if cfg!(windows) {
        "d:\\work\\none"
    } else {
        "/work/none"
    };
    ctx.binding_repo
        .create(&WorkspaceBinding::new_multi(
            normalize_workspace_root(root),
            ctx.space_id,
            vec![],
        ))
        .await
        .unwrap();
    ctx.session_roots.set("sess", [root]);
    ctx.session_roots.set_roots_capable("sess", true);

    let resolved = ctx.resolver.resolve(Some("sess"), None, None).await.unwrap();
    assert_eq!(resolved.source, ResolutionSource::WorkspaceBinding);
    assert!(resolved.feature_set_ids.is_empty());
    assert!(ctx.effective_tools("sess").await.is_empty());
}

/// A reported root with no mapping resolves to `Unbound` (deny by default) —
/// zero backend tools regardless of Starter FS membership.
#[tokio::test(flavor = "multi_thread")]
async fn unbound_session_returns_no_tools() {
    let ctx = Ctx::new().await;

    // Put one tool in the default Space's Starter FS — unbound sessions must
    // NOT see it (Starter is no longer the silent fallback).
    let starter = ctx
        .fs_repo
        .get_starter_for_space(&ctx.space_id_str)
        .await
        .unwrap()
        .expect("default Space has a Starter FS");
    ctx.fs_repo
        .add_feature_member(&starter.id, &ctx.gh_issue_id, MemberMode::Include)
        .await
        .unwrap();

    let root = if cfg!(windows) {
        "d:\\work\\unmapped"
    } else {
        "/work/unmapped"
    };
    ctx.session_roots.set("sess", [root]);
    ctx.session_roots.set_roots_capable("sess", true);

    let resolved = ctx.resolver.resolve(Some("sess"), None, None).await.unwrap();
    assert_eq!(resolved.source, ResolutionSource::Unbound);
    assert!(resolved.feature_set_ids.is_empty());
    assert!(ctx.effective_tools("sess").await.is_empty());
}

/// Unbound sessions get zero effective tools even when the Starter FS is
/// populated — deny by default is independent of Starter membership.
#[tokio::test(flavor = "multi_thread")]
async fn empty_starter_grants_nothing_to_unbound_session() {
    let ctx = Ctx::new().await;

    // Sanity-check the precondition: the seeded Starter has no members.
    let starter = ctx
        .fs_repo
        .get_starter_for_space(&ctx.space_id_str)
        .await
        .unwrap()
        .expect("default Space has a Starter FS");
    assert!(
        ctx.fs_repo
            .get_feature_members(&starter.id)
            .await
            .unwrap()
            .is_empty(),
        "seeded Starter should start empty",
    );

    let root = if cfg!(windows) {
        "d:\\work\\unmapped-empty"
    } else {
        "/work/unmapped-empty"
    };
    ctx.session_roots.set("sess", [root]);
    ctx.session_roots.set_roots_capable("sess", true);

    let resolved = ctx.resolver.resolve(Some("sess"), None, None).await.unwrap();
    assert_eq!(resolved.source, ResolutionSource::Unbound);
    assert!(resolved.feature_set_ids.is_empty());
    assert!(ctx.effective_tools("sess").await.is_empty());
}

/// Regression (resolution #9): a composition CYCLE (FS X ⊇ Y, FS Y ⊇ X) must
/// not infinite-loop the resolver on the live tools/list path. The command
/// layer now blocks creating such a cycle, but the repository can still hold
/// one (legacy data / direct writes), so the resolver must terminate
/// defensively — returning the de-duplicated union of both sets' features.
#[tokio::test(flavor = "multi_thread")]
async fn composition_cycle_terminates_and_returns_union() {
    let ctx = Ctx::new().await;

    // Two custom FeatureSets that include each other.
    let mut fs_x = FeatureSet::new_custom("CycX", ctx.space_id.to_string());
    let mut fs_y = FeatureSet::new_custom("CycY", ctx.space_id.to_string());
    ctx.fs_repo.create(&fs_x).await.unwrap();
    ctx.fs_repo.create(&fs_y).await.unwrap();

    let member = |fs_id: &str, mtype: MemberType, mid: String| FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: fs_id.to_string(),
        member_type: mtype,
        member_id: mid,
        mode: MemberMode::Include,
        surfaced: false,
    };

    // X ⊇ {gh_issue (feature), Y (featureset)}
    fs_x.members = vec![
        member(&fs_x.id, MemberType::Feature, ctx.gh_issue_id.clone()),
        member(&fs_x.id, MemberType::FeatureSet, fs_y.id.clone()),
    ];
    // Y ⊇ {fb_deploy (feature), X (featureset)}  ← closes the cycle
    fs_y.members = vec![
        member(&fs_y.id, MemberType::Feature, ctx.fb_deploy_id.clone()),
        member(&fs_y.id, MemberType::FeatureSet, fs_x.id.clone()),
    ];
    ctx.fs_repo.update(&fs_x).await.unwrap();
    ctx.fs_repo.update(&fs_y).await.unwrap();

    let root = if cfg!(windows) {
        "d:\\work\\cyc"
    } else {
        "/work/cyc"
    };
    ctx.bind("sess", root, &fs_x.id).await;

    // Must return (not hang) — the union of both sets' features, de-duplicated.
    let tools = ctx.effective_tools("sess").await;
    assert_eq!(
        tools,
        vec!["create_issue".to_string(), "deploy".to_string()],
        "cyclic composition must resolve to the de-duplicated union and terminate"
    );
}
