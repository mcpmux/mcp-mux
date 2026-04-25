//! FeatureSet Resolver Service.
//!
//! Two-tier resolution, keyed by the caller's reported workspace roots:
//!
//! ```text
//! resolve(session_id):
//!     if session reported roots AND a binding matches:
//!         return (binding.space_id, binding.feature_set_id, WorkspaceBinding)
//!     default_space = SpaceRepository.get_default()
//!     return (default_space.id, default_space's Default FS id, Default)
//! ```
//!
//! The caller's client identity is deliberately NOT used for routing — two
//! VS Code windows share one OAuth identity but open different folders;
//! routing must come from the session's reported root, not from the shared
//! client. See `mcpmux.space/diagrams/workppace-root-session/` for the full
//! design.

use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::{FeatureSetRepository, SpaceRepository, WorkspaceBindingRepository};
use serde::Serialize;
use tracing::{debug, warn};
use uuid::Uuid;

use super::session_roots::SessionRootsRegistry;

/// Why the resolver picked the FS it picked (or didn't pick one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    /// A [`WorkspaceBinding`](mcpmux_core::WorkspaceBinding) matched one of
    /// the session's reported MCP roots.
    WorkspaceBinding,
    /// No binding matched (or the session reported no roots); fell through
    /// to the default Space's Default FeatureSet.
    Default,
}

/// Output of [`FeatureSetResolverService::resolve`].
///
/// `feature_set_id` is a `String` (not `Uuid`) because built-in FeatureSets
/// use stable stringy ids like `fs_default_<space>` that aren't valid UUIDs.
#[derive(Debug, Clone)]
pub struct ResolvedFeatureSet {
    /// Chosen FeatureSet id. `None` only when every fallback tier failed
    /// (no default space, no Default FS in that space — a pathological setup).
    pub feature_set_id: Option<String>,
    /// Resolved Space id. Used by the routing layer when filtering features.
    pub space_id: Option<Uuid>,
    pub source: ResolutionSource,
}

/// Resolves which FeatureSet applies for a given session.
///
/// Cheap to clone via `Arc`; inject one instance into the gateway's service
/// container and reuse across requests.
pub struct FeatureSetResolverService {
    space_repo: Arc<dyn SpaceRepository>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    session_roots: Arc<SessionRootsRegistry>,
}

impl FeatureSetResolverService {
    pub fn new(
        space_repo: Arc<dyn SpaceRepository>,
        binding_repo: Arc<dyn WorkspaceBindingRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        session_roots: Arc<SessionRootsRegistry>,
    ) -> Self {
        Self {
            space_repo,
            binding_repo,
            feature_set_repo,
            session_roots,
        }
    }

    /// Resolve the effective (Space, FeatureSet) pair for a session.
    ///
    /// `session_id` is the client's `mcp-session-id` header (or `None` for
    /// stateless callers) — used to look up MCP roots reported on
    /// `on_initialized`.
    pub async fn resolve(&self, session_id: Option<&str>) -> Result<ResolvedFeatureSet> {
        let default_space_id = match self.space_repo.get_default().await? {
            Some(s) => s.id,
            None => {
                warn!("[FeatureSetResolver] no default space — deny");
                return Ok(ResolvedFeatureSet {
                    feature_set_id: None,
                    space_id: None,
                    source: ResolutionSource::Default,
                });
            }
        };

        // Tier 1: session has roots AND a binding matches.
        if let Some(sid) = session_id {
            if let Some(roots) = self.session_roots.get(sid) {
                if !roots.is_empty() {
                    if let Some(binding) = self
                        .binding_repo
                        .find_longest_prefix_match(&default_space_id, &roots)
                        .await?
                    {
                        debug!(
                            workspace_root = %binding.workspace_root,
                            space_id = %binding.space_id,
                            feature_set = %binding.feature_set_id,
                            "[FeatureSetResolver] resolved via WorkspaceBinding",
                        );
                        return Ok(ResolvedFeatureSet {
                            feature_set_id: Some(binding.feature_set_id),
                            space_id: Some(binding.space_id),
                            source: ResolutionSource::WorkspaceBinding,
                        });
                    }
                }
            }
        }

        // Tier 2: default — the default Space's seeded Default FS.
        let default_fs = self
            .feature_set_repo
            .get_default_for_space(&default_space_id.to_string())
            .await
            .unwrap_or_default()
            .map(|fs| fs.id);

        debug!(
            space_id = %default_space_id,
            feature_set = ?default_fs,
            "[FeatureSetResolver] resolved via Default (default space's Default FS)",
        );

        Ok(ResolvedFeatureSet {
            feature_set_id: default_fs,
            space_id: Some(default_space_id),
            source: ResolutionSource::Default,
        })
    }
}

#[cfg(test)]
mod tests {
    //! Resolver decision-table tests live in the integration test crate
    //! (`tests/rust/tests/integration/feature_set_resolver.rs`) so they can
    //! share the mock repositories with the other gateway tests.
}
