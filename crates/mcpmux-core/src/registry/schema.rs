//! MCP Server Registry Schema
//!
//! This module defines the complete schema for MCP server registry entries.
//! The registry can be loaded from a local JSON file or fetched from a remote API.
//!
//! Supports both formats:
//! - **Keyed object**: `{ "servers": { "io.github.xxx/yyy": { ... } } }`
//! - **Array**: `{ "servers": [{ "id": "io.github.xxx/yyy", ... }] }`

use super::types::*;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Registry schema version for compatibility
pub const REGISTRY_SCHEMA_VERSION: &str = "1.0";

/// Complete MCP Server Registry response (from JSON file or API)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerRegistry {
    /// Schema version
    #[serde(default = "default_version")]
    pub version: String,

    /// Registry metadata
    #[serde(default)]
    pub metadata: RegistryMetadata,

    /// Map of server ID to server definition
    /// Supports both array and object formats from API
    #[serde(deserialize_with = "deserialize_servers")]
    pub servers: HashMap<String, RegistryServer>,
}

fn default_version() -> String {
    REGISTRY_SCHEMA_VERSION.to_string()
}

/// Deserialize servers from either array or keyed object format
fn deserialize_servers<'de, D>(deserializer: D) -> Result<HashMap<String, RegistryServer>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, MapAccess, SeqAccess, Visitor};
    use std::fmt;

    struct ServersVisitor;

    impl<'de> Visitor<'de> for ServersVisitor {
        type Value = HashMap<String, RegistryServer>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a map of server IDs to servers, or an array of servers")
        }

        // Handle keyed object: { "id1": {...}, "id2": {...} }
        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut servers = HashMap::new();
            while let Some((key, value)) = map.next_entry::<String, RegistryServer>()? {
                servers.insert(key, value);
            }
            Ok(servers)
        }

        // Handle array: [{ "id": "...", ... }, { "id": "...", ... }]
        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: SeqAccess<'de>,
        {
            let mut servers = HashMap::new();
            while let Some(server) = seq.next_element::<RegistryServer>()? {
                if server.id.is_empty() {
                    return Err(de::Error::custom("server in array must have an 'id' field"));
                }
                servers.insert(server.id.clone(), server);
            }
            Ok(servers)
        }
    }

    deserializer.deserialize_any(ServersVisitor)
}

/// Registry metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistryMetadata {
    /// Registry name
    pub name: Option<String>,

    /// Registry description
    pub description: Option<String>,

    /// Last updated timestamp (ISO 8601)
    pub updated_at: Option<String>,

    /// Registry maintainer
    pub maintainer: Option<String>,

    /// Registry URL
    pub url: Option<String>,
}

/// Complete MCP Server definition from registry
///
/// This is the authoritative definition for an MCP server.
/// Users install servers from the registry into their spaces.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryServer {
    /// Globally unique identifier in reverse-domain notation
    /// e.g., "com.cloudflare/bindings-mcp", "io.github.github/github-mcp-server"
    pub id: String,

    /// Human-readable display name
    pub name: String,

    /// Optional short alias for tool prefixing (e.g., "gh" for GitHub)
    /// Used when exposing tools to clients if no conflict with other connected servers
    #[serde(default)]
    pub alias: Option<String>,

    /// Short description (1-2 sentences)
    pub description: Option<String>,

    /// Long description / about section (markdown supported)
    pub about: Option<String>,

    /// Icon URL or emoji
    pub icon: Option<String>,

    /// Schema version for this server definition
    #[serde(default = "default_version")]
    pub schema_version: String,

    /// Server version (semver)
    pub version: Option<String>,

    /// Primary category
    pub category: Option<ServerCategory>,

    /// Additional categories
    #[serde(default)]
    pub categories: Vec<ServerCategory>,

    /// Tags for search/filtering
    #[serde(default)]
    pub tags: Vec<String>,

    /// Publisher information
    pub publisher: Option<Publisher>,

    /// Quality/trust indicators
    #[serde(default)]
    pub quality: QualityIndicators,

    /// Related links
    #[serde(default)]
    pub links: ServerLinks,

    /// Platform compatibility
    #[serde(default)]
    pub platforms: Vec<Platform>,

    /// User inputs required for configuration
    #[serde(default)]
    pub inputs: Vec<InputDefinition>,

    /// Transport configuration (can also be specified as "connection" in JSON for backward compatibility)
    #[serde(alias = "connection", rename = "transport")]
    pub transport: TransportConfig,

    /// Authentication configuration
    #[serde(default)]
    pub auth: Option<AuthConfig>,

    /// Deprecated flag
    #[serde(default)]
    pub deprecated: bool,

    /// Deprecation message
    pub deprecation_message: Option<String>,

    /// Replacement server ID (if deprecated)
    pub replaced_by: Option<String>,
}


