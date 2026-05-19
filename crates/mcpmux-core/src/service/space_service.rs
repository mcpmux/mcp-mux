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

    /// Update a space's display metadata (name, icon, description).
    pub async fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        icon: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<Space> {
        let mut space = self
            .repository
            .get(&id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Space not found"))?;

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

        self.repository.update(&space).await?;
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

    /// Get the system's default Space (the gateway's routing fallback when
    /// no `WorkspaceBinding` matches a session's reported workspace root).
    pub async fn get_default(&self) -> anyhow::Result<Option<Space>> {
        self.repository.get_default().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    struct InMemorySpaceRepo {
        spaces: RwLock<HashMap<Uuid, Space>>,
    }

    async fn repo_with_space(space: Space) -> Arc<InMemorySpaceRepo> {
        let repo = Arc::new(InMemorySpaceRepo {
            spaces: RwLock::new(HashMap::new()),
        });
        repo.spaces.write().await.insert(space.id, space);
        repo
    }

    #[async_trait]
    impl SpaceRepository for InMemorySpaceRepo {
        async fn list(&self) -> crate::repository::RepoResult<Vec<Space>> {
            Ok(self.spaces.read().await.values().cloned().collect())
        }

        async fn get(&self, id: &Uuid) -> crate::repository::RepoResult<Option<Space>> {
            Ok(self.spaces.read().await.get(id).cloned())
        }

        async fn create(&self, space: &Space) -> crate::repository::RepoResult<()> {
            self.spaces.write().await.insert(space.id, space.clone());
            Ok(())
        }

        async fn update(&self, space: &Space) -> crate::repository::RepoResult<()> {
            self.spaces.write().await.insert(space.id, space.clone());
            Ok(())
        }

        async fn delete(&self, id: &Uuid) -> crate::repository::RepoResult<()> {
            self.spaces.write().await.remove(id);
            Ok(())
        }

        async fn get_default(&self) -> crate::repository::RepoResult<Option<Space>> {
            Ok(self
                .spaces
                .read()
                .await
                .values()
                .find(|s| s.is_default)
                .cloned())
        }

        async fn set_default(&self, id: &Uuid) -> crate::repository::RepoResult<()> {
            let mut spaces = self.spaces.write().await;
            for space in spaces.values_mut() {
                space.is_default = false;
            }
            if let Some(space) = spaces.get_mut(id) {
                space.is_default = true;
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn update_changes_name_and_bumps_updated_at() {
        let original = Space::new("Original");
        let id = original.id;
        let original_updated_at = original.updated_at;
        let repo = repo_with_space(original).await;
        let service = SpaceService::new(repo);

        let updated = service
            .update(id, Some("Renamed".to_string()), None, None)
            .await
            .unwrap();

        assert_eq!(updated.name, "Renamed");
        assert!(updated.updated_at >= original_updated_at);

        let loaded = service.get(&id).await.unwrap().expect("space exists");
        assert_eq!(loaded.name, "Renamed");
    }

    #[tokio::test]
    async fn update_applies_icon_and_description() {
        let space = Space::new("Space");
        let id = space.id;
        let repo = repo_with_space(space).await;
        let service = SpaceService::new(repo);

        let updated = service
            .update(
                id,
                None,
                Some("rocket".to_string()),
                Some("Side project".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(updated.icon.as_deref(), Some("rocket"));
        assert_eq!(updated.description.as_deref(), Some("Side project"));
    }

    #[tokio::test]
    async fn update_returns_not_found_for_missing_space() {
        let repo = Arc::new(InMemorySpaceRepo {
            spaces: RwLock::new(HashMap::new()),
        });
        let service = SpaceService::new(repo);

        let err = service
            .update(Uuid::new_v4(), Some("nope".to_string()), None, None)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("Space not found"));
    }
}
