//! FeatureSet Resolver Service.
//!
//! Capability-branched resolution. The branch point is the MCP `roots`
//! capability declared by the client at `initialize`:
//!
//! ```text
//! resolve(session_id, client_id):
//!     // Tier 1 — roots-capable session with reported roots
//!     if session reported roots AND a binding matches:
//!         return (binding.space_id, [binding.feature_set_id], WorkspaceBinding)
//!
//!     // Tier 1b — roots-capable, roots reported, but no binding yet
//!     if session reported roots AND no binding matched:
//!         return (default_space, [starter_fs], SpaceDefault)   // unmapped folder
//!         // (also emits WorkspaceNeedsBinding upstream so the user can still map)
//!
//!     // Tier 1c — declared `roots` but they haven't arrived yet
//!     if session declared `roots` AND none yet in registry:
//!         if within the pending-roots grace window:
//!             return ([], default_space, PendingRoots)   // wait for the root
//!         else:
//!             return (default_space, [starter_fs], SpaceDefault)   // gave up waiting
//!
//!     // Tier 2 — rootless-by-design (Claude.ai web, ChatGPT, …)
//!     if client has grants in the default space:
//!         return (default_space, grants, ClientGrant)
//!
//!     // Tier 3 — no roots, no grants
//!     return (default_space, [starter_fs], SpaceDefault)
//! ```
//!
//! # Default fallback (the "every folder needs mapping" fix)
//!
//! When nothing more specific resolves — an unmapped folder (Tier 1b), a
//! rootless client with no grants, or a roots-capable client that never
//! reported a folder (Tier 1c after the grace window) — the resolver falls
//! back to the **default Space's Starter FeatureSet** instead of denying.
//! That makes folders work out of the box: a freshly-opened project gets the
//! Starter tools immediately, and the user only *needs* an explicit
//! [`WorkspaceBinding`](mcpmux_core::WorkspaceBinding) when they want a folder
//! to see something *other* than the default. The Starter FS's membership is
//! the control surface: edit it to change what every unmapped folder sees, or
//! empty it to grant nothing by default. (The Starter is builtin and can't be
//! deleted, so the fallback always has a target.)
//!
//! ## Grace window — avoid "default then mapped" flips
//!
//! A roots-capable client that's *about* to report a folder must resolve
//! straight to that folder's binding (or the default-for-unmapped), never
//! flash the default tools first and then flip. So while a session has
//! declared (or might declare) `roots` and none have arrived yet, the
//! resolver holds at `PendingRoots` (empty) for a short grace window rather
//! than defaulting immediately. Only once the window lapses with no root in
//! sight does it settle on `SpaceDefault`, so a misbehaving client that never
//! reports isn't stranded on meta-tools forever. A roots-capable session
//! **never** falls through to another client's grants — after the grace it
//! goes straight to the Space default, preserving per-session isolation.
//!
//! The caller's client identity is used **only** for the rootless fallback —
//! every roots-capable session routes via its own reported roots, regardless
//! of which OAuth client opened it. This is what makes "two VS Code windows
//! sharing one OAuth identity" route independently.
//!
//! # Trust model (deliberate design decision)
//!
//! Roots are client-asserted, so **any** OAuth-approved local client can
//! report roots matching **any** binding and be routed into that binding's
//! Space — including its tools and the credentials behind them
//! (SECURITY_AUDIT HIGH-1, accepted 2026-06-12). The boundary is the
//! one-time client approval: every client the user approves is trusted with
//! every workspace binding, the same way it is trusted with the local
//! filesystem it already runs on. The gateway binds to loopback only, and
//! unapproved clients never get a token. If per-client Space isolation is
//! ever needed, gate Tier 1 on a per-`(client, space)` trust table before
//! honoring the binding.
//!
//! Roots-capable detection is stamped at `on_initialized` time into
//! [`SessionRootsRegistry::set_roots_capable`].
//!
//! # Explicit workspace root via the `X-Mcpmux-Workspace` header
//!
//! A connection can carry an explicit workspace root in the
//! `X-Mcpmux-Workspace` HTTP header, injected by McpMux's per-workspace client
//! configs. The OAuth middleware pins it into
//! [`SessionRootsRegistry::set_pinned`], where it **shadows** the client's
//! probed MCP roots in [`SessionRootsRegistry::get`]. Because this resolver
//! reads roots exclusively through `get`, a pinned root flows through Tier 1
//! unchanged — an exact binding match, else the Space default — with no extra
//! tier or parameter. This is the deterministic path for clients that don't
//! report `roots` reliably (e.g. Cursor multiplexing one MCP host across
//! windows): the header always wins over a stale or absent reported root.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use mcpmux_core::{
    FeatureSetRepository, SpaceBaseDirRepository, SpaceRepository, WorkspaceBindingRepository,
};
use mcpmux_storage::InboundClientRepository;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use super::session_roots::SessionRootsRegistry;

