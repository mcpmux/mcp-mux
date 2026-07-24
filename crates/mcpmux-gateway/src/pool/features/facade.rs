//! Feature Service Facade - Unified API delegating to specialized services

use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;

use crate::pool::instance::McpClient;
use crate::services::PrefixCacheService;
use mcpmux_core::{FeatureSetRepository, FeatureType, ServerFeature, ServerFeatureRepository};

use super::{
    CachedFeatures, FeatureDiscoveryService, FeatureResolutionService, FeatureRoutingService,
};

pub use super::resolution::InactiveDiscoveryEntry;

/// Unified facade providing all feature operations (Facade pattern)
pub struct FeatureService {
    discovery: Arc<FeatureDiscoveryService>,
    resolution: Arc<FeatureResolutionService>,
    routing: Arc<FeatureRoutingService>,
}

impl FeatureService {
    pub fn new(
        feature_repo: Arc<dyn ServerFeatureRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        prefix_cache: Arc<PrefixCacheService>,
    ) -> Self {
        let discovery = Arc::new(FeatureDiscoveryService::new(feature_repo.clone()));

        let resolution = Arc::new(FeatureResolutionService::new(
            feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache.clone(),
        ));

        let routing = Arc::new(FeatureRoutingService::new(
            feature_repo.clone(),
            prefix_cache.clone(),
        ));

        Self {
            discovery,
            resolution,
            routing,
        }
    }

    // Delegate to FeatureDiscoveryService
    pub async fn discover_and_cache(
        &self,
        space_id: &str,
        server_id: &str,
        client: &McpClient,
    ) -> Result<CachedFeatures> {
        self.discovery
            .discover_and_cache(space_id, server_id, client)
            .await
    }

    pub async fn mark_unavailable(&self, space_id: &str, server_id: &str) -> Result<()> {
        self.discovery.mark_unavailable(space_id, server_id).await
    }

    pub async fn delete_for_server(&self, space_id: &str, server_id: &str) -> Result<()> {
        self.discovery.delete_for_server(space_id, server_id).await
    }

    // Delegate to FeatureResolutionService
    pub async fn resolve_feature_sets(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.resolution
            .resolve_feature_sets(space_id, feature_set_ids, None)
            .await
    }

    /// Get all available features for a space (optionally filtered by type)
    pub async fn get_all_features_for_space(
        &self,
        space_id: &str,
        filter_type: Option<FeatureType>,
    ) -> Result<Vec<ServerFeature>> {
        self.resolution
            .get_all_features_for_space(space_id, filter_type)
            .await
    }

