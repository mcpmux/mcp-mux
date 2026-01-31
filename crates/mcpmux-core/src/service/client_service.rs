//! Client Service - manages AI client configuration and grants
//!
//! Handles auto-granting of Default feature set and permission resolution.

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};
use uuid::Uuid;

use crate::repository::{InboundMcpClientRepository, FeatureSetRepository};

/// Service for managing AI clients and their permissions
pub struct ClientService {
    client_repository: Arc<dyn InboundMcpClientRepository>,
    feature_set_repository: Arc<dyn FeatureSetRepository>,
}

impl ClientService {
    /// Create a new client service
    pub fn new(
        client_repository: Arc<dyn InboundMcpClientRepository>,
        feature_set_repository: Arc<dyn FeatureSetRepository>,
    ) -> Self {
        Self {
            client_repository,
            feature_set_repository,
        }
    }

    /// Ensure a client has the Default feature set granted for a space.
    /// This is called when a client first connects to a space.
    pub async fn ensure_default_grant(&self, client_id: &Uuid, space_id: &str) -> Result<bool> {
        // Check if client already has any grants for this space
        if self.client_repository.has_grants_for_space(client_id, space_id).await? {
            return Ok(false); // Already has grants, don't auto-grant
        }

        // Get the Default feature set for this space
        let default_fs = match self.feature_set_repository.get_default_for_space(space_id).await? {
            Some(fs) => fs,
            None => {
                warn!(
                    "Default feature set not found for space {}. Attempting to create.",
                    space_id
                );
                // Try to create builtin feature sets
                self.feature_set_repository.ensure_builtin_for_space(space_id).await?;
                
                // Try again
                match self.feature_set_repository.get_default_for_space(space_id).await? {
                    Some(fs) => fs,
                    None => {
                        anyhow::bail!("Could not find or create Default feature set for space {}", space_id);
                    }
                }
            }
        };

        // Grant the Default feature set
        self.client_repository
            .grant_feature_set(client_id, space_id, &default_fs.id)
            .await?;

        info!(
            client_id = %client_id,
            space_id = %space_id,
            feature_set_id = %default_fs.id,
            "Auto-granted Default feature set to client"
        );

        Ok(true)
    }

    /// Ensure a client has the All feature set granted for a space.
    pub async fn grant_all_features(&self, client_id: &Uuid, space_id: &str) -> Result<()> {
        // Get the All feature set for this space
        let all_fs = match self.feature_set_repository.get_all_for_space(space_id).await? {
            Some(fs) => fs,
            None => {
                // Try to create builtin feature sets
                self.feature_set_repository.ensure_builtin_for_space(space_id).await?;
                
                self.feature_set_repository.get_all_for_space(space_id).await?
                    .ok_or_else(|| anyhow::anyhow!("Could not find All feature set for space {}", space_id))?
            }
        };

        // Grant the All feature set
        self.client_repository
            .grant_feature_set(client_id, space_id, &all_fs.id)
            .await?;

        info!(
            client_id = %client_id,
            space_id = %space_id,
            feature_set_id = %all_fs.id,
            "Granted All feature set to client"
        );

        Ok(())
    }

    /// Get all granted feature set IDs for a client in a space (explicit grants only)
    pub async fn get_granted_feature_sets(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> Result<Vec<String>> {
        self.client_repository
            .get_grants_for_space(client_id, space_id)
            .await
    }

    /// Get effective feature set IDs for a client in a space.
    /// This includes explicit grants PLUS the default feature set for the space.
    /// Returns a deduplicated set (no repetition).
    pub async fn get_effective_grants(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> Result<Vec<String>> {
        // Get explicit grants from DB
        let mut grants = self.client_repository
            .get_grants_for_space(client_id, space_id)
            .await?;

        // Get default feature set for this space
        if let Some(default_fs) = self.feature_set_repository.get_default_for_space(space_id).await? {
            // Add default if not already in grants (set semantics)
            if !grants.contains(&default_fs.id) {
                grants.push(default_fs.id);
            }
        }

        Ok(grants)
    }
}
