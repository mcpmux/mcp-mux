//! Space service - business logic for managing spaces

use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::domain::Space;
use crate::repository::{FeatureSetRepository, SpaceRepository};

/// Service for managing Spaces
pub struct SpaceService {
    repository: Arc<dyn SpaceRepository>,
    feature_set_repository: Option<Arc<dyn FeatureSetRepository>>,
}

impl SpaceService {
    /// Create a new SpaceService
    pub fn new(repository: Arc<dyn SpaceRepository>) -> Self {
        Self {
            repository,
            feature_set_repository: None,
        }
    }

    /// Create a new SpaceService with feature set support
    pub fn with_feature_set_repository(
        repository: Arc<dyn SpaceRepository>,
        feature_set_repository: Arc<dyn FeatureSetRepository>,
    ) -> Self {
        Self {
            repository,
            feature_set_repository: Some(feature_set_repository),
        }
    }

    /// List all spaces
    pub async fn list(&self) -> anyhow::Result<Vec<Space>> {
        self.repository.list().await
    }

    /// Get a space by ID
    pub async fn get(&self, id: &Uuid) -> anyhow::Result<Option<Space>> {
        self.repository.get(id).await
    }

    /// Create a new space
    pub async fn create(&self, name: String, icon: Option<String>) -> anyhow::Result<Space> {
        let mut space = Space::new(&name);
        if let Some(icon) = icon {
            space = space.with_icon(icon);
        }

        // If no spaces exist, make this one the default
        let existing = self.repository.list().await?;
        if existing.is_empty() {
            space = space.set_default();
        }

        self.repository.create(&space).await?;

        // Create builtin feature sets for the new space
        if let Some(ref fs_repo) = self.feature_set_repository {
            if let Err(e) = fs_repo
                .ensure_builtin_for_space(&space.id.to_string())
                .await
            {
                tracing::warn!(
                    space_id = %space.id,
                    error = %e,
                    "Failed to create builtin feature sets for new space"
                );
            } else {
                info!(
                    space_id = %space.id,
                    "Created builtin feature sets for new space"
                );
            }
        }

        Ok(space)
    }

    /// Delete a space
    pub async fn delete(&self, id: &Uuid) -> anyhow::Result<()> {
        let space = self.repository.get(id).await?;
        if let Some(space) = space {
            if space.is_default {
                anyhow::bail!("Cannot delete the default space");
            }
        }
        self.repository.delete(id).await
    }

    /// Get the active (default) space
    pub async fn get_active(&self) -> anyhow::Result<Option<Space>> {
        self.repository.get_default().await
    }

    /// Set the active space
    pub async fn set_active(&self, id: &Uuid) -> anyhow::Result<()> {
        self.repository.set_default(id).await
    }
}
