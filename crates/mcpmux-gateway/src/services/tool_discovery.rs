//! In-memory tool index for meta-gateway search and schema lookup.
//!
//! Built from Space [`ServerFeature`] rows and filtered to the caller's
//! invokable tool set before search/schema operations run.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::{FeatureType, ServerFeature, ServerFeatureRepository};
use serde_json::{json, Value};

/// How much detail search results include per matched tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailLevel {
    Name,
    Description,
    Schema,
}

impl DetailLevel {
    /// Parse a wire-level detail level string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "name" => Some(Self::Name),
            "description" => Some(Self::Description),
            "schema" => Some(Self::Schema),
            _ => None,
        }
    }
}

/// One searchable tool entry in the Space index.
#[derive(Debug, Clone)]
pub struct ToolIndexEntry {
    pub server_id: String,
    pub feature_name: String,
    pub qualified_name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
    pub is_available: bool,
}

/// Paginated search output.
#[derive(Debug, Clone)]
pub struct SearchToolsResult {
    pub tools: Vec<Value>,
    pub next_cursor: Option<String>,
    pub total: usize,
}

/// Service that builds and queries a tool index for a Space.
pub struct ToolDiscoveryService {
    server_feature_repo: Arc<dyn ServerFeatureRepository>,
}

impl ToolDiscoveryService {
    /// Create a discovery service backed by the Space feature repository.
    pub fn new(server_feature_repo: Arc<dyn ServerFeatureRepository>) -> Self {
        Self {
            server_feature_repo,
        }
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
            })
            .collect();

        index.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        Ok(index)
    }

    /// Search the index with optional query, server filter, and pagination.
    pub fn search(
        index: &[ToolIndexEntry],
        query: Option<&str>,
        server_id: Option<&str>,
        detail_level: DetailLevel,
        limit: usize,
        cursor: Option<&str>,
    ) -> SearchToolsResult {
        let limit = limit.clamp(1, 100);
        let offset = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);

        let query_lower = query.map(|q| q.to_lowercase());
        let filtered: Vec<&ToolIndexEntry> = index
            .iter()
            .filter(|entry| {
                if let Some(sid) = server_id {
                    if entry.server_id != sid {
                        return false;
                    }
                }
                if let Some(ref q) = query_lower {
                    let haystack = format!(
                        "{} {} {}",
                        entry.qualified_name,
                        entry.feature_name,
                        entry.description.as_deref().unwrap_or("")
                    )
                    .to_lowercase();
                    if !haystack.contains(q.as_str()) {
                        return false;
                    }
                }
                true
            })
            .collect();

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

        SearchToolsResult {
            tools: page,
            next_cursor,
            total,
        }
    }

    /// Resolve schemas for one or more qualified tool names.
    pub fn get_schemas(
        index: &[ToolIndexEntry],
        tool_names: &[String],
        compact: bool,
    ) -> Vec<Value> {
        tool_names
            .iter()
            .filter_map(|name| {
                let entry = index.iter().find(|e| e.qualified_name == *name)?;
                Some(schema_entry_to_json(entry, compact))
            })
            .collect()
    }
}

/// Extract MCP `inputSchema` from a cached tool JSON blob.
fn extract_input_schema(raw_json: Option<&Value>) -> Option<Value> {
    raw_json.and_then(|json| {
        json.get("inputSchema")
            .or_else(|| json.get("input_schema"))
            .cloned()
    })
}

fn entry_to_json(entry: &ToolIndexEntry, detail_level: DetailLevel) -> Value {
    let mut obj = json!({
        "server_id": entry.server_id,
        "qualified_name": entry.qualified_name,
        "available": entry.is_available,
    });
    match detail_level {
        DetailLevel::Name => {}
        DetailLevel::Description | DetailLevel::Schema => {
            if let Some(desc) = &entry.description {
                obj["description"] = json!(desc);
            }
        }
    }
    if detail_level == DetailLevel::Schema {
        if let Some(schema) = &entry.input_schema {
            obj["input_schema"] = schema.clone();
        }
    }
    obj
}

fn schema_entry_to_json(entry: &ToolIndexEntry, compact: bool) -> Value {
    let mut obj = json!({
        "qualified_name": entry.qualified_name,
        "server_id": entry.server_id,
        "feature_name": entry.feature_name,
    });
    if !compact {
        if let Some(desc) = &entry.description {
            obj["description"] = json!(desc);
        }
    }
    if let Some(schema) = &entry.input_schema {
        obj["input_schema"] = schema.clone();
    } else {
        obj["input_schema"] = json!({"type": "object", "properties": {}});
    }
    obj
}