/// How long a session that's declared (or might declare) the `roots`
/// capability is held at [`ResolutionSource::PendingRoots`] before the
/// resolver gives up waiting and falls back to the Space default. Sized to
/// comfortably outlast a well-behaved client's `initialize` →
/// `roots/list` round-trip (typically sub-second) so the grace only ever
/// catches clients that declared `roots` but never actually report one.
const DEFAULT_PENDING_ROOTS_GRACE: Duration = Duration::from_secs(5);

/// Why the resolver picked the FS(es) it picked (or didn't pick any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    /// A [`WorkspaceBinding`](mcpmux_core::WorkspaceBinding) matched one of
    /// the session's reported MCP roots.
    WorkspaceBinding,
    /// No binding matched, but the client is roots-capable so its `roots`
    /// list is in flight; return empty and re-resolve when they arrive.
    PendingRoots,
    /// Rootless-by-design client. The space-default's per-client
    /// `client_grants` were applied.
    ClientGrant,
    /// Fell back to the default Space's Starter FeatureSet because nothing
    /// more specific resolved — an unmapped folder, a rootless client with
    /// no grants, or a roots-capable client that never reported a folder.
    /// For the unmapped-folder subcase the upstream caller still emits
    /// `WorkspaceNeedsBinding` so the user can attach an explicit mapping.
    SpaceDefault,
    /// No FeatureSet resolved at all. Defensive: reached only when there's no
    /// default Space, or — degenerately — the default Space somehow has no
    /// Starter FeatureSet. The Starter is builtin and seeded with every Space,
    /// so this is normally unreachable; to grant nothing by default the user
    /// empties the Starter (still `SpaceDefault`, just with no members).
    Deny,
}

/// Output of [`FeatureSetResolverService::resolve`].
///
/// `feature_set_ids` is empty when the resolution was a deny. Multiple ids
/// are possible only on the `ClientGrant` path — bindings always resolve to
/// exactly one FS.
#[derive(Debug, Clone)]
pub struct ResolvedFeatureSet {
    pub feature_set_ids: Vec<String>,
    /// Resolved Space id. Used by the routing layer when filtering features.
    pub space_id: Option<Uuid>,
    pub source: ResolutionSource,
    /// When `source == Deny` because a global binding was blocked by another
    /// client's scoped binding on the same path, holds that client's id.
    pub collision_client_id: Option<String>,
}

impl ResolvedFeatureSet {
    /// Stable key for change detection (sorted + comma-joined). Used by
    /// `SessionRootsRegistry::record_resolution` to decide when a session's
    /// effective tools changed and a per-peer `list_changed` is owed.
    pub fn fingerprint(&self) -> Option<String> {
        if self.feature_set_ids.is_empty() {
            return None;
        }
        let mut ids = self.feature_set_ids.clone();
        ids.sort();
        Some(ids.join(","))
    }
}

/// Resolves which FeatureSet(s) apply for a given session.
///
/// Cheap to clone via `Arc`; inject one instance into the gateway's service
/// container and reuse across requests.
pub struct FeatureSetResolverService {
    space_repo: Arc<dyn SpaceRepository>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    session_roots: Arc<SessionRootsRegistry>,
    /// Reads `client_grants` for the rootless Tier-2 fallback. Stored as a
    /// concrete repo (storage owns this type and there's only ever one).
    client_repo: Arc<InboundClientRepository>,
    /// Looks up each Space's Starter FeatureSet for the default fallback
    /// (Tier 1b / Tier 1c-after-grace / Tier 3).
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    /// Scopes an unmapped reported root to a Space by base directory — an
    /// unmapped folder under a Space's base dir falls back to that Space's
    /// Starter instead of the global default Space.
    space_base_dir_repo: Arc<dyn SpaceBaseDirRepository>,
    /// Grace window for the `PendingRoots` tier — see
    /// [`DEFAULT_PENDING_ROOTS_GRACE`]. Configurable so tests can force the
    /// post-grace path deterministically without sleeping.
    pending_grace: Duration,
    /// This install's registered machine id (`gateway.local_machine_id`).
    /// When set, Tier 1 prefers machine-scoped bindings before global fallbacks.
    local_machine_id: Arc<RwLock<Option<Uuid>>>,
}

