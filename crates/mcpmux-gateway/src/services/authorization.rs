//! Authorization Service
//!
//! Responsible for checking client permissions (grants) for accessing features.
//! Follows SRP: Single responsibility is authorization checking.
//! Follows DIP: Depends on repository abstractions, not concrete implementations.

use anyhow::Result;
use mcpmux_core::FeatureSetRepository;
use mcpmux_storage::InboundClientRepository;
use std::sync::Arc;
use uuid::Uuid;

/// Authorization service for checking client permissions
///
/// SRP: Only handles authorization decisions
/// DIP: Depends on repository abstractions
pub struct AuthorizationService {
    client_repo: Arc<InboundClientRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
}

impl AuthorizationService {
    pub fn new(
        client_repo: Arc<InboundClientRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
    ) -> Self {
        Self {
            client_repo,
            feature_set_repo,
        }
    }

    /// Get effective feature set grants for a client in a specific space.
    ///
    /// Resolution strategy (least-privilege by default):
    /// 1. Return explicit per-client grants from DB if any exist.
    /// 2. Always include the Default feature set as a baseline.
    ///
    /// Clients with no explicit grants only receive the Default feature set,
    /// which starts empty (no features). The user must explicitly grant
    /// additional feature sets (e.g. "All", "ServerAll", or custom sets)
    /// through the UI to expose tools/prompts/resources to a client.
    /// This avoids accidental exposure of all server capabilities.
    pub async fn get_client_grants(&self, client_id: &str, space_id: &Uuid) -> Result<Vec<String>> {
        let space_id_str = space_id.to_string();

        // Get explicit grants from DB
        let mut grants = self
            .client_repo
            .get_grants_for_space(client_id, &space_id_str)
            .await?;

        // Always include the Default feature set as baseline permissions.
        // Default starts empty — user must explicitly grant additional access.
        if let Some(default_fs) = self
            .feature_set_repo
            .get_default_for_space(&space_id_str)
            .await?
        {
            if !grants.contains(&default_fs.id) {
                grants.push(default_fs.id);
            }
        }

        Ok(grants)
    }

    /// Check if a client has any grants in a space
    pub async fn has_access(&self, client_id: &str, space_id: &Uuid) -> Result<bool> {
        let grants = self.get_client_grants(client_id, space_id).await?;
        Ok(!grants.is_empty())
    }

    /// Check if a client has access to a specific feature set
    pub async fn has_feature_set_access(
        &self,
        client_id: &str,
        space_id: &Uuid,
        feature_set_id: &str,
    ) -> Result<bool> {
        let grants = self.get_client_grants(client_id, space_id).await?;
        Ok(grants.contains(&feature_set_id.to_string()))
    }
}
