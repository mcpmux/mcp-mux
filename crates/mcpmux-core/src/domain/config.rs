use crate::domain::server::{
    AuthConfig, HostingType, InputDefinition, PublisherInfo, ServerDefinition, ServerSource,
    TransportConfig, TransportMetadata,
};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

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
    pub fn to_server_definitions(
        &self,
        space_id: &str,
        file_path: std::path::PathBuf,
    ) -> Vec<ServerDefinition> {
        self.servers
            .iter()
            .map(|(id, entry)| entry.to_server_definition(id, space_id, file_path.clone()))
            .collect()
    }
}

impl UserServerEntry {
    pub fn to_server_definition(
        &self,
        id: &str,
        space_id: &str,
        file_path: std::path::PathBuf,
    ) -> ServerDefinition {
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
        let alias = self
            .alias
            .clone()
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
            badges: vec![],
            hosting_type: HostingType::default(),
            license: None,
            license_url: None,
            installation: None,
            capabilities: None,
            sponsored: None,
            media: None,
            changelog_url: None,
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
        alias
            .chars()
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
        let mut inputs_map: HashMap<String, InputDefinition> = self
            .metadata
            .as_ref()
            .and_then(|m| m.inputs.as_ref())
            .map(|inputs| inputs.iter().map(|i| (i.id.clone(), i.clone())).collect())
            .unwrap_or_default();

        // Check transport.metadata.inputs (Format B copy-paste style)
        match &transport {
            TransportConfig::Stdio { metadata, .. } | TransportConfig::Http { metadata, .. } => {
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

        // Scan the URL and headers of HTTP servers
        if let TransportConfig::Http { url, headers, .. } = &transport {
            for value in std::iter::once(url).chain(headers.values()) {
                for cap in INPUT_REGEX.captures_iter(value) {
                    discovered_ids.insert(cap[1].to_string());
                }
            }
        }

        // Create InputDefinitions for discovered IDs (if not already defined)
        for input_id in discovered_ids {
            inputs_map
                .entry(input_id.clone())
                .or_insert_with(|| InputDefinition {
                    id: input_id.clone(),
                    label: input_id,
                    r#type: "password".to_string(), // Default to secret
                    required: true,
                    secret: true,
                    description: None,
                    default: None,
                    placeholder: None,
                    obtain_url: None,
                    obtain_instructions: None,
                });
        }

        (transport, inputs_map.into_values().collect())
    }

    fn inject_inputs_into_transport(
        &self,
        mut transport: TransportConfig,
        inputs: Vec<InputDefinition>,
    ) -> TransportConfig {
        // Update the transport's metadata with the consolidated inputs
        match &mut transport {
            TransportConfig::Stdio { metadata, .. } | TransportConfig::Http { metadata, .. } => {
                metadata.inputs = inputs;
            }
        }
        transport
    }
}

/// The literal `env` and `headers` values taken out of one server entry by
/// [`move_literal_values_to_inputs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedInputValues {
    /// The server's key in the document.
    pub key: String,
    /// The installed server id that key normalizes to.
    pub server_id: String,
    /// Input id -> the value its placeholder replaced.
    pub values: BTreeMap<String, String>,
}

/// Replace the literal `env` and `headers` values of every server in an
/// `mcpServers` document with `${input:ID}` placeholders, and return the
/// values. Stored as the servers' inputs, they are kept encrypted in the
/// database rather than in clear in the Space file. Empty values and values
/// that already use a placeholder are left alone.
///
/// The gateway also passes every input to a stdio server as an env var
/// named after its id, so an id that differs from its variable name (a
/// lowercase key, or a suffixed `API_KEY_2`) shows up there under both
/// names. Re-importing doesn't remove inputs a server no longer uses.
pub fn move_literal_values_to_inputs(doc: &mut serde_json::Value) -> Vec<MovedInputValues> {
    let Some(servers) = doc.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
        return Vec::new();
    };
    let mut moved = Vec::new();
    for (key, entry) in servers.iter_mut() {
        // Input ids the entry already uses, which a new one must not reuse.
        let mut taken: HashSet<String> = INPUT_REGEX
            .captures_iter(&entry.to_string())
            .map(|cap| cap[1].to_string())
            .collect();
        if let Some(inputs) = entry.pointer("/metadata/inputs").and_then(|v| v.as_array()) {
            taken.extend(
                inputs
                    .iter()
                    .filter_map(|i| i.get("id").and_then(|id| id.as_str()).map(str::to_string)),
            );
        }

        let mut values = BTreeMap::new();
        for field in ["env", "headers"] {
            let Some(map) = entry.get_mut(field).and_then(|v| v.as_object_mut()) else {
                continue;
            };
            for (name, value) in map.iter_mut() {
                let Some(literal) = value.as_str() else {
                    continue;
                };
                if literal.is_empty() || literal.contains("${input:") {
                    continue;
                }
                let id = unused_input_id(name, &mut taken);
                values.insert(id.clone(), literal.to_string());
                *value = serde_json::Value::String(format!("${{input:{id}}}"));
            }
        }
        if !values.is_empty() {
            moved.push(MovedInputValues {
                key: key.clone(),
                server_id: UserServerEntry::normalize_server_id(key),
                values,
            });
        }
    }
    moved
}

