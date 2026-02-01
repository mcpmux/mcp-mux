//! Feature Discovery Service - SRP: Discovery & caching

use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::{convert_to_feature, resource_to_feature, CachedFeatures};
use crate::pool::instance::McpClient;
use mcpmux_core::{FeatureSetRepository, ServerFeatureRepository};

/// Handles feature discovery and caching from MCP clients
pub struct FeatureDiscoveryService {
    feature_repo: Arc<dyn ServerFeatureRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
}

impl FeatureDiscoveryService {
    pub fn new(
        feature_repo: Arc<dyn ServerFeatureRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
    ) -> Self {
        Self {
            feature_repo,
            feature_set_repo,
        }
    }

    /// Discover features from a connected MCP client and cache them
    pub async fn discover_and_cache(
        &self,
        space_id: &str,
        server_id: &str,
        client: &McpClient,
    ) -> Result<CachedFeatures> {
        info!(
            "[FeatureDiscovery] Discovering features for {}/{}",
            space_id, server_id
        );

        let mut discovered = CachedFeatures::default();

        // Discover tools
        match client.list_all_tools().await {
            Ok(tools) => {
                discovered.tools = tools
                    .into_iter()
                    .map(|t| convert_to_feature(space_id, server_id, t))
                    .collect();
                debug!(
                    "[FeatureDiscovery] Discovered {} tools",
                    discovered.tools.len()
                );
            }
            Err(e) => warn!("[FeatureDiscovery] Failed to list tools: {}", e),
        }

        // Discover prompts
        match client.list_all_prompts().await {
            Ok(prompts) => {
                discovered.prompts = prompts
                    .into_iter()
                    .map(|p| convert_to_feature(space_id, server_id, p))
                    .collect();
                debug!(
                    "[FeatureDiscovery] Discovered {} prompts",
                    discovered.prompts.len()
                );
            }
            Err(e) => warn!("[FeatureDiscovery] Failed to list prompts: {}", e),
        }

        // Discover resources
        match client.list_all_resources().await {
            Ok(resources) => {
                discovered.resources = resources
                    .into_iter()
                    .map(|r| resource_to_feature(space_id, server_id, r))
                    .collect();
                debug!(
                    "[FeatureDiscovery] Discovered {} resources",
                    discovered.resources.len()
                );
            }
            Err(e) => warn!("[FeatureDiscovery] Failed to list resources: {}", e),
        }

        // Cache all features in database
        let all_features = discovered.all_features();
        if !all_features.is_empty() {
            if let Err(e) = self.feature_repo.upsert_many(&all_features).await {
                warn!("[FeatureDiscovery] Failed to cache features: {}", e);
            } else {
                info!(
                    "[FeatureDiscovery] Cached {} features for {}/{}",
                    all_features.len(),
                    space_id,
                    server_id
                );
            }
        }

        // Ensure server-all featureset exists
        if let Err(e) = self
            .feature_set_repo
            .ensure_server_all(space_id, server_id, server_id)
            .await
        {
            warn!(
                "[FeatureDiscovery] Failed to ensure server-all featureset: {}",
                e
            );
        }

        Ok(discovered)
    }

    /// Mark all features for a server as unavailable (on disconnect)
    pub async fn mark_unavailable(&self, space_id: &str, server_id: &str) -> Result<()> {
        self.feature_repo
            .mark_unavailable(space_id, server_id)
            .await
    }

    /// Delete all features for a server (on uninstall)
    pub async fn delete_for_server(&self, space_id: &str, server_id: &str) -> Result<()> {
        self.feature_repo
            .delete_for_server(space_id, server_id)
            .await
    }
}
