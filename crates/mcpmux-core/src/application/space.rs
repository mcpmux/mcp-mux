//! Space Application Service
//!
//! Manages spaces with automatic event emission.

use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::domain::{DomainEvent, Space};
use crate::event_bus::EventSender;
use crate::repository::{FeatureSetRepository, SpaceRepository};

/// Application service for space management
///
/// Wraps space repository operations with event emission.
pub struct SpaceAppService {
    space_repo: Arc<dyn SpaceRepository>,
    feature_set_repo: Option<Arc<dyn FeatureSetRepository>>,
    event_sender: EventSender,
}

impl SpaceAppService {
    pub fn new(
        space_repo: Arc<dyn SpaceRepository>,
        feature_set_repo: Option<Arc<dyn FeatureSetRepository>>,
        event_sender: EventSender,
    ) -> Self {
        Self {
            space_repo,
            feature_set_repo,
            event_sender,
        }
    }

    /// List all spaces
    pub async fn list(&self) -> Result<Vec<Space>> {
        self.space_repo.list().await
    }

    /// Get a space by ID
    pub async fn get(&self, id: Uuid) -> Result<Option<Space>> {
        self.space_repo.get(&id).await
    }

    /// Get the active (default) space
    pub async fn get_active(&self) -> Result<Option<Space>> {
        self.space_repo.get_default().await
    }

    /// Create a new space
    ///
    /// Emits: `SpaceCreated`
    pub async fn create(&self, name: &str, icon: Option<String>) -> Result<Space> {
        let mut space = Space::new(name);
        if let Some(icon) = &icon {
            space = space.with_icon(icon);
        }

        // If no spaces exist, make this one the default
        let existing = self.space_repo.list().await?;
        if existing.is_empty() {
            space = space.set_default();
        }

        // Persist
        self.space_repo.create(&space).await?;

        // Create builtin feature sets
        if let Some(ref fs_repo) = self.feature_set_repo {
            if let Err(e) = fs_repo
                .ensure_builtin_for_space(&space.id.to_string())
                .await
            {
                tracing::warn!(
                    space_id = %space.id,
                    error = %e,
                    "Failed to create builtin feature sets for new space"
                );
            }
        }

        info!(space_id = %space.id, name = %space.name, "[SpaceAppService] Created space");

        // Emit event
        self.event_sender.emit(DomainEvent::SpaceCreated {
            space_id: space.id,
            name: space.name.clone(),
            icon: space.icon.clone(),
        });

        Ok(space)
    }

    /// Update a space
    ///
    /// Emits: `SpaceUpdated`
    pub async fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        icon: Option<String>,
        description: Option<String>,
    ) -> Result<Space> {
        let mut space = self
            .space_repo
            .get(&id)
            .await?
            .ok_or_else(|| anyhow!("Space not found"))?;

        if let Some(name) = name {
            space.name = name;
        }
        if let Some(icon) = icon {
            space.icon = Some(icon);
        }
        if let Some(description) = description {
            space.description = Some(description);
        }
        space.updated_at = chrono::Utc::now();

        self.space_repo.update(&space).await?;

        info!(space_id = %space.id, name = %space.name, "[SpaceAppService] Updated space");

        // Emit event
        self.event_sender.emit(DomainEvent::SpaceUpdated {
            space_id: space.id,
            name: space.name.clone(),
        });

        Ok(space)
    }

    /// Delete a space
    ///
    /// Emits: `SpaceDeleted`
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let space = self
            .space_repo
            .get(&id)
            .await?
            .ok_or_else(|| anyhow!("Space not found"))?;

        if space.is_default {
            return Err(anyhow!("Cannot delete the default space"));
        }

        self.space_repo.delete(&id).await?;

        info!(space_id = %id, "[SpaceAppService] Deleted space");

        // Emit event
        self.event_sender
            .emit(DomainEvent::SpaceDeleted { space_id: id });

        Ok(())
    }

    /// Set the active space
    ///
    /// Emits: `SpaceActivated`
    pub async fn set_active(&self, id: Uuid) -> Result<Space> {
        // Get current active space
        let old_space = self.space_repo.get_default().await?;

        // Get new space
        let new_space = self
            .space_repo
            .get(&id)
            .await?
            .ok_or_else(|| anyhow!("Space not found"))?;

        // Set as default
        self.space_repo.set_default(&id).await?;

        info!(
            space_id = %id,
            name = %new_space.name,
            "[SpaceAppService] Activated space"
        );

        // Emit event
        self.event_sender.emit(DomainEvent::SpaceActivated {
            from_space_id: old_space.map(|s| s.id),
            to_space_id: new_space.id,
            to_space_name: new_space.name.clone(),
        });

        Ok(new_space)
    }
}