/// The input id (`[A-Z_][A-Z0-9_]*`) for an env or header name:
/// `Authorization` -> `AUTHORIZATION`, `x-api-key` -> `X_API_KEY`.
pub fn input_id_for_name(name: &str) -> String {
    let mut id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if id.is_empty() || id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    id
}

/// [`input_id_for_name`], suffixed if needed so it is not yet in `taken`;
/// records it there.
fn unused_input_id(name: &str, taken: &mut HashSet<String>) -> String {
    let base = input_id_for_name(name);
    let mut id = base.clone();
    let mut n = 2;
    while !taken.insert(id.clone()) {
        id = format!("{base}_{n}");
        n += 1;
    }
    id
}

#[cfg(test)]
mod tests {

    #[test]
    fn literal_env_and_header_values_move_to_inputs() {
        let mut doc = serde_json::json!({"mcpServers": {
            "My GitHub": {
                "command": "npx",
                "args": ["-y", "server", "--token", "${input:API_KEY}"],
                "env": {
                    "GITHUB_TOKEN": "ghp_literal",
                    "API_KEY": "second-literal",
                    "EMPTY": "",
                    "TENANT": "${input:TENANT}"
                }
            },
            "remote": {
                "url": "https://example.com/mcp",
                "headers": {"Authorization": "Bearer abc", "x-api-key": "k1", "X_API_KEY": "k2"}
            },
            "plain": {"command": "server"}
        }});

        let moved = move_literal_values_to_inputs(&mut doc);

        assert_eq!(
            moved,
            vec![
                MovedInputValues {
                    key: "My GitHub".into(),
                    server_id: "mygithub".into(),
                    values: BTreeMap::from([
                        ("GITHUB_TOKEN".into(), "ghp_literal".into()),
                        // API_KEY is already an input of this server (in args).
                        ("API_KEY_2".into(), "second-literal".into()),
                    ]),
                },
                MovedInputValues {
                    key: "remote".into(),
                    server_id: "remote".into(),
                    values: BTreeMap::from([
                        ("AUTHORIZATION".into(), "Bearer abc".into()),
                        ("X_API_KEY".into(), "k1".into()),
                        ("X_API_KEY_2".into(), "k2".into()),
                    ]),
                },
            ]
        );
        let github = &doc["mcpServers"]["My GitHub"];
        assert_eq!(github["env"]["GITHUB_TOKEN"], "${input:GITHUB_TOKEN}");
        assert_eq!(github["env"]["API_KEY"], "${input:API_KEY_2}");
        assert_eq!(github["env"]["EMPTY"], "");
        assert_eq!(github["env"]["TENANT"], "${input:TENANT}");
        let headers = &doc["mcpServers"]["remote"]["headers"];
        assert_eq!(headers["Authorization"], "${input:AUTHORIZATION}");
        assert_eq!(headers["x-api-key"], "${input:X_API_KEY}");
        assert_eq!(headers["X_API_KEY"], "${input:X_API_KEY_2}");
        assert!(!doc.to_string().contains("ghp_literal"));
        assert!(!doc.to_string().contains("Bearer abc"));

        // The rewritten entries define those inputs, as secrets.
        let entry: UserServerEntry =
            serde_json::from_value(doc["mcpServers"]["remote"].clone()).unwrap();
        let definition = entry.to_server_definition("remote", "space", PathBuf::new());
        let TransportConfig::Http { metadata, .. } = definition.transport else {
            panic!("expected an HTTP transport");
        };
        let mut ids: Vec<_> = metadata.inputs.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["AUTHORIZATION", "X_API_KEY", "X_API_KEY_2"]);
        assert!(metadata.inputs.iter().all(|i| i.secret));
    }

    #[test]
    fn a_document_without_literal_values_is_unchanged() {
        let original = serde_json::json!({"mcpServers": {
            "a": {"command": "server", "env": {"KEY": "${input:KEY}"}},
            "b": {"url": "https://example.com/mcp"}
        }});
        let mut doc = original.clone();
        assert!(move_literal_values_to_inputs(&mut doc).is_empty());
        assert_eq!(doc, original);
        assert!(move_literal_values_to_inputs(&mut serde_json::json!({"servers": {}})).is_empty());
    }

    #[test]
    fn input_ids_are_valid_placeholder_names() {
        let mut taken = HashSet::new();
        assert_eq!(unused_input_id("api.key", &mut taken), "API_KEY");
        assert_eq!(unused_input_id("1PASSWORD", &mut taken), "_1PASSWORD");
        assert_eq!(unused_input_id("", &mut taken), "_");
        for id in taken {
            assert!(INPUT_REGEX.is_match(&format!("${{input:{id}}}")), "{id}");
        }
    }

    #[test]
    fn http_url_and_header_placeholders_become_inputs() {
        let entry: UserServerEntry = serde_json::from_str(
            r#"{"url":"https://example.com/${input:TENANT}/mcp",
                "headers":{"Authorization":"Bearer ${input:API_TOKEN}"}}"#,
        )
        .unwrap();
        let (_, inputs) = entry.resolve_transport_and_inputs();
        let mut ids: Vec<&str> = inputs.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["API_TOKEN", "TENANT"]);
        assert!(inputs.iter().all(|i| i.secret));
    }

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_auto_discover_from_env() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: Some(vec!["server.js".to_string()]),
            env: Some(HashMap::from([(
                "GITHUB_TOKEN".to_string(),
                "${input:GITHUB_TOKEN}".to_string(),
            )])),
            url: None,
            headers: None,
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
            url: None,
            headers: None,
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
            url: None,
            headers: None,
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
            env: Some(HashMap::from([(
                "API_KEY".to_string(),
                "${input:API_KEY}".to_string(),
            )])),
            url: None,
            headers: None,
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
            args: Some(vec!["--token".to_string(), "${input:TOKEN}".to_string()]),
            env: Some(HashMap::from([
                ("TOKEN".to_string(), "${input:TOKEN}".to_string()),
                ("BACKUP_TOKEN".to_string(), "${input:TOKEN}".to_string()),
            ])),
            url: None,
            headers: None,
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
            env: Some(HashMap::from([(
                "API_KEY".to_string(),
                "${input:API_KEY}".to_string(),
            )])),
            url: None,
            headers: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: Some(UserServerMetadata {
                inputs: Some(vec![InputDefinition {
                    id: "API_KEY".to_string(),
                    label: "My Custom Label".to_string(),
                    r#type: "text".to_string(),
                    required: false,
                    secret: false,
                    description: Some("Custom description".to_string()),
                    default: None,
                    placeholder: None,
                    obtain_url: None,
                    obtain_instructions: None,
                }]),
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

        let definitions =
            config.to_server_definitions("test-space", PathBuf::from("/test/path.json"));

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].id, "test-server");

        // Check that input was auto-discovered
        let inputs = &definitions[0].transport.metadata().inputs;
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "API_KEY");
    }

    #[test]
    fn test_normalize_server_id() {
        // Basic lowercase
        assert_eq!(UserServerEntry::normalize_server_id("GitHub"), "github");

        // Hyphens and dots preserved
        assert_eq!(
            UserServerEntry::normalize_server_id("my-server.v2"),
            "my-server.v2"
        );

        // Spaces and underscores removed
        assert_eq!(
            UserServerEntry::normalize_server_id("My Server"),
            "myserver"
        );
        assert_eq!(
            UserServerEntry::normalize_server_id("my_server"),
            "myserver"
        );

        // Mixed special chars
        assert_eq!(
            UserServerEntry::normalize_server_id("GitHub Copilot v2"),
            "githubcopilotv2"
        );
    }

    #[test]
    fn test_normalize_alias() {
        // Underscores become hyphens
        assert_eq!(UserServerEntry::normalize_alias("my_alias"), "my-alias");

        // Lowercase
        assert_eq!(UserServerEntry::normalize_alias("MyAlias"), "myalias");

        // Multiple underscores
        assert_eq!(UserServerEntry::normalize_alias("a_b_c"), "a-b-c");
    }

    #[test]
    fn test_http_transport_detection() {
        let entry = UserServerEntry {
            command: None,
            args: None,
            env: None,
            url: Some("https://api.example.com/mcp".to_string()),
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer token".to_string(),
            )])),
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (transport, _) = entry.resolve_transport_and_inputs();

        match transport {
            TransportConfig::Http { url, headers, .. } => {
                assert_eq!(url, "https://api.example.com/mcp");
                assert_eq!(
                    headers.get("Authorization"),
                    Some(&"Bearer token".to_string())
                );
            }
            _ => panic!("Expected HTTP transport"),
        }
    }

    #[test]
    fn test_stdio_transport_detection() {
        let entry = UserServerEntry {
            command: Some("npx".to_string()),
            args: Some(vec!["mcp-server".to_string()]),
            env: Some(HashMap::from([(
                "NODE_ENV".to_string(),
                "production".to_string(),
            )])),
            url: None,
            headers: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (transport, _) = entry.resolve_transport_and_inputs();

        match transport {
            TransportConfig::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "npx");
                assert_eq!(args, vec!["mcp-server"]);
                assert_eq!(env.get("NODE_ENV"), Some(&"production".to_string()));
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_auto_auth_config_required_secret() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: None,
            env: Some(HashMap::from([(
                "API_KEY".to_string(),
                "${input:API_KEY}".to_string(),
            )])),
            url: None,
            headers: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None, // No explicit auth
            metadata: None,
        };

        let def = entry.to_server_definition("test", "space", PathBuf::from("/test"));

        // Should auto-detect ApiKey auth from required secret input
        assert!(matches!(def.auth, Some(AuthConfig::ApiKey { .. })));
    }

    #[test]
    fn test_explicit_auth_not_overridden() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: None,
            env: Some(HashMap::from([(
                "TOKEN".to_string(),
                "${input:TOKEN}".to_string(),
            )])),
            url: None,
            headers: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: Some(AuthConfig::Oauth),
            metadata: None,
        };

        let def = entry.to_server_definition("test", "space", PathBuf::from("/test"));

        // Explicit OAuth should not be overridden
        assert!(matches!(def.auth, Some(AuthConfig::Oauth)));
    }

    #[test]
    fn test_input_default_value_parsed_from_json() {
        let json = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {
                        "LOG_LEVEL": "${input:LOG_LEVEL}"
                    },
                    "metadata": {
                        "inputs": [
                            {
                                "id": "LOG_LEVEL",
                                "label": "Log Level",
                                "type": "text",
                                "required": false,
                                "secret": false,
                                "default": "info"
                            }
                        ]
                    }
                }
            }
        }"#;

        let config: UserSpaceConfig = serde_json::from_str(json).unwrap();
        let definitions =
            config.to_server_definitions("test-space", PathBuf::from("/test/path.json"));

        assert_eq!(definitions.len(), 1);
        let inputs = &definitions[0].transport.metadata().inputs;
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "LOG_LEVEL");
        assert_eq!(inputs[0].default, Some("info".to_string()));
    }

    #[test]
    fn test_explicit_input_with_default_takes_precedence_over_autodiscovery() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: None,
            env: Some(HashMap::from([(
                "LOG_LEVEL".to_string(),
                "${input:LOG_LEVEL}".to_string(),
            )])),
            url: None,
            headers: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: Some(UserServerMetadata {
                inputs: Some(vec![InputDefinition {
                    id: "LOG_LEVEL".to_string(),
                    label: "Log Level".to_string(),
                    r#type: "text".to_string(),
                    required: false,
                    secret: false,
                    description: None,
                    default: Some("info".to_string()),
                    placeholder: None,
                    obtain_url: None,
                    obtain_instructions: None,
                }]),
                publisher: None,
            }),
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "LOG_LEVEL");
        assert_eq!(inputs[0].default, Some("info".to_string()));
        // Should use explicit definition's type, not auto-discovered "password"
        assert_eq!(inputs[0].r#type, "text");
        assert!(!inputs[0].required);
        assert!(!inputs[0].secret);
    }

    #[test]
    fn test_auto_discovered_inputs_have_no_default() {
        let entry = UserServerEntry {
            command: Some("node".to_string()),
            args: None,
            env: Some(HashMap::from([(
                "API_KEY".to_string(),
                "${input:API_KEY}".to_string(),
            )])),
            url: None,
            headers: None,
            name: None,
            description: None,
            icon: None,
            alias: None,
            auth: None,
            metadata: None,
        };

        let (_, inputs) = entry.resolve_transport_and_inputs();

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].id, "API_KEY");
        assert_eq!(inputs[0].default, None);
    }

    #[test]
    fn test_input_default_serializes_roundtrip() {
        let input = InputDefinition {
            id: "PORT".to_string(),
            label: "Port".to_string(),
            r#type: "number".to_string(),
            required: false,
            secret: false,
            description: None,
            default: Some("8080".to_string()),
            placeholder: None,
            obtain_url: None,
            obtain_instructions: None,
        };

        let json = serde_json::to_string(&input).unwrap();
        let deserialized: InputDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "PORT");
        assert_eq!(deserialized.default, Some("8080".to_string()));
    }
}
