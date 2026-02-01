//! InstalledServer entity - per-space server installation

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

use super::ServerDefinition;

/// Tracks how a server was installed (for sync/cleanup decisions)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstallationSource {
    /// Installed from Registry via UI
    #[default]
    Registry,
    /// Auto-synced from user space JSON file
    UserConfig {
        /// Path to the JSON file this server came from
        file_path: PathBuf,
    },
    /// Manually entered via "Add Server" UI (not from any file)
    ManualEntry,
}

/// Installed server - represents a server installation in a space
///
/// This is the **single source of truth** for all connectable servers,
/// regardless of whether they came from:
/// - Registry (bundled or API)
/// - User space JSON config files
/// - Manual entry via UI
///
/// The server definition is cached at install time for offline operation.
/// This entity stores both the cached definition and user-specific configuration.
///
/// Note: Connection status is NOT stored here - it's runtime-only state
/// managed by ServerManager and communicated via events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledServer {
    /// Unique installation ID
    pub id: Uuid,

    /// Space this server is installed in
    pub space_id: String,

    /// Server ID (e.g., "com.cloudflare/bindings-mcp" or "my-custom-server")
    pub server_id: String,

    /// Server display name (cached from definition for offline display)
    pub server_name: Option<String>,

    /// Cached server definition (JSON) for offline operation
    /// Contains transport config, auth requirements, etc.
    pub cached_definition: Option<String>,

    /// User's input values for this installation
    /// Keys are input IDs, values are user-provided credentials/config
    pub input_values: HashMap<String, String>,

    /// Whether this installation is enabled (will auto-connect on gateway start)
    pub enabled: bool,

    /// Environment variable overrides beyond inputs
    #[serde(default)]
    pub env_overrides: HashMap<String, String>,

    /// Extra arguments to append to command
    #[serde(default)]
    pub args_append: Vec<String>,

    /// Extra HTTP headers for HTTP transports (e.g., custom auth headers)
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,

    /// Whether OAuth authentication has been completed
    pub oauth_connected: bool,

    /// How this server was installed (for sync/cleanup decisions)
    #[serde(default)]
    pub source: InstallationSource,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl InstalledServer {
    /// Create a new installed server
    ///
    /// Servers are disabled by default and must be explicitly enabled by the user.
    pub fn new(space_id: impl Into<String>, server_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            space_id: space_id.into(),
            server_id: server_id.into(),
            server_name: None,
            cached_definition: None,
            input_values: HashMap::new(),
            enabled: false, // Disabled by default - user must explicitly enable
            env_overrides: HashMap::new(),
            args_append: Vec::new(),
            extra_headers: HashMap::new(),
            oauth_connected: false,
            source: InstallationSource::default(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Create with a cached server definition (for offline operation)
    pub fn with_definition(mut self, definition: &ServerDefinition) -> Self {
        self.server_name = Some(definition.name.clone());
        self.cached_definition = serde_json::to_string(definition).ok();
        self
    }

    /// Get the cached server definition (deserialized)
    pub fn get_definition(&self) -> Option<ServerDefinition> {
        self.cached_definition
            .as_ref()
            .and_then(|json| serde_json::from_str(json).ok())
    }

    /// Get display name (from cached definition or server_id fallback)
    pub fn display_name(&self) -> &str {
        self.server_name.as_deref().unwrap_or_else(|| {
            self.server_id
                .split('/')
                .next_back()
                .unwrap_or(&self.server_id)
        })
    }

    /// Set input values
    pub fn with_inputs(mut self, inputs: HashMap<String, String>) -> Self {
        self.input_values = inputs;
        self
    }

    /// Set a single input value
    pub fn with_input(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.input_values.insert(key.into(), value.into());
        self
    }

    /// Set enabled state
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set installation source
    pub fn with_source(mut self, source: InstallationSource) -> Self {
        self.source = source;
        self
    }

    /// Update OAuth connected state
    pub fn set_oauth_connected(&mut self, connected: bool) {
        self.oauth_connected = connected;
        self.updated_at = Utc::now();
    }

    /// Check if this server came from a user config file
    pub fn is_from_user_config(&self) -> bool {
        matches!(self.source, InstallationSource::UserConfig { .. })
    }

    /// Get the source file path if this server came from a user config
    pub fn source_file_path(&self) -> Option<&PathBuf> {
        match &self.source {
            InstallationSource::UserConfig { file_path } => Some(file_path),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_installed_server() {
        let server = InstalledServer::new("space_default", "com.cloudflare/docs-mcp");

        assert_eq!(server.space_id, "space_default");
        assert_eq!(server.server_id, "com.cloudflare/docs-mcp");
        assert!(!server.enabled, "New servers should be disabled by default");
    }

    #[test]
    fn test_with_enabled() {
        let server = InstalledServer::new("space_default", "test.server").with_enabled(true);

        assert!(
            server.enabled,
            "Server should be enabled after calling with_enabled(true)"
        );
    }

    #[test]
    fn test_with_inputs() {
        let server = InstalledServer::new("space_default", "io.github.github/github-mcp-server")
            .with_input("GITHUB_TOKEN", "ghp_xxxxx");

        assert_eq!(
            server.input_values.get("GITHUB_TOKEN"),
            Some(&"ghp_xxxxx".to_string())
        );
    }
}
