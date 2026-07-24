//! In-memory resource index for meta-gateway search and read lookup.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::{FeatureType, ServerFeature, ServerFeatureRepository};
use serde_json::{json, Value};

use super::discovery_rank::filter_and_rank;

/// How much detail search results include per matched resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceDetailLevel {
    Name,
    Description,
    Full,
}

impl ResourceDetailLevel {
    /// Parse a wire-level detail level string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "name" => Some(Self::Name),
            "description" => Some(Self::Description),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// One searchable resource entry in the Space index.
#[derive(Debug, Clone)]
pub struct ResourceIndexEntry {
    pub server_id: String,
    pub uri: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub is_available: bool,
}

/// Paginated resource search output.
#[derive(Debug, Clone)]
pub struct SearchResourcesResult {
    pub resources: Vec<Value>,
    pub next_cursor: Option<String>,
    pub total: usize,
}

/// Service that builds and queries a resource index for a Space.
pub struct ResourceDiscoveryService {
    server_feature_repo: Arc<dyn ServerFeatureRepository>,
}

impl ResourceDiscoveryService {
    /// Create a discovery service backed by the Space feature repository.
    pub fn new(server_feature_repo: Arc<dyn ServerFeatureRepository>) -> Self {
        Self {
            server_feature_repo,
        }
    }

    /// Build an index for `space_id`, retaining only resources present in `readable`.
    pub async fn build_index(
        &self,
        space_id: &str,
        readable: &[ServerFeature],
    ) -> Result<Vec<ResourceIndexEntry>> {
        let readable_keys: HashSet<(String, String)> = readable
            .iter()
            .filter(|f| f.feature_type == FeatureType::Resource)
            .map(|f| (f.server_id.clone(), f.feature_name.clone()))
            .collect();

        let features = self.server_feature_repo.list_for_space(space_id).await?;
        let mut index: Vec<ResourceIndexEntry> = features
            .into_iter()
            .filter(|f| {
                f.feature_type == FeatureType::Resource
                    && readable_keys.contains(&(f.server_id.clone(), f.feature_name.clone()))
            })
            .map(|f| ResourceIndexEntry {
                server_id: f.server_id.clone(),
                uri: f.feature_name.clone(),
                name: extract_resource_name(f.raw_json.as_ref()),
                description: f.description.clone(),
                mime_type: extract_mime_type(f.raw_json.as_ref()),
                is_available: f.is_available,
            })
            .collect();

        index.sort_by(|a, b| a.uri.cmp(&b.uri));
        Ok(index)
    }

    /// Search the index with optional query, server filter, and pagination.
    pub fn search(
        index: &[ResourceIndexEntry],
        query: Option<&str>,
        server_id: Option<&str>,
        detail_level: ResourceDetailLevel,
        limit: usize,
        cursor: Option<&str>,
    ) -> SearchResourcesResult {
        let limit = limit.clamp(1, 100);
        let offset = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);

        let filtered = filter_and_rank(
            index,
            query,
            server_id,
            |entry| entry.server_id.as_str(),
            |entry| {
                format!(
                    "{} {} {}",
                    entry.uri,
                    entry.name.as_deref().unwrap_or(""),
                    entry.description.as_deref().unwrap_or("")
                )
            },
        );

        let total = filtered.len();
        let page: Vec<Value> = filtered
            .iter()
            .skip(offset)
            .take(limit)
            .map(|entry| entry_to_json(entry, detail_level))
            .collect();

        let next_offset = offset + page.len();
        let next_cursor = if next_offset < total {
            Some(next_offset.to_string())
        } else {
            None
        };

        SearchResourcesResult {
            resources: page,
            next_cursor,
            total,
        }
    }
}

fn extract_resource_name(raw_json: Option<&Value>) -> Option<String> {
    raw_json.and_then(|json| json.get("name").and_then(|v| v.as_str()).map(String::from))
}

fn extract_mime_type(raw_json: Option<&Value>) -> Option<String> {
    raw_json.and_then(|json| {
        json.get("mimeType")
            .or_else(|| json.get("mime_type"))
            .and_then(|v| v.as_str())
            .map(String::from)
    })
}

fn entry_to_json(entry: &ResourceIndexEntry, detail_level: ResourceDetailLevel) -> Value {
    let mut obj = json!({
        "server_id": entry.server_id,
        "uri": entry.uri,
        "available": entry.is_available,
    });
    match detail_level {
        ResourceDetailLevel::Name => {}
        ResourceDetailLevel::Description | ResourceDetailLevel::Full => {
            if let Some(name) = &entry.name {
                obj["name"] = json!(name);
            }
            if let Some(desc) = &entry.description {
                obj["description"] = json!(desc);
            }
        }
    }
    if detail_level == ResourceDetailLevel::Full {
        if let Some(mime) = &entry.mime_type {
            obj["mime_type"] = json!(mime);
        }
    }
    obj
}
