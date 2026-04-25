//! Authorization Service.
//!
//! Thin adapter over [`FeatureSetResolverService`]. Routing decisions are
//! keyed purely on session (→ workspace root → binding); client_id is only
//! used for approval (upstream of this service), never for routing. That's
//! what fixes the "two VS Code windows share a pin" bug — a single client
//! can have many sessions, each routing independently.

use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

use super::feature_set_resolver::{FeatureSetResolverService, ResolvedFeatureSet};

pub struct AuthorizationService {
    resolver: Arc<FeatureSetResolverService>,
}

impl AuthorizationService {
    pub fn new(resolver: Arc<FeatureSetResolverService>) -> Self {
        Self { resolver }
    }

    /// Resolve the active FeatureSet for a session and return it as a
    /// one-element Vec (or empty when resolution fully fails — no active
    /// space + no "All" FS, a pathological setup).
    ///
    /// `session_id` is the client's `mcp-session-id` header. `client_id`
    /// and `space_id` are ignored — they come from legacy call sites and
    /// are not used by the new resolver.
    pub async fn get_client_grants(
        &self,
        _client_id: &str,
        _space_id: &Uuid,
        session_id: Option<&str>,
    ) -> Result<Vec<String>> {
        let resolved = self.resolver.resolve(session_id).await?;
        Ok(resolved
            .feature_set_id
            .map(|fs| vec![fs])
            .unwrap_or_default())
    }

    /// Full resolution metadata — returns (Space, FS, source) so the MCP
    /// handler can also filter on the resolved Space rather than the
    /// caller-advertised one.
    pub async fn resolve(&self, session_id: Option<&str>) -> Result<ResolvedFeatureSet> {
        self.resolver.resolve(session_id).await
    }

    /// Does this session resolve to any FeatureSet?
    pub async fn has_access(
        &self,
        client_id: &str,
        space_id: &Uuid,
        session_id: Option<&str>,
    ) -> Result<bool> {
        let grants = self
            .get_client_grants(client_id, space_id, session_id)
            .await?;
        Ok(!grants.is_empty())
    }
}
