//! Config export engine for generating client configuration files.
//!
//! Generates MCP configuration in formats compatible with:
//! - Cursor (mcp.json)
//! - VS Code Continue (.continuerc / settings.json)
//! - Claude Desktop (claude_desktop_config.json)
//!
//! This module works with:
//! - `RegistryServer` - Server definition from registry (transport config, inputs)
//! - `InstalledServer` - User's installation with input values

use crate::domain::InstalledServer;
use crate::registry::{TransportConfig, RegistryServer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Client configuration format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// Cursor-style configuration
    Cursor,
    /// VS Code Continue extension format
    VsCodeContinue,
    /// Claude Desktop format
    ClaudeDesktop,
}

impl ConfigFormat {
    /// Get file extension for this format
    pub fn file_extension(&self) -> &'static str {
        ".json"
    }

    /// Get default config file path for this format
    pub fn default_path(&self) -> Option<PathBuf> {
        match self {
            ConfigFormat::Cursor => {
                // ~/.cursor/mcp.json or project-level .cursor/mcp.json
                dirs::home_dir().map(|h| h.join(".cursor").join("mcp.json"))
            }
            ConfigFormat::VsCodeContinue => {
                // ~/.continue/config.json
                dirs::home_dir().map(|h| h.join(".continue").join("config.json"))
            }
            ConfigFormat::ClaudeDesktop => {
                // Platform-specific
                #[cfg(target_os = "macos")]
                {
                    dirs::home_dir().map(|h| {
                        h.join("Library")
                            .join("Application Support")
                            .join("Claude")
                            .join("claude_desktop_config.json")
                    })
                }
                #[cfg(target_os = "windows")]
                {
                    dirs::config_dir()
                        .map(|c| c.join("Claude").join("claude_desktop_config.json"))
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    dirs::config_dir()
                        .map(|c| c.join("Claude").join("claude_desktop_config.json"))
                }
            }
        }
    }
}

/// Cursor MCP configuration format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorConfig {
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, CursorServerConfig>,
}

/// Server config for Cursor format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CursorServerConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
    },
}

/// VS Code Continue configuration format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<ContinueExperimental>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueExperimental {
    #[serde(rename = "modelContextProtocol")]
    pub model_context_protocol: ContinueMcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueMcp {
    pub servers: HashMap<String, ContinueServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinueServerConfig {
    pub transport: ContinueTransport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContinueTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
    },
}

/// Claude Desktop configuration format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, ClaudeServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClaudeServerConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        transport: String,
    },
}

/// Configuration exporter
pub struct ConfigExporter {
    /// Credential resolver function
    credential_resolver:
        Option<Box<dyn Fn(&str, &Uuid) -> Option<HashMap<String, String>> + Send + Sync>>,
}

impl Default for ConfigExporter {
    fn default() -> Self {
        Self::new()
    }
}

/// A resolved server ready for export (registry + installed data combined)
pub struct ResolvedServer {
    /// Server ID from registry
    pub server_id: String,
    /// Resolved transport config
    pub transport: ResolvedTransport,
}

/// Resolved transport with placeholders replaced
pub enum ResolvedTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        headers: HashMap<String, String>,
    },
}

impl ConfigExporter {
    /// Create a new config exporter
    pub fn new() -> Self {
        Self {
            credential_resolver: None,
        }
    }

