//! Permission Application Service
//!
//! Manages feature sets and grants with automatic event emission.

use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::domain::{DomainEvent, FeatureSet, FeatureSetMember, MemberMode};
use crate::event_bus::EventSender;
use crate::repository::FeatureSetRepository;

/// Application service for feature sets.
///
/// Grants no longer exist — routing is driven by WorkspaceBinding and each
/// Space's Default feature set. This service therefore only covers FS
/// creation, edits, and membership.
pub struct PermissionAppService {
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    event_sender: EventSender,
}

impl PermissionAppService {
    pub fn new(feature_set_repo: Arc<dyn FeatureSetRepository>, event_sender: EventSender) -> Self {
        Self {
            feature_set_repo,
            event_sender,
        }
    }

    // ========================================================================
    // FEATURE SET OPERATIONS
    // ========================================================================

    /// List all feature sets
    pub async fn list_feature_sets(&self) -> Result<Vec<FeatureSet>> {
        self.feature_set_repo.list().await
    }

    /// List feature sets for a space
    pub async fn list_feature_sets_for_space(&self, space_id: &str) -> Result<Vec<FeatureSet>> {
        self.feature_set_repo.list_by_space(space_id).await
    }

    /// Get a feature set with its members
    pub async fn get_feature_set(&self, id: &str) -> Result<Option<FeatureSet>> {
        self.feature_set_repo.get_with_members(id).await
    }

    /// Create a feature set
    ///
    /// Emits: `FeatureSetCreated`
    pub async fn create_feature_set(
        &self,
        space_id: &str,
        name: &str,
        description: Option<String>,
        icon: Option<String>,
    ) -> Result<FeatureSet> {
        let mut feature_set = FeatureSet::new_custom(name, space_id);

        if let Some(desc) = description {
            feature_set = feature_set.with_description(desc);
        }
        if let Some(ic) = icon {
            feature_set = feature_set.with_icon(ic);
        }

        self.feature_set_repo.create(&feature_set).await?;

        info!(
            feature_set_id = %feature_set.id,
            space_id = space_id,
            name = name,
            "[PermissionAppService] Created feature set"
        );

        // Parse space_id to UUID
        let space_uuid =
            Uuid::parse_str(space_id).map_err(|e| anyhow!("Invalid space ID: {}", e))?;

        // Emit event
        self.event_sender.emit(DomainEvent::FeatureSetCreated {
            space_id: space_uuid,
            feature_set_id: feature_set.id.clone(),
            name: feature_set.name.clone(),
            feature_set_type: None, // Custom set, not builtin
        });

        Ok(feature_set)
    }

    /// Update a feature set
    ///
    /// Emits: `FeatureSetUpdated`
    pub async fn update_feature_set(
        &self,
        id: &str,
        name: Option<String>,
        description: Option<String>,
        icon: Option<String>,
    ) -> Result<FeatureSet> {
        let mut feature_set = self
            .feature_set_repo
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("Feature set not found"))?;

        if let Some(name) = name {
            feature_set.name = name;
        }
        if let Some(description) = description {
            feature_set.description = Some(description);
        }
        if let Some(icon) = icon {
            feature_set.icon = Some(icon);
        }
        feature_set.updated_at = chrono::Utc::now();

        self.feature_set_repo.update(&feature_set).await?;

        // Parse space_id to UUID (feature_set.space_id is Option<String>)
        let space_uuid = feature_set
            .space_id
            .as_ref()
            .ok_or_else(|| anyhow!("Feature set has no space_id"))?;
        let space_uuid =
            Uuid::parse_str(space_uuid).map_err(|e| anyhow!("Invalid space ID: {}", e))?;

        info!(
            feature_set_id = %feature_set.id,
            "[PermissionAppService] Updated feature set"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::FeatureSetUpdated {
            space_id: space_uuid,
            feature_set_id: feature_set.id.clone(),
            name: feature_set.name.clone(),
        });

        Ok(feature_set)
    }

    /// Delete a feature set
    ///
    /// Emits: `FeatureSetDeleted`
    pub async fn delete_feature_set(&self, id: &str) -> Result<()> {
        let feature_set = self
            .feature_set_repo
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("Feature set not found"))?;

        // Don't allow deleting builtin sets
        if feature_set.is_builtin {
            return Err(anyhow!("Cannot delete builtin feature sets"));
        }

        // Parse space_id to UUID
        let space_uuid = feature_set
            .space_id
            .as_ref()
            .ok_or_else(|| anyhow!("Feature set has no space_id"))?;
        let space_uuid =
            Uuid::parse_str(space_uuid).map_err(|e| anyhow!("Invalid space ID: {}", e))?;

        self.feature_set_repo.delete(id).await?;

        info!(
            feature_set_id = id,
            "[PermissionAppService] Deleted feature set"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::FeatureSetDeleted {
            space_id: space_uuid,
            feature_set_id: id.to_string(),
        });

        Ok(())
    }

    // ========================================================================
    // FEATURE SET MEMBER OPERATIONS
    // ========================================================================

    /// Add a feature to a feature set
    ///
    /// Emits: `FeatureSetMembersChanged`
    pub async fn add_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
        mode: MemberMode,
    ) -> Result<()> {
        let feature_set = self
            .feature_set_repo
            .get(feature_set_id)
            .await?
            .ok_or_else(|| anyhow!("Feature set not found"))?;

        self.feature_set_repo
            .add_feature_member(feature_set_id, feature_id, mode)
            .await?;

        // Parse space_id to UUID
        let space_uuid = feature_set
            .space_id
            .as_ref()
            .ok_or_else(|| anyhow!("Feature set has no space_id"))?;
        let space_uuid =
            Uuid::parse_str(space_uuid).map_err(|e| anyhow!("Invalid space ID: {}", e))?;

        info!(
            feature_set_id = feature_set_id,
            feature_id = feature_id,
            "[PermissionAppService] Added feature to set"
        );

        // Emit event
        self.event_sender
            .emit(DomainEvent::FeatureSetMembersChanged {
                space_id: space_uuid,
                feature_set_id: feature_set_id.to_string(),
                added_count: 1,
                removed_count: 0,
            });

        Ok(())
    }

    /// Remove a feature from a feature set
    ///
    /// Emits: `FeatureSetMembersChanged`
    pub async fn remove_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
    ) -> Result<()> {
        let feature_set = self
            .feature_set_repo
            .get(feature_set_id)
            .await?
            .ok_or_else(|| anyhow!("Feature set not found"))?;

        self.feature_set_repo
            .remove_feature_member(feature_set_id, feature_id)
            .await?;

        // Parse space_id to UUID
        let space_uuid = feature_set
            .space_id
            .as_ref()
            .ok_or_else(|| anyhow!("Feature set has no space_id"))?;
        let space_uuid =
            Uuid::parse_str(space_uuid).map_err(|e| anyhow!("Invalid space ID: {}", e))?;

        info!(
            feature_set_id = feature_set_id,
            feature_id = feature_id,
            "[PermissionAppService] Removed feature from set"
        );

        // Emit event
        self.event_sender
            .emit(DomainEvent::FeatureSetMembersChanged {
                space_id: space_uuid,
                feature_set_id: feature_set_id.to_string(),
                added_count: 0,
                removed_count: 1,
            });

        Ok(())
    }

    /// Get members of a feature set
    pub async fn get_feature_members(&self, feature_set_id: &str) -> Result<Vec<FeatureSetMember>> {
        self.feature_set_repo
            .get_feature_members(feature_set_id)
            .await
    }
}
