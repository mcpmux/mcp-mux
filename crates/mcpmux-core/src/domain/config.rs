use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use regex::Regex;
use lazy_static::lazy_static;
use crate::domain::server::{ServerDefinition, TransportConfig, TransportMetadata, InputDefinition, ServerSource, PublisherInfo, AuthConfig};

lazy_static! {
    static ref INPUT_REGEX: Regex = Regex::new(r"\$\{input:([A-Z_][A-Z0-9_]*)\}").unwrap();
}

/// Format A: User Space Configuration File
#[derive(Debug, Serialize, Deserialize)]
pub struct UserSpaceConfig {
    #[serde(rename = "mcpServers")]
    pub servers: HashMap<String, UserServerEntry>,
}

/// A single server entry in Format A (User Space Config)
/// 
/// **IMPORTANT**: This follows the Standard MCP Format used by VS Code, Cursor, Claude Desktop.
/// Transport fields (command/args/env OR url/headers) go at the TOP LEVEL.
/// There is NO `transport: {}` wrapper - users copy the CONTENTS of registry transport blocks.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserServerEntry {
    // --- Stdio Transport (command-based) ---
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    
    // --- HTTP Transport (URL-based) ---
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,

    // --- Common Metadata ---
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub alias: Option<String>,
    pub auth: Option<AuthConfig>,
    
    // Optional metadata block with inputs definition
    pub metadata: Option<UserServerMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserServerMetadata {
    pub inputs: Option<Vec<InputDefinition>>,
    // We might allow overriding publisher info locally, though rare
    pub publisher: Option<PublisherInfo>,
}

impl UserSpaceConfig {
    pub fn to_server_definitions(&self, space_id: &str, file_path: std::path::PathBuf) -> Vec<ServerDefinition> {
        self.servers.iter().map(|(id, entry)| {
            entry.to_server_definition(id, space_id, file_path.clone())
        }).collect()
    }
}

impl UserServerEntry {
    pub fn to_server_definition(&self, id: &str, space_id: &str, file_path: std::path::PathBuf) -> ServerDefinition {
        let (transport, inputs) = self.resolve_transport_and_inputs();

        // Dynamically figure out AuthConfig if missing
        let auth = self.auth.clone().or_else(|| {
            // Heuristic: If we have required secret inputs, assume ApiKey
            let has_required_secret = inputs.iter().any(|i| i.required && i.secret);
            let has_optional_secret = inputs.iter().any(|i| !i.required && i.secret);
            
            if has_required_secret {
                Some(AuthConfig::ApiKey { instructions: None })
            } else if has_optional_secret {
                Some(AuthConfig::OptionalApiKey { instructions: None })
            } else {
                Some(AuthConfig::None)
            }
        });

        // Normalize the server ID for prefix compatibility:
        // - Remove spaces and special chars (concatenate words)
        // - Convert to lowercase
        // - IMPORTANT: No underscores allowed in prefix (underscore is the delimiter in qualified names)
        // This ensures tool prefixing works correctly (prefix_toolname format)
        let normalized_id = Self::normalize_server_id(id);
        
        // Use explicit alias if provided, otherwise auto-generate from normalized ID
        // Aliases must also be underscore-free for routing to work
        let alias = self.alias.clone()
            .map(|a| Self::normalize_alias(&a))
            .or_else(|| {
                // Auto-generate alias if ID was normalized (i.e., contained spaces/special chars)
                if normalized_id != id.to_lowercase() {
                    Some(normalized_id.clone())
                } else {
                    None
                }
            });

        ServerDefinition {
            id: normalized_id,
            name: self.name.clone().unwrap_or_else(|| id.to_string()), // Keep original name for display
            description: self.description.clone(),
            icon: self.icon.clone(),
            alias,
            auth,
            transport: self.inject_inputs_into_transport(transport, inputs),
            categories: vec![],
            publisher: self.metadata.as_ref().and_then(|m| m.publisher.clone()),
            source: ServerSource::UserSpace {
                space_id: space_id.to_string(),
                file_path,
            },
        }
    }

