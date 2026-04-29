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
            let has_roots = roots.as_ref().is_some_and(|r| !r.is_empty());
            let roots_capable = self.session_roots.is_roots_capable(sid).unwrap_or(false);

            // Tier 1: session reported roots — try a binding match.
            if has_roots {
                if let Some(binding) = self
                    .binding_repo
                    .find_longest_prefix_match(&default_space_id, &roots.unwrap())
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

            // Tier 1c: client declared `roots` but they haven't shown up yet.
            // Don't fall through to client grants — that's the leak the old
            // Tier-2 fallback caused. Return empty; we'll fire `list_changed`
            // when roots actually arrive.
            if roots_capable {
                debug!(
                    session_id = %sid,
                    "[FeatureSetResolver] roots-capable, roots pending — empty until they arrive",
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
            let grants = self
                .client_repo
                .get_grants_for_space(cid, &default_space_id.to_string())
                .await
                .unwrap_or_default();
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

#[cfg(test)]
mod tests {
    //! Resolver decision-table tests live in the integration test crate
    //! (`tests/rust/tests/integration/feature_set_resolver.rs`) so they can
    //! share the mock repositories with the other gateway tests.
}