    // Type-specific helpers
    pub async fn get_tools_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.resolution
            .resolve_feature_sets(space_id, feature_set_ids, Some(FeatureType::Tool))
            .await
    }

    /// Resolve granted feature sets to tools invokable via search/invoke ACL.
    /// Alias of [`Self::get_tools_for_grants`] surfaced for meta-tool discovery.
    pub async fn get_invokable_tools_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.get_tools_for_grants(space_id, feature_set_ids).await
    }

    /// Tools promoted into client `tools/list` (surfaced backend tools only).
    pub async fn get_advertised_tools_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        if feature_set_ids.is_empty() {
            return Ok(Vec::new());
        }

        let invokable = self
            .get_invokable_tools_for_grants(space_id, feature_set_ids)
            .await?;
        let surfaced_ids = self
            .resolution
            .resolve_surfaced_feature_ids(feature_set_ids)
            .await?;

        Ok(invokable
            .into_iter()
            .filter(|f| surfaced_ids.contains(&f.id.to_string()))
            .collect())
    }

    /// Resolve granted feature sets to resources readable via search/read ACL.
    pub async fn get_readable_resources_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.get_resources_for_grants(space_id, feature_set_ids)
            .await
    }

    /// Resources promoted into client `resources/list` (surfaced backend resources only).
    pub async fn get_advertised_resources_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        if feature_set_ids.is_empty() {
            return Ok(Vec::new());
        }

        let readable = self
            .get_readable_resources_for_grants(space_id, feature_set_ids)
            .await?;
        let surfaced_ids = self
            .resolution
            .resolve_surfaced_feature_ids(feature_set_ids)
            .await?;

        Ok(readable
            .into_iter()
            .filter(|f| surfaced_ids.contains(&f.id.to_string()))
            .collect())
    }

    /// Resolve granted feature sets to prompts fetchable via search/fetch ACL.
    pub async fn get_fetchable_prompts_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.get_prompts_for_grants(space_id, feature_set_ids).await
    }

    /// Prompts promoted into client `prompts/list` (surfaced backend prompts only).
    pub async fn get_advertised_prompts_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        if feature_set_ids.is_empty() {
            return Ok(Vec::new());
        }

        let fetchable = self
            .get_fetchable_prompts_for_grants(space_id, feature_set_ids)
            .await?;
        let surfaced_ids = self
            .resolution
            .resolve_surfaced_feature_ids(feature_set_ids)
            .await?;

        Ok(fetchable
            .into_iter()
            .filter(|f| surfaced_ids.contains(&f.id.to_string()))
            .collect())
    }

    /// Catalog tools in the Space that require binding a FeatureSet before invoke.
    pub async fn list_inactive_discovery_tools(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
        query_id: Option<&str>,
    ) -> Result<Vec<InactiveDiscoveryEntry>> {
        let invokable = self
            .get_invokable_tools_for_grants(space_id, feature_set_ids)
            .await?;
        let invokable_keys: HashSet<(String, String)> = invokable
            .iter()
            .filter(|f| f.feature_type == FeatureType::Tool)
            .map(|f| (f.server_id.clone(), f.feature_name.clone()))
            .collect();

        self.resolution
            .list_inactive_tools_for_discovery(space_id, &invokable_keys, query_id)
            .await
    }

    /// Resolve the owning server for `uri` among grant-visible readable resources.
    ///
    /// Clone servers can expose the same URI as their parent; grant-scoped
    /// resolution ensures reads route to the bound clone, not an inactive parent.
    pub fn resolve_resource_server_from_grants(
        readable: &[ServerFeature],
        uri: &str,
    ) -> Option<String> {
        readable
            .iter()
            .find(|f| {
                f.feature_type == FeatureType::Resource && f.feature_name == uri && f.is_available
            })
            .map(|f| f.server_id.clone())
    }

    pub async fn get_prompts_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.resolution
            .resolve_feature_sets(space_id, feature_set_ids, Some(FeatureType::Prompt))
            .await
    }

    pub async fn get_resources_for_grants(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<Vec<ServerFeature>> {
        self.resolution
            .resolve_feature_sets(space_id, feature_set_ids, Some(FeatureType::Resource))
            .await
    }

    // Delegate to FeatureRoutingService (with type-specific helpers)
    pub async fn find_server_for_qualified_tool(
        &self,
        space_id: &str,
        qualified_name: &str,
    ) -> Result<Option<(String, String)>> {
        self.routing
            .find_server_for_qualified_feature(space_id, qualified_name, FeatureType::Tool)
            .await
    }

    pub async fn find_server_for_qualified_prompt(
        &self,
        space_id: &str,
        qualified_name: &str,
    ) -> Result<Option<(String, String)>> {
        self.routing
            .find_server_for_qualified_feature(space_id, qualified_name, FeatureType::Prompt)
            .await
    }

    /// Find server for a resource by its URI (not prefixed)
    /// Resources use URIs which are already namespaced
    pub async fn find_server_for_resource(
        &self,
        space_id: &str,
        uri: &str,
    ) -> Result<Option<String>> {
        self.routing
            .find_server_for_resource_uri(space_id, uri)
            .await
    }

    // === Helper methods for MCP handler ===

    /// Parse qualified tool name into (server_id, tool_name)
    pub async fn parse_qualified_tool_name(
        &self,
        space_id: &str,
        qualified_name: &str,
    ) -> Result<(String, String)> {
        self.routing
            .parse_qualified_tool_name(space_id, qualified_name)
            .await
    }

    /// Parse qualified prompt name into (server_id, prompt_name)
    pub async fn parse_qualified_prompt_name(
        &self,
        space_id: &str,
        qualified_name: &str,
    ) -> Result<(String, String)> {
        self.routing
            .parse_qualified_prompt_name(space_id, qualified_name)
            .await
    }
}