    /// Normalize a server ID for prefix compatibility
    /// Removes spaces and special characters, converts to lowercase
    /// IMPORTANT: No underscores - underscore is reserved as delimiter in qualified names (prefix_toolname)
    fn normalize_server_id(id: &str) -> String {
        id.chars()
            .filter_map(|c| {
                if c.is_alphanumeric() {
                    Some(c.to_ascii_lowercase())
                } else if c == '-' || c == '.' {
                    Some(c) // Keep hyphens and dots
                } else {
                    None // Remove spaces, underscores, and other special chars
                }
            })
            .collect()
    }
    
    /// Normalize an alias to be underscore-free
    /// Underscores are replaced with hyphens since underscore is the prefix_toolname delimiter
    fn normalize_alias(alias: &str) -> String {
        alias.chars()
            .map(|c| {
                if c == '_' {
                    '-' // Replace underscore with hyphen
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect()
    }

    fn resolve_transport_and_inputs(&self) -> (TransportConfig, Vec<InputDefinition>) {
        // Determine transport type from top-level fields
        // Standard MCP format: command/args/env for stdio, url/headers for http
        let transport = if let Some(url) = &self.url {
            // HTTP transport (URL-based)
            TransportConfig::Http {
                url: url.clone(),
                headers: self.headers.clone().unwrap_or_default(),
                metadata: TransportMetadata::default(),
            }
        } else if let Some(cmd) = &self.command {
            // Stdio transport (command-based)
            TransportConfig::Stdio {
                command: cmd.clone(),
                args: self.args.clone().unwrap_or_default(),
                env: self.env.clone().unwrap_or_default(),
                metadata: TransportMetadata::default(),
            }
        } else {
            // Fallback / Error case - default to empty stdio
            TransportConfig::Stdio {
                command: String::new(),
                args: vec![],
                env: HashMap::new(),
                metadata: TransportMetadata::default(),
            }
        };

        // 2. Gather explicit inputs
        // Check metadata.inputs (Format A style)
        let mut inputs_map: HashMap<String, InputDefinition> = self.metadata
            .as_ref()
            .and_then(|m| m.inputs.as_ref())
            .map(|inputs| inputs.iter().map(|i| (i.id.clone(), i.clone())).collect())
            .unwrap_or_default();

        // Check transport.metadata.inputs (Format B copy-paste style)
        match &transport {
            TransportConfig::Stdio { metadata, .. } 
            | TransportConfig::Http { metadata, .. } => {
                for input in &metadata.inputs {
                    inputs_map.entry(input.id.clone()).or_insert(input.clone());
                }
            }
        }

        // 3. Auto-discover inputs from placeholders in command, args, and env
        let mut discovered_ids = std::collections::HashSet::new();
        
        // Scan command
        if let TransportConfig::Stdio { command, .. } = &transport {
            for cap in INPUT_REGEX.captures_iter(command) {
                discovered_ids.insert(cap[1].to_string());
            }
        }
        
        // Scan args
        if let TransportConfig::Stdio { args, .. } = &transport {
            for arg in args {
                for cap in INPUT_REGEX.captures_iter(arg) {
                    discovered_ids.insert(cap[1].to_string());
                }
            }
        }
        
        // Scan environment variables
        if let TransportConfig::Stdio { env, .. } = &transport {
            for value in env.values() {
                for cap in INPUT_REGEX.captures_iter(value) {
                    discovered_ids.insert(cap[1].to_string());
                }
            }
        }
        
        // Create InputDefinitions for discovered IDs (if not already defined)
        for input_id in discovered_ids {
            inputs_map.entry(input_id.clone()).or_insert_with(|| InputDefinition {
                id: input_id.clone(),
                label: input_id,
                r#type: "password".to_string(), // Default to secret
                required: true,
                secret: true,
                description: None,
                placeholder: None,
                obtain_url: None,
                obtain_instructions: None,
            });
        }

        (transport, inputs_map.into_values().collect())
    }

    fn inject_inputs_into_transport(&self, mut transport: TransportConfig, inputs: Vec<InputDefinition>) -> TransportConfig {
        // Update the transport's metadata with the consolidated inputs
        match &mut transport {
            TransportConfig::Stdio { metadata, .. } 
            | TransportConfig::Http { metadata, .. } => {
                metadata.inputs = inputs;
            }
        }
        transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_auto_discover_from_env() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: Some(vec!["server.js".to_string()]),
            env: Some(HashMap::from([
                ("GITHUB_TOKEN".to_string(), "${input:GITHUB_TOKEN}".to_string()),
            ])),
            transport: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();
        
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "GITHUB_TOKEN");
        assert_eq!(inputs[0].r#type, "password");
        assert!(inputs[0].required);
        assert!(inputs[0].secret);
    }

    #[test]
    fn test_auto_discover_from_command() {
        let entry = UserServerEntry {
            command: Some("${input:BINARY_PATH}".to_string()),
            args: None,
            env: None,
            transport: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();
        
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "BINARY_PATH");
    }

    #[test]
    fn test_auto_discover_from_args() {
        let entry = UserServerEntry {
            command: Some("gh".to_string()),
            args: Some(vec![
                "api".to_string(),
                "--token".to_string(),
                "${input:GITHUB_TOKEN}".to_string(),
            ]),
            env: None,
            transport: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();
        
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "GITHUB_TOKEN");
    }

    #[test]
    fn test_auto_discover_multiple_placeholders() {
        let entry = UserServerEntry {
            command: Some("${input:CLI_PATH}".to_string()),
            args: Some(vec![
                "--token".to_string(),
                "${input:API_TOKEN}".to_string(),
            ]),
            env: Some(HashMap::from([
                ("API_KEY".to_string(), "${input:API_KEY}".to_string()),
            ])),
            transport: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();
        
        // Should discover 3 unique inputs
        assert_eq!(inputs.len(), 3);
        
        let input_ids: std::collections::HashSet<String> = 
            inputs.iter().map(|i| i.id.clone()).collect();
        
        assert!(input_ids.contains("CLI_PATH"));
        assert!(input_ids.contains("API_TOKEN"));
        assert!(input_ids.contains("API_KEY"));
    }

    #[test]
    fn test_auto_discover_deduplication() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: Some(vec![
                "--token".to_string(),
                "${input:TOKEN}".to_string(),
            ]),
            env: Some(HashMap::from([
                ("TOKEN".to_string(), "${input:TOKEN}".to_string()),
                ("BACKUP_TOKEN".to_string(), "${input:TOKEN}".to_string()),
            ])),
            transport: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();
        
        // TOKEN appears 3 times but should only be discovered once
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "TOKEN");
    }

    #[test]
    fn test_explicit_inputs_take_precedence() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: None,
            env: Some(HashMap::from([
                ("API_KEY".to_string(), "${input:API_KEY}".to_string()),
            ])),
            transport: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: Some(UserServerMetadata {
                inputs: Some(vec![
                    InputDefinition {
                        id: "API_KEY".to_string(),
                        label: "My Custom Label".to_string(),
                        r#type: "text".to_string(),
                        required: false,
                        secret: false,
                        description: Some("Custom description".to_string()),
                        placeholder: None,
                        obtain_url: None,
                        obtain_instructions: None,
                    }
                ]),
                publisher: None,
            }),
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();
        
        // Should use explicit definition, not auto-discovered defaults
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "API_KEY");
        assert_eq!(inputs[0].label, "My Custom Label");
        assert_eq!(inputs[0].r#type, "text");
        assert!(!inputs[0].required);
        assert!(!inputs[0].secret);
    }

    #[test]
    fn test_user_space_config_parsing() {
        let json = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {
                        "API_KEY": "${input:API_KEY}"
                    }
                }
            }
        }"#;

        let config: UserSpaceConfig = serde_json::from_str(json).unwrap();
        
        assert_eq!(config.servers.len(), 1);
        assert!(config.servers.contains_key("test-server"));
        
        let definitions = config.to_server_definitions("test-space", PathBuf::from("/test/path.json"));
        
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].id, "test-server");
        
        // Check that input was auto-discovered
        let inputs = &definitions[0].transport.metadata().inputs;
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "API_KEY");
    }
}