impl FeatureSetResolverService {
    pub fn new(
        space_repo: Arc<dyn SpaceRepository>,
        binding_repo: Arc<dyn WorkspaceBindingRepository>,
        session_roots: Arc<SessionRootsRegistry>,
        client_repo: Arc<InboundClientRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        space_base_dir_repo: Arc<dyn SpaceBaseDirRepository>,
        local_machine_id: Option<Uuid>,
    ) -> Self {
        Self {
            space_repo,
            binding_repo,
            session_roots,
            client_repo,
            feature_set_repo,
            space_base_dir_repo,
            pending_grace: DEFAULT_PENDING_ROOTS_GRACE,
            local_machine_id: Arc::new(RwLock::new(local_machine_id)),
        }
    }

    /// Hot-reload this install's machine identity without restarting the gateway.
    pub async fn set_local_machine_id(&self, id: Option<Uuid>) {
        *self.local_machine_id.write().await = id;
    }

    /// Tier 1 exact binding lookup: client machine, then local machine, then global.
    async fn find_binding_for_roots(
        &self,
        roots: &[String],
        client_id: Option<&str>,
    ) -> Result<Option<mcpmux_core::WorkspaceBinding>> {
        for root in roots {
            if let Some(cid) = client_id {
                if let Some(client_machine) = self.client_repo.get_machine_id(cid).await? {
                    if let Some(binding) = self
                        .binding_repo
                        .find_exact_for_machine(&client_machine, root, Some(cid))
                        .await?
                    {
                        return Ok(Some(binding));
                    }
                }
            }
            if let Some(local_id) = *self.local_machine_id.read().await {
                if let Some(binding) = self
                    .binding_repo
                    .find_exact_for_machine(&local_id, root, client_id)
                    .await?
                {
                    return Ok(Some(binding));
                }
            }
            if let Some(binding) = self.binding_repo.find_exact_global(root).await? {
                return Ok(Some(binding));
            }
        }
        // ponytail: when this install has no machine identity, preserve the
        // pre-machine exact match (includes client-scoped bindings). Once
        // local_machine_id is set, only machine + global canonical bindings apply.
        if self.local_machine_id.read().await.is_none() {
            return self.binding_repo.find_exact_for_roots(roots).await;
        }
        Ok(None)
    }

    /// The Space that claims one of `roots` by base directory, or `None`. Each
    /// root's longest-prefix match is taken (via the repo); the first reported
    /// root that lands in a Space wins. Used to scope an unmapped folder to its
    /// Space rather than always falling back to the global default.
    async fn space_for_roots(&self, roots: &[String]) -> Result<Option<Uuid>> {
        for r in roots {
            if let Some(space_id) = self.space_base_dir_repo.find_space_for_root(r).await? {
                return Ok(Some(space_id));
            }
        }
        Ok(None)
    }

    /// The Space a session is scoped to by base directory — its reported root
    /// sits under that Space's base dir — or `None` when it isn't base-dir
    /// scoped (no session, no roots, or no matching base dir). The meta-tools
    /// use this to hard-restrict self-optimization to the matched Space.
    pub async fn scoped_space_for_session(&self, session_id: Option<&str>) -> Result<Option<Uuid>> {
        let Some(sid) = session_id else {
            return Ok(None);
        };
        let Some(roots) = self.session_roots.get(sid) else {
            return Ok(None);
        };
        self.space_for_roots(&roots).await
    }

    /// Override the pending-roots grace window. `Duration::ZERO` makes the
    /// resolver skip the wait entirely and fall back to the Space default on
    /// the first pending resolution — used by tests to exercise the
    /// post-grace path without a real delay.
    pub fn with_pending_grace(mut self, grace: Duration) -> Self {
        self.pending_grace = grace;
        self
    }

