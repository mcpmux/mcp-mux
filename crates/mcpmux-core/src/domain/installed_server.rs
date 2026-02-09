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

    #[test]
    fn test_new_server_has_empty_custom_fields() {
        let server = InstalledServer::new("space_default", "test-server");

        assert!(
            server.env_overrides.is_empty(),
            "New server should have empty env_overrides"
        );
        assert!(
            server.args_append.is_empty(),
            "New server should have empty args_append"
        );
        assert!(
            server.extra_headers.is_empty(),
            "New server should have empty extra_headers"
        );
    }

    #[test]
    fn test_env_overrides_can_be_set() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server
            .env_overrides
            .insert("NODE_ENV".to_string(), "production".to_string());
        server
            .env_overrides
            .insert("DEBUG".to_string(), "true".to_string());

        assert_eq!(server.env_overrides.len(), 2);
        assert_eq!(
            server.env_overrides.get("NODE_ENV"),
            Some(&"production".to_string())
        );
        assert_eq!(server.env_overrides.get("DEBUG"), Some(&"true".to_string()));
    }

    #[test]
    fn test_args_append_can_be_set() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server.args_append = vec![
            "--verbose".to_string(),
            "--port".to_string(),
            "8080".to_string(),
        ];

        assert_eq!(server.args_append.len(), 3);
        assert_eq!(server.args_append[0], "--verbose");
        assert_eq!(server.args_append[1], "--port");
        assert_eq!(server.args_append[2], "8080");
    }

    #[test]
    fn test_extra_headers_can_be_set() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server
            .extra_headers
            .insert("Authorization".to_string(), "Bearer token123".to_string());
        server
            .extra_headers
            .insert("X-Custom-Header".to_string(), "custom-value".to_string());

        assert_eq!(server.extra_headers.len(), 2);
        assert_eq!(
            server.extra_headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(
            server.extra_headers.get("X-Custom-Header"),
            Some(&"custom-value".to_string())
        );
    }

    #[test]
    fn test_custom_fields_serialize_deserialize() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server
            .env_overrides
            .insert("KEY".to_string(), "value".to_string());
        server.args_append = vec!["--flag".to_string()];
        server
            .extra_headers
            .insert("X-Test".to_string(), "test".to_string());

        // Serialize
        let json = serde_json::to_string(&server).expect("Failed to serialize");

        // Deserialize
        let deserialized: InstalledServer =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(
            deserialized.env_overrides.get("KEY"),
            Some(&"value".to_string())
        );
        assert_eq!(deserialized.args_append, vec!["--flag".to_string()]);
        assert_eq!(
            deserialized.extra_headers.get("X-Test"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_custom_fields_default_on_deserialize() {
        // JSON without custom fields should deserialize with empty defaults
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "space_id": "space_default",
            "server_id": "test-server",
            "server_name": null,
            "cached_definition": null,
            "input_values": {},
            "enabled": false,
            "oauth_connected": false,
            "source": {"type": "registry"},
            "created_at": "2025-01-01T00:00:00Z",
            "updated_at": "2025-01-01T00:00:00Z"
        }"#;

        let server: InstalledServer = serde_json::from_str(json).expect("Failed to deserialize");

        assert!(server.env_overrides.is_empty());
        assert!(server.args_append.is_empty());
        assert!(server.extra_headers.is_empty());
    }

    #[test]
    fn test_env_overrides_empty_key_allowed() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server
            .env_overrides
            .insert("".to_string(), "value".to_string());

        assert_eq!(server.env_overrides.get(""), Some(&"value".to_string()));

        // Serialize and deserialize
        let json = serde_json::to_string(&server).expect("serialize");
        let deserialized: InstalledServer = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            deserialized.env_overrides.get(""),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_env_overrides_overwrite_existing_key() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server
            .env_overrides
            .insert("KEY".to_string(), "first".to_string());
        server
            .env_overrides
            .insert("KEY".to_string(), "second".to_string());

        assert_eq!(server.env_overrides.len(), 1);
        assert_eq!(server.env_overrides.get("KEY"), Some(&"second".to_string()));
    }

    #[test]
    fn test_special_characters_in_values() {
        let mut server = InstalledServer::new("space_default", "test-server");
        // Env var with special chars
        server.env_overrides.insert(
            "PATH_WITH=EQUALS".to_string(),
            "value with spaces & \"quotes\" and \nnewlines".to_string(),
        );
        // Args with special chars
        server.args_append = vec![
            "--config=/path/to/file".to_string(),
            "arg with spaces".to_string(),
            "unicode: 日本語".to_string(),
        ];
        // Header with special chars
        server.extra_headers.insert(
            "X-Special".to_string(),
            "value/with:colons and;semicolons".to_string(),
        );

        let json = serde_json::to_string(&server).expect("serialize");
        let deserialized: InstalledServer = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(
            deserialized.env_overrides.get("PATH_WITH=EQUALS"),
            Some(&"value with spaces & \"quotes\" and \nnewlines".to_string())
        );
        assert_eq!(deserialized.args_append[2], "unicode: 日本語");
        assert_eq!(
            deserialized.extra_headers.get("X-Special"),
            Some(&"value/with:colons and;semicolons".to_string())
        );
    }

    #[test]
    fn test_clear_custom_fields_to_empty() {
        let mut server = InstalledServer::new("space_default", "test-server");
        // Set values
        server
            .env_overrides
            .insert("KEY".to_string(), "value".to_string());
        server.args_append = vec!["--flag".to_string()];
        server
            .extra_headers
            .insert("X-Test".to_string(), "test".to_string());

        assert!(!server.env_overrides.is_empty());
        assert!(!server.args_append.is_empty());
        assert!(!server.extra_headers.is_empty());

        // Clear all fields
        server.env_overrides = HashMap::new();
        server.args_append = Vec::new();
        server.extra_headers = HashMap::new();

        assert!(server.env_overrides.is_empty());
        assert!(server.args_append.is_empty());
        assert!(server.extra_headers.is_empty());

        // Verify empty fields serialize/deserialize correctly
        let json = serde_json::to_string(&server).expect("serialize");
        let deserialized: InstalledServer = serde_json::from_str(&json).expect("deserialize");
        assert!(deserialized.env_overrides.is_empty());
        assert!(deserialized.args_append.is_empty());
        assert!(deserialized.extra_headers.is_empty());
    }

    #[test]
    fn test_large_args_list() {
        let mut server = InstalledServer::new("space_default", "test-server");
        server.args_append = (0..100).map(|i| format!("--arg-{}", i)).collect();

        assert_eq!(server.args_append.len(), 100);

        let json = serde_json::to_string(&server).expect("serialize");
        let deserialized: InstalledServer = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.args_append.len(), 100);
        assert_eq!(deserialized.args_append[99], "--arg-99");
    }
}
