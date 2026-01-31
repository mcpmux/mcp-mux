//! Feature Service Facade - Unified API delegating to specialized services

use std::sync::Arc;
use anyhow::Result;

use mcpmux_core::{
    FeatureSetRepository, FeatureType, ServerFeature, ServerFeatureRepository,
};
use crate::services::PrefixCacheService;
use crate::pool::instance::McpClient;

use super::{
    CachedFeatures, FeatureDiscoveryService, FeatureResolutionService, FeatureRoutingService,
};

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
        let discovery = Arc::new(FeatureDiscoveryService::new(
            feature_repo.clone(),
            feature_set_repo.clone(),
        ));
        
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
        self.discovery.discover_and_cache(space_id, server_id, client).await
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
    pub async fn parse_qualified_tool_name(&self, space_id: &str, qualified_name: &str) -> Result<(String, String)> {
        self.routing.parse_qualified_tool_name(space_id, qualified_name).await
    }
    
    /// Parse qualified prompt name into (server_id, prompt_name)
    pub async fn parse_qualified_prompt_name(&self, space_id: &str, qualified_name: &str) -> Result<(String, String)> {
        self.routing.parse_qualified_prompt_name(space_id, qualified_name).await
    }
}


