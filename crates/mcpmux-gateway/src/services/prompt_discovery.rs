//! In-memory prompt index for meta-gateway search and fetch lookup.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use mcpmux_core::{FeatureType, ServerFeature, ServerFeatureRepository};
use serde_json::{json, Value};

use super::discovery_rank::filter_and_rank;

/// How much detail search results include per matched prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDetailLevel {
    Name,
    Description,
    Full,
}

impl PromptDetailLevel {
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

/// One searchable prompt entry in the Space index.
#[derive(Debug, Clone)]
pub struct PromptIndexEntry {
    pub server_id: String,
    pub feature_name: String,
    pub qualified_name: String,
    pub description: Option<String>,
    pub arguments: Option<Value>,
    pub is_available: bool,
}

/// Paginated prompt search output.
#[derive(Debug, Clone)]
pub struct SearchPromptsResult {
    pub prompts: Vec<Value>,
    pub next_cursor: Option<String>,
    pub total: usize,
}

/// Service that builds and queries a prompt index for a Space.
pub struct PromptDiscoveryService {
    server_feature_repo: Arc<dyn ServerFeatureRepository>,
}

impl PromptDiscoveryService {
    /// Create a discovery service backed by the Space feature repository.
    pub fn new(server_feature_repo: Arc<dyn ServerFeatureRepository>) -> Self {
        Self {
            server_feature_repo,
        }
    }

    /// Build an index for `space_id`, retaining only prompts present in `fetchable`.
    pub async fn build_index(
        &self,
        space_id: &str,
        fetchable: &[ServerFeature],
    ) -> Result<Vec<PromptIndexEntry>> {
        let fetchable_keys: HashSet<(String, String)> = fetchable
            .iter()
            .filter(|f| f.feature_type == FeatureType::Prompt)
            .map(|f| (f.server_id.clone(), f.feature_name.clone()))
            .collect();

        let features = self.server_feature_repo.list_for_space(space_id).await?;
        let mut index: Vec<PromptIndexEntry> = features
            .into_iter()
            .filter(|f| {
                f.feature_type == FeatureType::Prompt
                    && fetchable_keys.contains(&(f.server_id.clone(), f.feature_name.clone()))
            })
            .map(|f| PromptIndexEntry {
                server_id: f.server_id.clone(),
                feature_name: f.feature_name.clone(),
                qualified_name: f.qualified_name(),
                description: f.description.clone(),
                arguments: extract_prompt_arguments(f.raw_json.as_ref()),
                is_available: f.is_available,
            })
            .collect();

        index.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        Ok(index)
    }

    /// Search the index with optional query, server filter, and pagination.
    pub fn search(
        index: &[PromptIndexEntry],
        query: Option<&str>,
        server_id: Option<&str>,
        detail_level: PromptDetailLevel,
        limit: usize,
        cursor: Option<&str>,
    ) -> SearchPromptsResult {
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
                    entry.qualified_name,
                    entry.feature_name,
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

        SearchPromptsResult {
            prompts: page,
            next_cursor,
            total,
        }
    }
}

fn extract_prompt_arguments(raw_json: Option<&Value>) -> Option<Value> {
    raw_json.and_then(|json| json.get("arguments").or_else(|| json.get("args")).cloned())
}

fn entry_to_json(entry: &PromptIndexEntry, detail_level: PromptDetailLevel) -> Value {
    let mut obj = json!({
        "server_id": entry.server_id,
        "qualified_name": entry.qualified_name,
        "prompt": entry.feature_name,
        "available": entry.is_available,
    });
    match detail_level {
        PromptDetailLevel::Name => {}
        PromptDetailLevel::Description | PromptDetailLevel::Full => {
            if let Some(desc) = &entry.description {
                obj["description"] = json!(desc);
            }
        }
    }
    if detail_level == PromptDetailLevel::Full {
        if let Some(args) = &entry.arguments {
            obj["arguments"] = args.clone();
        }
    }
    obj
}
