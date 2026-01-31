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
    /// Uses layered resolution: Returns explicit grants PLUS the default feature set
    /// (deduplicated as a set). This ensures all clients always get default permissions.
    /// 
    /// Returns Vec of feature_set_ids that the client has access to.
    pub async fn get_client_grants(
        &self,
        client_id: &str,
        space_id: &Uuid,
    ) -> Result<Vec<String>> {
        let space_id_str = space_id.to_string();
        
        // Get explicit grants from DB
        let mut grants = self
            .client_repo
            .get_grants_for_space(client_id, &space_id_str)
            .await?;
        
        // Add default feature set (layered resolution)
        if let Some(default_fs) = self.feature_set_repo.get_default_for_space(&space_id_str).await? {
            // Add default if not already in grants (set semantics - no repetition)
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

