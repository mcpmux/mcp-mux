//! ServerFeature entity - discovered MCP features from servers

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Type of MCP feature
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FeatureType {
    /// Tool that can be invoked
    #[default]
    Tool,
    /// Prompt template
    Prompt,
    /// Resource that can be read
    Resource,
}

impl FeatureType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Prompt => "prompt",
            Self::Resource => "resource",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tool" => Some(Self::Tool),
            "prompt" => Some(Self::Prompt),
            "resource" => Some(Self::Resource),
            _ => None,
        }
    }
}


/// A discovered feature from an MCP server
///
/// Features are discovered when a server is connected and reports
/// its available tools, prompts, and resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerFeature {
    /// Unique ID for this feature record
    pub id: Uuid,

    /// Space this feature was discovered in
    pub space_id: String,

    /// Registry server ID this feature came from
    pub server_id: String,

    /// Optional server alias for prefixing (if unique within space)
    #[serde(default)]
    pub server_alias: Option<String>,

    /// Type of feature
    pub feature_type: FeatureType,

    /// MCP feature name (unique per server+type)
    pub feature_name: String,

    /// Human-readable display name
    pub display_name: Option<String>,

    /// Description of what this feature does
    pub description: Option<String>,
    
    /// Raw JSON from backend MCP server (complete feature object)
    /// This preserves all fields from the backend, making the system
    /// forward-compatible with any MCP protocol changes
    pub raw_json: Option<serde_json::Value>,

    /// When this feature was first discovered
    pub discovered_at: DateTime<Utc>,

    /// When this feature was last seen
    pub last_seen_at: DateTime<Utc>,

    /// Whether this feature is currently available
    pub is_available: bool,
}

impl ServerFeature {
    /// Create a new server feature
    pub fn new(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        feature_type: FeatureType,
        feature_name: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            space_id: space_id.into(),
            server_id: server_id.into(),
            server_alias: None,
            feature_type,
            feature_name: feature_name.into(),
            display_name: None,
            description: None,
            raw_json: None,
            discovered_at: now,
            last_seen_at: now,
            is_available: true,
        }
    }

    /// Create a new tool feature
    pub fn tool(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::new(space_id, server_id, FeatureType::Tool, name)
    }

    /// Create a new prompt feature
    pub fn prompt(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::new(space_id, server_id, FeatureType::Prompt, name)
    }

    /// Create a new resource feature
    pub fn resource(
        space_id: impl Into<String>,
        server_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self::new(space_id, server_id, FeatureType::Resource, name)
    }

    /// Set display name
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set raw JSON (complete backend feature object)
    pub fn with_raw_json(mut self, json: serde_json::Value) -> Self {
        self.raw_json = Some(json);
        self
    }

    /// Set server alias
    pub fn with_server_alias(mut self, alias: Option<String>) -> Self {
        self.server_alias = alias;
        self
    }

    /// Mark as seen (update last_seen_at)
    pub fn mark_seen(&mut self) {
        self.last_seen_at = Utc::now();
        self.is_available = true;
    }

    /// Mark as unavailable
    pub fn mark_unavailable(&mut self) {
        self.is_available = false;
    }

    /// Get a unique key for this feature
    pub fn unique_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.space_id,
            self.server_id,
            self.feature_type.as_str(),
            self.feature_name
        )
    }

    /// Get the prefix for this feature (alias or server_id)
    /// No character transformation needed - server_ids and aliases follow MCP spec
    pub fn prefix(&self) -> &str {
        self.server_alias.as_ref().unwrap_or(&self.server_id)
    }

    /// Get a qualified name for this feature
    /// Format for tools/prompts: prefix_feature_name (e.g., "cfdocs_search")
    /// Format for resources: unchanged URI (e.g., "instant-domains://tld-categories")
    /// 
    /// Resources don't need prefixing because URIs have built-in namespacing via their scheme.
    /// 
    /// Uses underscore separator for maximum client compatibility (Cursor only allows [a-z0-9_-])
    pub fn qualified_name(&self) -> String {
        match self.feature_type {
            FeatureType::Tool | FeatureType::Prompt => {
                // Tools and prompts need prefixing for disambiguation
                // Use underscore separator for Cursor compatibility
                format!("{}_{}", self.prefix(), self.feature_name)
            }
            FeatureType::Resource => {
                // Resources use URIs which are already namespaced
                self.feature_name.clone()
            }
        }
    }

    /// Get the qualified name using only server_id (for conflict resolution)
    pub fn qualified_name_with_server_id(&self) -> String {
        match self.feature_type {
            FeatureType::Tool | FeatureType::Prompt => {
                format!("{}_{}", self.server_id, self.feature_name)
            }
            FeatureType::Resource => {
                self.feature_name.clone()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tool() {
        let feature = ServerFeature::tool(
            "space_default",
            "io.github.github/github-mcp-server",
            "create_issue",
        )
        .with_description("Create a new GitHub issue");

        assert_eq!(feature.feature_type, FeatureType::Tool);
        assert_eq!(feature.feature_name, "create_issue");
        assert!(feature.is_available);
    }

    #[test]
    fn test_unique_key() {
        let feature = ServerFeature::tool(
            "space_1",
            "com.cloudflare/docs-mcp",
            "search_docs",
        );

        assert_eq!(
            feature.unique_key(),
            "space_1:com.cloudflare/docs-mcp:tool:search_docs"
        );
    }
}
