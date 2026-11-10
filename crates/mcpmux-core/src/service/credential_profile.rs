//! CredentialProfileService - management and single-point rotation for credential profiles

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::{CredentialProfile, CredentialProfileSummary};
use crate::repository::{CredentialProfileRepository, InstalledServerRepository};

/// Service for managing centralized credential profiles.
pub struct CredentialProfileService {
    profile_repo: Arc<dyn CredentialProfileRepository>,
    installed_server_repo: Arc<dyn InstalledServerRepository>,
}

impl CredentialProfileService {
    /// Create a new CredentialProfileService.
    pub fn new(
        profile_repo: Arc<dyn CredentialProfileRepository>,
        installed_server_repo: Arc<dyn InstalledServerRepository>,
    ) -> Self {
        Self {
            profile_repo,
            installed_server_repo,
        }
    }

    /// List all credential profiles as summaries (with usage count and configured keys).
    pub async fn list_summaries(&self) -> anyhow::Result<Vec<CredentialProfileSummary>> {
        let profiles = self.profile_repo.list().await?;
        let mut summaries = Vec::with_capacity(profiles.len());

        for profile in profiles {
            let usage_count = self.profile_repo.count_usages(&profile.id).await?;
            summaries.push(profile.to_summary(usage_count));
        }

        summaries.sort_by_key(|a| a.name.to_lowercase());
        Ok(summaries)
    }

    /// List credential profile summaries for a specific MCP server definition ID.
    pub async fn list_summaries_for_server(
        &self,
        server_id: &str,
    ) -> anyhow::Result<Vec<CredentialProfileSummary>> {
        let profiles = self.profile_repo.list_for_server(server_id).await?;
        let mut summaries = Vec::with_capacity(profiles.len());

        for profile in profiles {
            let usage_count = self.profile_repo.count_usages(&profile.id).await?;
            summaries.push(profile.to_summary(usage_count));
        }

        summaries.sort_by_key(|a| a.name.to_lowercase());
        Ok(summaries)
    }

    /// Get a full credential profile by ID.
    pub async fn get(&self, id: &Uuid) -> anyhow::Result<Option<CredentialProfile>> {
        self.profile_repo.get(id).await
    }

    /// Create and persist a new credential profile.
    pub async fn create(
        &self,
        name: impl Into<String>,
        server_id: impl Into<String>,
        server_name: Option<String>,
        values: HashMap<String, String>,
    ) -> anyhow::Result<CredentialProfile> {
        let name_str = name.into();
        let name_trimmed = name_str.trim();
        if name_trimmed.is_empty() {
            anyhow::bail!("Credential profile name cannot be empty");
        }

        let server_id_str = server_id.into();
        if server_id_str.trim().is_empty() {
            anyhow::bail!("Server ID cannot be empty");
        }

        let mut profile = CredentialProfile::new(name_trimmed, server_id_str, values);
        if let Some(sn) = server_name {
            profile = profile.with_server_name(sn);
        }

        self.profile_repo.save(&profile).await?;
        Ok(profile)
    }

    /// Update an existing credential profile and propagate rotated secrets to all linked server installations.
    pub async fn update(
        &self,
        id: &Uuid,
        name: impl Into<String>,
        values: HashMap<String, String>,
    ) -> anyhow::Result<CredentialProfile> {
        let existing = self
            .profile_repo
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Credential profile not found: {id}"))?;

        let name_str = name.into();
        let name_trimmed = name_str.trim();
        if name_trimmed.is_empty() {
            anyhow::bail!("Credential profile name cannot be empty");
        }

        let mut updated = existing;
        updated.name = name_trimmed.to_string();
        updated.values = values.clone();
        updated.updated_at = chrono::Utc::now();

        self.profile_repo.save(&updated).await?;

        // Single-point rotation: propagate updated values to all installed servers referencing this profile
        let linked_servers = self
            .installed_server_repo
            .list_by_credential_profile(id)
            .await?;

        for mut server in linked_servers {
            // Merge updated profile values into server's inputs
            for (k, v) in &values {
                server.input_values.insert(k.clone(), v.clone());
            }
            server.updated_at = chrono::Utc::now();
            self.installed_server_repo.update(&server).await?;
        }

        Ok(updated)
    }

    /// Delete a credential profile by ID.
    pub async fn delete(&self, id: &Uuid) -> anyhow::Result<()> {
        self.profile_repo.delete(id).await
    }
}
