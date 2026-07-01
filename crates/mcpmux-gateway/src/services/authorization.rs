//! Authorization Service.
//!
//! Thin adapter over [`FeatureSetResolverService`]. Routing decisions are
//! keyed primarily on session (→ workspace root → binding); `client_id` is
//! consulted only on the rootless Tier-2 fallback (`client_grants` lookup).
//! Two VS Code windows sharing one OAuth identity still route independently
//! because the binding path uses session-reported roots.

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

    /// Resolve the active FeatureSet ids for a session/client pair.
    ///
    /// Returns an empty Vec when resolution denies (no roots + no grants,
    /// or roots reported but no binding matched). The MCP request handler
    /// surfaces this as "no tools" plus its own `WorkspaceNeedsBinding`
    /// nudge for bound-but-unbound roots.
    pub async fn get_client_grants(
        &self,
        client_id: &str,
        _space_id: &Uuid,
        session_id: Option<&str>,
    ) -> Result<Vec<String>> {
        let resolved = self
            .resolver
            .resolve(session_id, Some(client_id), None)
            .await?;
        Ok(resolved.feature_set_ids)
    }

    /// Full resolution metadata — returns (Space, FS list, source) so the
    /// MCP handler can also filter on the resolved Space rather than the
    /// caller-advertised one.
    pub async fn resolve(
        &self,
        session_id: Option<&str>,
        client_id: Option<&str>,
        request_machine_id: Option<Uuid>,
    ) -> Result<ResolvedFeatureSet> {
        self.resolver
            .resolve(session_id, client_id, request_machine_id)
            .await
    }

    /// Does this session/client resolve to any FeatureSet?
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