impl RegistryServer {
    /// Check if this server requires user configuration
    pub fn requires_configuration(&self) -> bool {
        !self.inputs.is_empty() || self.auth.as_ref().map(|a| a.auth_type != AuthType::None).unwrap_or(false)
    }

    /// Check if this server requires OAuth
    pub fn requires_oauth(&self) -> bool {
        self.auth.as_ref().map(|a| a.auth_type == AuthType::Oauth).unwrap_or(false)
    }

    /// Get required inputs (non-optional)
    pub fn required_inputs(&self) -> Vec<&InputDefinition> {
        self.inputs.iter().filter(|i| i.required).collect()
    }

    /// Get secret inputs
    pub fn secret_inputs(&self) -> Vec<&InputDefinition> {
        self.inputs.iter().filter(|i| i.secret).collect()
    }

    /// Get the effective name for tool prefixing.
    /// Returns alias if available, otherwise falls back to id.
    pub fn effective_prefix(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.id)
    }
}

impl ServerRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            version: REGISTRY_SCHEMA_VERSION.to_string(),
            metadata: RegistryMetadata::default(),
            servers: std::collections::HashMap::new(),
        }
    }

    /// Get a server by ID
    pub fn get(&self, id: &str) -> Option<&RegistryServer> {
        self.servers.get(id)
    }

    /// List all server IDs
    pub fn server_ids(&self) -> Vec<&str> {
        self.servers.keys().map(|s| s.as_str()).collect()
    }

    /// Filter servers by category
    pub fn by_category(&self, category: &ServerCategory) -> Vec<&RegistryServer> {
        self.servers
            .values()
            .filter(|s| {
                s.category.as_ref() == Some(category) || s.categories.contains(category)
            })
            .collect()
    }

    /// Filter servers by tag
    pub fn by_tag(&self, tag: &str) -> Vec<&RegistryServer> {
        self.servers
            .values()
            .filter(|s| s.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
            .collect()
    }

    /// Search servers by name, description, or tags
    pub fn search(&self, query: &str) -> Vec<&RegistryServer> {
        let query_lower = query.to_lowercase();
        self.servers
            .values()
            .filter(|s| {
                s.name.to_lowercase().contains(&query_lower)
                    || s.id.to_lowercase().contains(&query_lower)
                    || s.description
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
                    || s.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Get featured/verified servers
    pub fn featured(&self) -> Vec<&RegistryServer> {
        self.servers
            .values()
            .filter(|s| s.quality.featured || s.quality.verified)
            .collect()
    }

    /// Get official servers
    pub fn official(&self) -> Vec<&RegistryServer> {
        self.servers
            .values()
            .filter(|s| s.quality.official)
            .collect()
    }
}

impl Default for ServerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_registry_json() {
        let json = r#"{
            "version": "1.0",
            "metadata": {
                "name": "Test Registry"
            },
            "servers": {
                "com.cloudflare/docs-mcp": {
                    "id": "com.cloudflare/docs-mcp",
                    "name": "Cloudflare Documentation",
                    "description": "Search Cloudflare docs",
                    "transport": {
                        "type": "http",
                        "url": "https://docs.mcp.cloudflare.com/mcp"
                    }
                }
            }
        }"#;

        let registry: ServerRegistry = serde_json::from_str(json).unwrap();
        assert_eq!(registry.servers.len(), 1);
        assert!(registry.get("com.cloudflare/docs-mcp").is_some());
    }

    #[test]
    fn test_stdio_server_with_inputs() {
        let json = r#"{
            "id": "io.github.github/github-mcp-server",
            "name": "GitHub",
            "description": "GitHub integration",
            "inputs": [
                {
                    "id": "GITHUB_PERSONAL_ACCESS_TOKEN",
                    "label": "GitHub Personal Access Token",
                    "type": "password",
                    "required": true,
                    "secret": true,
                    "obtain": {
                        "url": "https://github.com/settings/tokens"
                    }
                }
            ],
            "transport": {
                "type": "stdio",
                "command": "docker",
                "args": ["run", "-i", "--rm", "-e", "GITHUB_PERSONAL_ACCESS_TOKEN", "ghcr.io/github/github-mcp-server"],
                "env": {
                    "GITHUB_PERSONAL_ACCESS_TOKEN": "${input:GITHUB_PERSONAL_ACCESS_TOKEN}"
                }
            },
            "auth": {
                "type": "api_key",
                "instructions": "Get token from GitHub"
            }
        }"#;

        let server: RegistryServer = serde_json::from_str(json).unwrap();
        assert!(server.requires_configuration());
        assert_eq!(server.required_inputs().len(), 1);
        assert_eq!(server.secret_inputs().len(), 1);
    }
}
