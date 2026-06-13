//! FeatureSet Resolver Service.
//!
//! Capability-branched four-tier resolution. The branch point is the MCP
//! `roots` capability declared by the client at `initialize`:
//!
//! ```text
//! resolve(session_id, client_id):
//!     // Tier 1 — roots-capable session with reported roots
//!     if session reported roots AND a binding matches:
//!         return (binding.space_id, [binding.feature_set_id], WorkspaceBinding)
//!
//!     // Tier 1b — roots-capable, roots reported, but no binding yet
//!     if session reported roots AND no binding matched:
//!         return ([], <space>, Deny)   // emits WorkspaceNeedsBinding upstream
//!
//!     // Tier 1c — declared `roots` but they haven't arrived yet
//!     if session declared `roots` AND none yet in registry:
//!         return ([], default_space, PendingRoots)
//!
//!     // Tier 2 — rootless-by-design (Claude.ai web, ChatGPT, …)
//!     if client has grants in the default space:
//!         return (default_space, grants, ClientGrant)
//!
//!     // Tier 3 — no signal at all
//!     return ([], default_space, Deny)
//! ```
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

use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::{SpaceRepository, WorkspaceBindingRepository};
use mcpmux_storage::InboundClientRepository;
use serde::Serialize;
use tracing::{debug, warn};
use uuid::Uuid;

use super::session_roots::SessionRootsRegistry;

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
    /// No FeatureSet resolved. Either no roots + no grants, or the session
    /// reported roots but no binding matched (the upstream caller emits
    /// `WorkspaceNeedsBinding` in that subcase).
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
}

impl FeatureSetResolverService {
    pub fn new(
        space_repo: Arc<dyn SpaceRepository>,
        binding_repo: Arc<dyn WorkspaceBindingRepository>,
        session_roots: Arc<SessionRootsRegistry>,
        client_repo: Arc<InboundClientRepository>,
    ) -> Self {
        Self {
            space_repo,
            binding_repo,
            session_roots,
            client_repo,
        }
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
                if let Some(binding) = self
                    .binding_repo
                    .find_exact_for_roots(&roots.unwrap())
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
                    });
                }
                // Tier 1b: had roots, no binding — deny + upstream emits
                // WorkspaceNeedsBinding so the user can choose an FS.
                debug!("[FeatureSetResolver] roots reported but no binding matched — deny",);
                return Ok(ResolvedFeatureSet {
                    feature_set_ids: vec![],
                    space_id: Some(default_space_id),
                    source: ResolutionSource::Deny,
                });
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
                debug!(
                    session_id = %sid,
                    capability = ?roots_capable_known,
                    "[FeatureSetResolver] roots-capable (or unknown), roots pending — empty until they arrive",
                );
                return Ok(ResolvedFeatureSet {
                    feature_set_ids: vec![],
                    space_id: Some(default_space_id),
                    source: ResolutionSource::PendingRoots,
                });
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
                });
            }
        }

        // Tier 3 — no roots, no grants. Deny.
        // The mcpmux_* meta tools are still appended unconditionally by the
        // request handler, so the LLM can self-bind / ask the user for
        // a grant from this state.
        debug!(
            space_id = %default_space_id,
            ?client_id,
            "[FeatureSetResolver] no roots + no grants — deny",
        );

        Ok(ResolvedFeatureSet {
            feature_set_ids: vec![],
            space_id: Some(default_space_id),
            source: ResolutionSource::Deny,
        })
    }
}
