//! Shared types for tool index build and hybrid search.

use dashmap::DashMap;
use serde_json::Value;

use crate::services::embedding::EmbeddingService;

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

/// In-memory active tool index for a resolved binding (search cache value).
pub type ToolIndex = Vec<ToolIndexEntry>;

/// Per-binding hybrid search inputs (global embedding store + active corpus).
pub struct SearchContext<'a> {
    pub embeddings: &'a EmbeddingService,
    pub embedding_store: &'a DashMap<String, Vec<f32>>,
    /// Active-only index used as the semantic embedding corpus.
    pub active_index: &'a [ToolIndexEntry],
    pub index_cache_hit: bool,
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
    /// `inactive` when matched via `include_inactive` discovery widening.
    pub status: Option<String>,
    pub bindable_feature_set_id: Option<String>,
}

/// Paginated search output.
#[derive(Debug, Clone)]
pub struct SearchToolsResult {
    pub tools: Vec<Value>,
    pub next_cursor: Option<String>,
    pub total: usize,
    /// Ranking mode used for this result set (`hybrid` or `lexical`).
    pub ranking: &'static str,
    /// Fused or lexical score of the top-ranked match when a query was provided.
    pub top_fused_score: Option<f64>,
}

impl AsRef<ToolIndexEntry> for ToolIndexEntry {
    fn as_ref(&self) -> &ToolIndexEntry {
        self
    }
}
