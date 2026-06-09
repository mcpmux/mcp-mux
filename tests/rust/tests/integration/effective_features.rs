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
    normalize_workspace_root, FeatureSet, FeatureSetRepository, MemberMode, ServerFeature,
    ServerFeatureRepository, SpaceRepository, WorkspaceBinding, WorkspaceBindingRepository,
};
use mcpmux_gateway::services::{FeatureSetResolverService, SessionRootsRegistry};
use mcpmux_gateway::{FeatureService, PrefixCacheService};
use mcpmux_storage::{
    Database, InboundClientRepository, SqliteFeatureSetRepository, SqliteServerFeatureRepository,
    SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Ctx {
    resolver: FeatureSetResolverService,
    feature_service: FeatureService,
    session_roots: Arc<SessionRootsRegistry>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    space_id: Uuid,
    space_id_str: String,
    /// FeatureSet whose members are the two `github` tools.
    fs_github: String,
    /// FeatureSet whose only member is the `firebase` tool.
    fs_firebase: String,
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
        );
        let feature_service =
            FeatureService::new(feature_repo.clone(), fs_repo.clone(), prefix_cache);

        Self {
            resolver,
            feature_service,
            session_roots,
            binding_repo,
            space_id,
            space_id_str,
            fs_github: fs_github.id,
            fs_firebase: fs_firebase.id,
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
        let resolved = self.resolver.resolve(Some(session_id), None).await.unwrap();
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

/// A reported root with no mapping resolves to Deny → empty feature-set list
/// → zero effective tools. (The gateway then appends the `mcpmux_*` Tool
/// Optimization tools only when that switch is on — covered in meta_tools.)
#[tokio::test(flavor = "multi_thread")]
async fn unbound_session_sees_zero_effective_tools() {
    let ctx = Ctx::new().await;
    let root = if cfg!(windows) {
        "d:\\work\\unmapped"
    } else {
        "/work/unmapped"
    };
    ctx.session_roots.set("sess", [root]);
    ctx.session_roots.set_roots_capable("sess", true);

    assert!(ctx.effective_tools("sess").await.is_empty());
}