    /// Fall back to `space_id`'s Starter FeatureSet. `space_id` is the global
    /// default Space for rootless sessions, or a base-dir-scoped Space for an
    /// unmapped folder under that Space's base directory. Returns
    /// [`ResolutionSource::SpaceDefault`] when a Starter exists (the normal
    /// path — it's builtin and seeded per Space), or, defensively,
    /// [`ResolutionSource::Deny`] in the degenerate case where the Space has no
    /// Starter.
    async fn default_fallback(&self, space_id: Uuid) -> Result<ResolvedFeatureSet> {
        if let Some(fs) = self
            .feature_set_repo
            .get_starter_for_space(&space_id.to_string())
            .await?
        {
            debug!(
                %space_id,
                feature_set_id = %fs.id,
                "[FeatureSetResolver] resolved via SpaceDefault (Starter fallback)",
            );
            return Ok(ResolvedFeatureSet {
                feature_set_ids: vec![fs.id],
                space_id: Some(space_id),
                source: ResolutionSource::SpaceDefault,
                collision_client_id: None,
            });
        }
        debug!(
            %space_id,
            "[FeatureSetResolver] no Starter FeatureSet in Space — deny",
        );
        Ok(ResolvedFeatureSet {
            feature_set_ids: vec![],
            space_id: Some(space_id),
            source: ResolutionSource::Deny,
            collision_client_id: None,
        })
    }

    /// Borrow the session-roots registry. The notifier uses this to GC
    /// dead sessions out of the registry when reaping the corresponding
    /// peer entries — keeping both stores in sync.
    pub fn session_roots(&self) -> &Arc<SessionRootsRegistry> {
        &self.session_roots
    }

