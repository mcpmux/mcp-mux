//! Feature Routing Service - SRP: Qualified name resolution

use anyhow::Result;
use std::sync::Arc;
use tracing::warn;

use crate::services::PrefixCacheService;
use mcpmux_core::{FeatureType, ServerFeatureRepository};

/// Handles qualified name resolution and routing to servers
pub struct FeatureRoutingService {
    feature_repo: Arc<dyn ServerFeatureRepository>,
    prefix_cache: Arc<PrefixCacheService>,
}

impl FeatureRoutingService {
    pub fn new(
        feature_repo: Arc<dyn ServerFeatureRepository>,
        prefix_cache: Arc<PrefixCacheService>,
    ) -> Self {
        Self {
            feature_repo,
            prefix_cache,
        }
    }

    /// Find which server provides a qualified tool or prompt
    ///
    /// Format: `prefix_feature_name` (underscore separator for Cursor compatibility)
    /// Supports: alias_name or server_id_name
    ///
    /// Note: This method does NOT work for resources - use find_server_for_resource_uri instead
    pub async fn find_server_for_qualified_feature(
        &self,
        space_id: &str,
        qualified_name: &str,
        feature_type: FeatureType,
    ) -> Result<Option<(String, String)>> {
        // Use shared resolution logic from PrefixCacheService
        // Format: prefix_feature_name (underscore separator)
        let (server_id, feature_name) = match self
            .prefix_cache
            .resolve_qualified_name(space_id, qualified_name)
            .await
        {
            Some(res) => res,
            None => {
                warn!(
                    "[FeatureRouting] Feature '{}' is not qualified. Must use: prefix_name",
                    qualified_name
                );
                return Ok(None);
            }
        };

        // Verify feature exists
        let features = self
            .feature_repo
            .list_for_server(space_id, &server_id)
            .await?;

        if features.iter().any(|f| {
            f.feature_type == feature_type && f.feature_name == feature_name && f.is_available
        }) {
            return Ok(Some((server_id, feature_name)));
        }

        warn!(
            "[FeatureRouting] {:?} (server={}, name={}) not found",
            feature_type, server_id, feature_name
        );
        Ok(None)
    }

    /// Parse qualified tool name into (server_id, tool_name)
    ///
    /// Qualified name format: "prefix_tool_name" where prefix is alias or server_id
    /// Uses underscore separator for VSCode/Cursor compatibility
    pub async fn parse_qualified_tool_name(
        &self,
        space_id: &str,
        qualified_name: &str,
    ) -> Result<(String, String)> {
        self.prefix_cache
            .resolve_qualified_name(space_id, qualified_name)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Tool name must be qualified with prefix: prefix_tool_name")
            })
    }

    /// Parse qualified prompt name into (server_id, prompt_name)
    ///
    /// Qualified name format: "prefix_prompt_name" where prefix is alias or server_id
    /// Uses underscore separator for VSCode/Cursor compatibility
    pub async fn parse_qualified_prompt_name(
        &self,
        space_id: &str,
        qualified_name: &str,
    ) -> Result<(String, String)> {
        self.prefix_cache
            .resolve_qualified_name(space_id, qualified_name)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Prompt name must be qualified with prefix: prefix_prompt_name")
            })
    }

    /// Find which server provides a resource by its URI
    ///
    /// Resources don't use prefix.name format - they use URIs which are already
    /// namespaced (e.g., instant-domains://tld-categories, file:///path/to/file)
    ///
    /// This method does a direct lookup in the database by URI
    pub async fn find_server_for_resource_uri(
        &self,
        space_id: &str,
        uri: &str,
    ) -> Result<Option<String>> {
        let features = self.feature_repo.list_for_space(space_id).await?;

        // Find resource by URI (feature_name contains the URI for resources)
        let resource = features.iter().find(|f| {
            f.feature_type == FeatureType::Resource && f.feature_name == uri && f.is_available
        });

        Ok(resource.map(|r| r.server_id.clone()))
    }
}