    /// Set the credential resolver function
    pub fn with_credential_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&str, &Uuid) -> Option<HashMap<String, String>> + Send + Sync + 'static,
    {
        self.credential_resolver = Some(Box::new(resolver));
        self
    }

    /// Resolve credentials for a server in a space
    fn resolve_credentials(&self, server_id: &str, space_id: &Uuid) -> HashMap<String, String> {
        self.credential_resolver
            .as_ref()
            .and_then(|r| r(server_id, space_id))
            .unwrap_or_default()
    }

    /// Resolve a string by replacing ${input:id} placeholders with actual values
    fn resolve_placeholders(template: &str, input_values: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        for (key, value) in input_values {
            let placeholder = format!("${{input:{}}}", key);
            result = result.replace(&placeholder, value);
        }
        result
    }

    /// Resolve a registry server with installed values to a transport config
    pub fn resolve_server(
        registry_server: &RegistryServer,
        installed: &InstalledServer,
        credentials: &HashMap<String, String>,
    ) -> ResolvedServer {
        let transport = match &registry_server.transport {
            TransportConfig::Stdio { command, args, env, .. } => {
                let resolved_command = Self::resolve_placeholders(command, &installed.input_values);
                let resolved_args: Vec<String> = args
                    .iter()
                    .map(|a| Self::resolve_placeholders(a, &installed.input_values))
                    .collect();
                
                let mut resolved_env: HashMap<String, String> = env
                    .iter()
                    .map(|(k, v)| {
                        (k.clone(), Self::resolve_placeholders(v, &installed.input_values))
                    })
                    .collect();
                
                // Merge in credentials
                resolved_env.extend(credentials.clone());

                ResolvedTransport::Stdio {
                    command: resolved_command,
                    args: resolved_args,
                    env: resolved_env,
                }
            }
            TransportConfig::Http { url, headers, .. } => {
                let resolved_url = Self::resolve_placeholders(url, &installed.input_values);
                let resolved_headers: HashMap<String, String> = headers
                    .iter()
                    .map(|(k, v)| {
                        (k.clone(), Self::resolve_placeholders(v, &installed.input_values))
                    })
                    .collect();

                ResolvedTransport::Http {
                    url: resolved_url,
                    headers: resolved_headers,
                }
            }
        };

        ResolvedServer {
            server_id: registry_server.id.clone(),
            transport,
        }
    }

    /// Generate Cursor format config
    pub fn to_cursor(&self, servers: &[ResolvedServer]) -> CursorConfig {
        let mut mcp_servers = HashMap::new();

        for server in servers {
            let server_config = match &server.transport {
                ResolvedTransport::Stdio { command, args, env } => {
                    CursorServerConfig::Stdio {
                        command: command.clone(),
                        args: args.clone(),
                        env: env.clone(),
                    }
                }
                ResolvedTransport::Http { url, .. } => CursorServerConfig::Http { url: url.clone() },
            };

            mcp_servers.insert(server.server_id.clone(), server_config);
        }

        CursorConfig { mcp_servers }
    }

    /// Generate VS Code Continue format config
    pub fn to_continue(&self, servers: &[ResolvedServer]) -> ContinueConfig {
        let mut mcp_servers = HashMap::new();

        for server in servers {
            let transport = match &server.transport {
                ResolvedTransport::Stdio { command, args, env } => {
                    ContinueTransport::Stdio {
                        command: command.clone(),
                        args: args.clone(),
                        env: env.clone(),
                    }
                }
                ResolvedTransport::Http { url, .. } => ContinueTransport::Http { url: url.clone() },
            };

            mcp_servers.insert(
                server.server_id.clone(),
                ContinueServerConfig { transport },
            );
        }

        ContinueConfig {
            experimental: Some(ContinueExperimental {
                model_context_protocol: ContinueMcp {
                    servers: mcp_servers,
                },
            }),
        }
    }

    /// Generate Claude Desktop format config
    pub fn to_claude_desktop(&self, servers: &[ResolvedServer]) -> ClaudeDesktopConfig {
        let mut mcp_servers = HashMap::new();

        for server in servers {
            let server_config = match &server.transport {
                ResolvedTransport::Stdio { command, args, env } => {
                    ClaudeServerConfig::Stdio {
                        command: command.clone(),
                        args: args.clone(),
                        env: env.clone(),
                    }
                }
                // Claude Desktop uses HTTP for remote connections
                ResolvedTransport::Http { url, .. } => {
                    ClaudeServerConfig::Http {
                        url: url.clone(),
                        transport: "http".to_string(),
                    }
                }
            };

            mcp_servers.insert(server.server_id.clone(), server_config);
        }

        ClaudeDesktopConfig { mcp_servers }
    }

    /// Export config to JSON string
    pub fn export_json(
        &self,
        format: ConfigFormat,
        servers: &[ResolvedServer],
    ) -> Result<String, serde_json::Error> {
        match format {
            ConfigFormat::Cursor => {
                let config = self.to_cursor(servers);
                serde_json::to_string_pretty(&config)
            }
            ConfigFormat::VsCodeContinue => {
                let config = self.to_continue(servers);
                serde_json::to_string_pretty(&config)
            }
            ConfigFormat::ClaudeDesktop => {
                let config = self.to_claude_desktop(servers);
                serde_json::to_string_pretty(&config)
            }
        }
    }

    /// Resolve multiple servers from registry and installed data
    pub fn resolve_servers(
        &self,
        registry_servers: &HashMap<String, RegistryServer>,
        installed_servers: &[InstalledServer],
        space_id: &Uuid,
    ) -> Vec<ResolvedServer> {
        installed_servers
            .iter()
            .filter(|installed| installed.enabled)
            .filter_map(|installed| {
                registry_servers.get(&installed.server_id).map(|registry| {
                    let credentials = self.resolve_credentials(&installed.server_id, space_id);
                    Self::resolve_server(registry, installed, &credentials)
                })
            })
            .collect()
    }
}