    /// Resolve the effective (Space, FS list, source) tuple for a session.
    ///
    /// `session_id`: the client's `mcp-session-id` header (or `None` when
    /// the caller is stateless — e.g. desktop UI HTTP path).
    /// `client_id`: the OAuth client identity. Used only for the Tier-2
    /// `client_grants` lookup; ignored for binding-based routing.
    pub async fn resolve(
        &self,
        session_id: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<ResolvedFeatureSet> {
        let default_space_id = match self.space_repo.get_default().await? {
            Some(s) => s.id,
            None => {
                warn!("[FeatureSetResolver] no default space — deny");
                return Ok(ResolvedFeatureSet {
                    feature_set_ids: vec![],
                    space_id: None,
                    source: ResolutionSource::Deny,
                    collision_client_id: None,
                });
            }
        };

        // Tier 1 / 1b / 1c — branches on roots-capable + roots-arrived state.
        if let Some(sid) = session_id {
            let roots = self.session_roots.get(sid);
            // Three distinct states, NOT two:
            //   None      — roots never arrived (still in flight, or never
            //               declared). Eligible for Tier 1c PendingRoots.
            //   Some([])  — roots ARRIVED and the client has no folder open
            //               (Claude Desktop chat, empty VS Code window). This
            //               is a settled "rootless right now" answer — it must
            //               fall through to the Tier-2 grant lookup, NOT hang
            //               in PendingRoots forever. (Bug: conflating this with
            //               None stranded granted clients on meta-tools-only.)
            //   Some([..])— has roots → Tier 1 binding match.
            let roots_arrived = roots.is_some();
            let has_roots = roots.as_ref().is_some_and(|r| !r.is_empty());
            // Three states for capability:
            //   `Some(true)`  — declared roots on initialize
            //   `Some(false)` — explicitly didn't declare; treat as rootless
            //   `None`        — never observed `notifications/initialized`
            //                   for this session yet. Treat as PROBABLY
            //                   capable (most modern MCP clients are) so
            //                   the resolver returns PendingRoots instead
            //                   of falling all the way through to the
            //                   rootless client_grants tier — otherwise a
            //                   tools/list that races on_initialized
            //                   yields "no roots + no grants — deny" and
            //                   the user sees only meta tools until
            //                   reconnect.
            let roots_capable_known = self.session_roots.is_roots_capable(sid);

            // Tier 1: session reported roots — try an EXACT binding match
            // (no ancestor inheritance).
            if has_roots {
                let reported_roots = roots.expect("has_roots implies Some");
                if let Some(binding) = self
                    .find_binding_for_roots(&reported_roots, client_id)
                    .await?
                {
                    debug!(
                        workspace_root = %binding.workspace_root,
                        space_id = %binding.space_id,
                        feature_sets = ?binding.feature_set_ids,
                        "[FeatureSetResolver] resolved via WorkspaceBinding",
                    );
                    return Ok(ResolvedFeatureSet {
                        feature_set_ids: binding.feature_set_ids,
                        space_id: Some(binding.space_id),
                        source: ResolutionSource::WorkspaceBinding,
                        collision_client_id: None,
                    });
                }
                // Tier 1b: had roots, no binding. The folder is unmapped, so
                // fall back to a Starter FS — the folder works immediately
                // instead of getting nothing. Scope it to the Space whose base
                // directory claims the root (longest-prefix), if any; otherwise
                // the global default Space. Upstream still emits
                // WorkspaceNeedsBinding (it prompts on SpaceDefault too) so the
                // user can attach an explicit mapping for something other than
                // the default.
                let target_space = self
                    .space_for_roots(&reported_roots)
                    .await?
                    .unwrap_or(default_space_id);
                debug!(
                    %target_space,
                    scoped_by_base_dir = target_space != default_space_id,
                    "[FeatureSetResolver] roots reported but no binding matched — SpaceDefault",
                );
                return self.default_fallback(target_space).await;
            }

            // Tier 1c: client declared `roots` but none have ARRIVED yet
            // (roots == None), OR we haven't observed `initialize` yet so we
            // don't know either way. Returning PendingRoots (empty) means the
            // first response is empty if the on-demand probe loses the race,
            // but the next request retries via the probe + the on_initialized
            // list_roots task fires `list_changed` once roots actually land.
            //
            // Crucially gated on `!roots_arrived`: if roots already arrived
            // empty (`Some([])`), this is a settled answer — skip PendingRoots
            // and fall through to Tier 2 so a granted-but-folderless client
            // still gets its tools.
            if !roots_arrived && !matches!(roots_capable_known, Some(false)) {
                // Grace window: hold at PendingRoots (empty) while the client
                // still might report a folder, so it resolves straight to
                // that folder's binding (or the default-for-unmapped) instead
                // of flashing the Space default and then flipping. Stamps
                // first-seen on the first pending resolve and measures from
                // there.
                let elapsed = self.session_roots.elapsed_since_first_seen(sid);
                if elapsed < self.pending_grace {
                    debug!(
                        session_id = %sid,
                        capability = ?roots_capable_known,
                        elapsed_ms = elapsed.as_millis(),
                        "[FeatureSetResolver] roots-capable (or unknown), roots pending — empty until they arrive",
                    );
                    return Ok(ResolvedFeatureSet {
                        feature_set_ids: vec![],
                        space_id: Some(default_space_id),
                        source: ResolutionSource::PendingRoots,
                        collision_client_id: None,
                    });
                }
                // Grace lapsed with no root in sight — settle on the Space
                // default rather than stranding the client on meta-tools
                // forever. Go STRAIGHT to the default (not via Tier-2 grants):
                // a roots-capable session must never pick up another client's
                // grants (per-session isolation invariant).
                debug!(
                    session_id = %sid,
                    capability = ?roots_capable_known,
                    "[FeatureSetResolver] pending-roots grace lapsed, no root reported — SpaceDefault",
                );
                return self.default_fallback(default_space_id).await;
            }
        }

        // Tier 2 — rootless-by-design. Either the session declared no
        // `roots` capability, or the caller has no session id at all
        // (the desktop UI's preview HTTP path lands here too). Consult the
        // per-client grant table.
        if let Some(cid) = client_id {
            // Propagate storage errors instead of treating them as "no
            // grants": a transient DB failure must surface as a request
            // error, not a silent deny (which would also record a `None`
            // fingerprint and fire a spurious Deny→Grant flip-notification
            // cycle once the error clears).
            let grants = self
                .client_repo
                .get_grants_for_space(cid, &default_space_id.to_string())
                .await?;
            if !grants.is_empty() {
                debug!(
                    client_id = %cid,
                    space_id = %default_space_id,
                    grant_count = grants.len(),
                    "[FeatureSetResolver] resolved via ClientGrant",
                );
                return Ok(ResolvedFeatureSet {
                    feature_set_ids: grants,
                    space_id: Some(default_space_id),
                    source: ResolutionSource::ClientGrant,
                    collision_client_id: None,
                });
            }
        }

        // Tier 3 — no roots, no grants. Fall back to the Space default so a
        // bare client still gets the Starter tools instead of nothing. The
        // mcpmux_* meta tools are appended unconditionally by the request
        // handler regardless, so the LLM can always self-bind / ask the user
        // for a grant from here.
        debug!(
            space_id = %default_space_id,
            ?client_id,
            "[FeatureSetResolver] no roots + no grants — SpaceDefault",
        );
        self.default_fallback(default_space_id).await
    }
}
