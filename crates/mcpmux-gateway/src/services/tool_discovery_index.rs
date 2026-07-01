//! Tool index build helpers and embedding content-hash keys.

use std::collections::HashSet;

use anyhow::Result;
use mcpmux_core::{FeatureType, ServerFeature};
use serde_json::Value;

use crate::pool::InactiveDiscoveryEntry;
use crate::services::embedding::EmbeddingService;

use super::tool_discovery_types::ToolIndexEntry;
use super::ToolDiscoveryService;

/// Extract MCP `inputSchema` from a cached tool JSON blob.
fn extract_input_schema(raw_json: Option<&Value>) -> Option<Value> {
    raw_json.and_then(|json| {
        json.get("inputSchema")
            .or_else(|| json.get("input_schema"))
            .cloned()
    })
}

/// Stable alias-free content hash for embedding vectors.
pub fn entry_content_hash(entry: &ToolIndexEntry) -> String {
    EmbeddingService::content_hash(&entry.feature_name, entry.description.as_deref())
}

impl ToolDiscoveryService {
    /// Build an index of every tool installed in `space_id` (ignores FeatureSet ACL).
    pub async fn build_catalog_index(&self, space_id: &str) -> Result<Vec<ToolIndexEntry>> {
        let features = self.server_feature_repo.list_for_space(space_id).await?;
        let mut index: Vec<ToolIndexEntry> = features
            .into_iter()
            .filter(|f| f.feature_type == FeatureType::Tool)
            .map(|f| ToolIndexEntry {
                server_id: f.server_id.clone(),
                feature_name: f.feature_name.clone(),
                qualified_name: f.qualified_name(),
                description: f.description.clone(),
                input_schema: extract_input_schema(f.raw_json.as_ref()),
                is_available: f.is_available,
                status: None,
                bindable_feature_set_id: None,
            })
            .collect();
        index.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        Ok(index)
    }

    /// Build an index for `space_id`, retaining only tools present in `invokable`.
    pub async fn build_index(
        &self,
        space_id: &str,
        invokable: &[ServerFeature],
    ) -> Result<Vec<ToolIndexEntry>> {
        let invokable_keys: HashSet<(String, String)> = invokable
            .iter()
            .filter(|f| f.feature_type == FeatureType::Tool)
            .map(|f| (f.server_id.clone(), f.feature_name.clone()))
            .collect();

        let features = self.server_feature_repo.list_for_space(space_id).await?;
        let mut index: Vec<ToolIndexEntry> = features
            .into_iter()
            .filter(|f| {
                f.feature_type == FeatureType::Tool
                    && invokable_keys.contains(&(f.server_id.clone(), f.feature_name.clone()))
            })
            .map(|f| ToolIndexEntry {
                server_id: f.server_id.clone(),
                feature_name: f.feature_name.clone(),
                qualified_name: f.qualified_name(),
                description: f.description.clone(),
                input_schema: extract_input_schema(f.raw_json.as_ref()),
                is_available: f.is_available,
                status: None,
                bindable_feature_set_id: None,
            })
            .collect();

        index.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        Ok(index)
    }

    /// Build index entries for tools that exist in a FeatureSet but are not invokable yet.
    pub fn build_inactive_index(entries: &[InactiveDiscoveryEntry]) -> Vec<ToolIndexEntry> {
        let mut index: Vec<ToolIndexEntry> = entries
            .iter()
            .map(|entry| {
                let f = &entry.feature;
                ToolIndexEntry {
                    server_id: f.server_id.clone(),
                    feature_name: f.feature_name.clone(),
                    qualified_name: f.qualified_name(),
                    description: f.description.clone(),
                    input_schema: extract_input_schema(f.raw_json.as_ref()),
                    is_available: f.is_available,
                    status: Some("inactive".to_string()),
                    bindable_feature_set_id: Some(entry.bindable_feature_set_id.clone()),
                }
            })
            .collect();
        index.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        index
    }
}