/// Export result
#[derive(Debug, Clone)]
pub struct ExportResult {
    /// Generated JSON content
    pub content: String,
    /// Default file path for this format
    pub default_path: Option<PathBuf>,
    /// Format used
    pub format: ConfigFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_resolved_server(id: &str, command: &str, args: Vec<&str>) -> ResolvedServer {
        ResolvedServer {
            server_id: id.to_string(),
            transport: ResolvedTransport::Stdio {
                command: command.to_string(),
                args: args.into_iter().map(String::from).collect(),
                env: HashMap::new(),
            },
        }
    }

    #[test]
    fn test_cursor_config() {
        let servers = vec![create_test_resolved_server(
            "io.github.github/github-mcp-server",
            "npx",
            vec!["-y", "@modelcontextprotocol/server-github"],
        )];

        let exporter = ConfigExporter::new();
        let config = exporter.to_cursor(&servers);

        assert!(config
            .mcp_servers
            .contains_key("io.github.github/github-mcp-server"));
    }

    #[test]
    fn test_continue_config() {
        let servers = vec![create_test_resolved_server(
            "io.github.modelcontextprotocol/filesystem",
            "npx",
            vec!["-y", "@modelcontextprotocol/server-filesystem"],
        )];

        let exporter = ConfigExporter::new();
        let config = exporter.to_continue(&servers);

        assert!(config.experimental.is_some());
        let exp = config.experimental.unwrap();
        assert!(exp
            .model_context_protocol
            .servers
            .contains_key("io.github.modelcontextprotocol/filesystem"));
    }

    #[test]
    fn test_claude_desktop_config() {
        let servers = vec![create_test_resolved_server(
            "io.github.modelcontextprotocol/memory",
            "npx",
            vec!["-y", "@modelcontextprotocol/server-memory"],
        )];

        let exporter = ConfigExporter::new();
        let config = exporter.to_claude_desktop(&servers);

        assert!(config
            .mcp_servers
            .contains_key("io.github.modelcontextprotocol/memory"));
    }

    #[test]
    fn test_resolve_placeholders() {
        let template = "https://api.example.com/${input:api_key}/v1";
        let mut input_values = HashMap::new();
        input_values.insert("api_key".to_string(), "my-secret-key".to_string());

        let resolved = ConfigExporter::resolve_placeholders(template, &input_values);
        assert_eq!(resolved, "https://api.example.com/my-secret-key/v1");
    }

    #[test]
    fn test_resolve_multiple_placeholders() {
        let template = "${input:command} --token ${input:token}";
        let mut input_values = HashMap::new();
        input_values.insert("command".to_string(), "my-cli".to_string());
        input_values.insert("token".to_string(), "abc123".to_string());

        let resolved = ConfigExporter::resolve_placeholders(template, &input_values);
        assert_eq!(resolved, "my-cli --token abc123");
    }
}
