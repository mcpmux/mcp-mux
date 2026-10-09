use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Whether `definition` is still the one the user was shown, by its
/// transport (what runs, or where it connects). `None` means the caller
/// showed nothing to compare against. A registry refresh between showing
/// and installing could otherwise install a different command.
pub fn transport_matches_shown(
    definition: &ServerDefinition,
    shown: Option<&serde_json::Value>,
) -> bool {
    let Some(shown) = shown else {
        return true;
    };
    serde_json::to_value(&definition.transport).is_ok_and(|current| &current == shown)
}

/// Error shown when [`transport_matches_shown`] fails.
pub const DEFINITION_CHANGED: &str =
    "This server's definition changed since it was shown; review it again before installing";

/// The canonical internal representation for ALL servers (Unified Runtime Model).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDefinition {
    /// Unique identifier (e.g., "com.anthropic.github")
    pub id: String,

    /// Display name
    pub name: String,

    /// Optional description
    pub description: Option<String>,

    /// Optional short alias for tool prefixing (e.g., "gh")
    pub alias: Option<String>,

    /// Authentication configuration
    pub auth: Option<AuthConfig>,

    /// Optional icon (emoji or URL)
    pub icon: Option<String>,

    /// Self-contained transport configuration (includes inputs!)
    pub transport: TransportConfig,

    /// Registry categorization
    #[serde(default)]
    pub categories: Vec<String>,

    /// Publisher info
    pub publisher: Option<PublisherInfo>,

    /// Where this server came from
    #[serde(default)]
    pub source: ServerSource,

    /// Visual badges for trust and discovery (v2.1)
    #[serde(default)]
    pub badges: Vec<Badge>,

    /// Where the server runs: local, remote, or hybrid (v2.1)
    #[serde(default)]
    pub hosting_type: HostingType,

    /// SPDX license identifier (v2.1)
    pub license: Option<String>,

    /// URL to full license text (v2.1)
    pub license_url: Option<String>,

    /// Installation metadata (v2.1)
    pub installation: Option<Installation>,

    /// MCP capabilities (v2.1)
    pub capabilities: Option<Capabilities>,

    /// Sponsorship information (v2.1)
    pub sponsored: Option<Sponsored>,

    /// Rich media content (v2.1)
    pub media: Option<Media>,

    /// Changelog URL (v2.1)
    pub changelog_url: Option<String>,
    // NOTE: Runtime state like 'enabled' is NOT stored here.
    // It is injected at the application layer by merging with DB state.
}

impl ServerDefinition {
    pub fn requires_oauth(&self) -> bool {
        matches!(self.auth, Some(AuthConfig::Oauth))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
pub enum ServerSource {
    /// Loaded from a user-defined JSON file in the spaces directory
    UserSpace {
        space_id: String,
        file_path: PathBuf,
    },
    /// Loaded from the bundled registry.json (Legacy/Default)
    #[default]
    Bundled,
    /// Loaded from a remote or custom registry (API, NPM, etc.)
    Registry { url: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default)]
        metadata: TransportMetadata,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        metadata: TransportMetadata,
    },
}

impl TransportConfig {
    /// Get metadata reference for this transport
    pub fn metadata(&self) -> &TransportMetadata {
        match self {
            TransportConfig::Stdio { metadata, .. } => metadata,
            TransportConfig::Http { metadata, .. } => metadata,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransportMetadata {
    /// Inputs required by this transport
    #[serde(default)]
    pub inputs: Vec<InputDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDefinition {
    pub id: String,
    pub label: String,
    #[serde(default = "default_input_type")]
    pub r#type: String, // "text", "number", "boolean", "url", etc. Masking controlled by `secret` field.
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    pub description: Option<String>,
    pub default: Option<String>,
    pub placeholder: Option<String>,

    // Additional helpful metadata for acquiring credentials
    pub obtain_url: Option<String>,
    pub obtain_instructions: Option<String>,
}

fn default_input_type() -> String {
    "text".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    None,
    ApiKey {
        instructions: Option<String>,
    },
    OptionalApiKey {
        instructions: Option<String>,
    },
    /// HTTP Basic authentication (username/password).
    /// The user supplies credentials directly; the transport/inputs carry
    /// them via environment variables or headers.
    Basic {
        instructions: Option<String>,
    },
    Oauth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherInfo {
    pub name: String,
    pub domain: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub official: bool,
}

// ============================================
// Schema v2.1 Additions
// ============================================

/// Visual badge indicators for server listings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Badge {
    Official,
    Verified,
    Featured,
    Sponsored,
    Popular,
}

/// Where the server runs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostingType {
    #[default]
    Local,
    Remote,
    Hybrid,
}

/// Installation complexity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallDifficulty {
    Easy,
    Moderate,
    Advanced,
}

/// Installation metadata for user guidance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Installation {
    pub difficulty: Option<InstallDifficulty>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    pub estimated_time: Option<String>,
}

/// MCP capabilities with read-only mode support
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Capabilities {
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub resources: bool,
    #[serde(default)]
    pub prompts: bool,
    #[serde(default)]
    pub read_only_mode: bool,
}

/// Sponsorship information for commercial listings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sponsored {
    #[serde(default)]
    pub enabled: bool,
    pub sponsor_name: Option<String>,
    pub sponsor_url: Option<String>,
    pub sponsor_logo: Option<String>,
    pub campaign_id: Option<String>,
}

/// Rich media content for enhanced discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    #[serde(default)]
    pub screenshots: Vec<String>,
    pub demo_video: Option<String>,
    pub banner: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_definition_that_was_shown_is_installed() {
        let definition: ServerDefinition = serde_json::from_value(serde_json::json!({
            "id": "srv",
            "name": "Server",
            "transport": {"type": "stdio", "command": "npx", "args": ["-y", "pkg"]}
        }))
        .unwrap();
        let shown = serde_json::to_value(&definition.transport).unwrap();
        assert!(transport_matches_shown(&definition, Some(&shown)));
        assert!(transport_matches_shown(&definition, None));

        let mut changed = shown.clone();
        changed["args"] = serde_json::json!(["-y", "other-pkg"]);
        assert!(!transport_matches_shown(&definition, Some(&changed)));
    }

    /// Regression test: a registry server with `auth.type = "basic"` (e.g.
    /// Darkmoon) must deserialize instead of failing the entire bundle parse.
    /// See: registry API returns servers with `auth.type: "basic"` which was
    /// not a recognized variant of `AuthConfig`, causing
    /// `Failed to parse registry bundle JSON` and forcing the app offline.
    #[test]
    fn test_auth_basic_deserializes() {
        let json = r#"{
            "id": "io.github-ascit31-darkmoon-mcp-npx",
            "name": "Darkmoon (npx)",
            "description": "Darkmoon MCP server",
            "transport": {
                "type": "stdio",
                "command": "npx"
            },
            "auth": {
                "type": "basic",
                "instructions": "Use the username and password of a user on your own Darkmoon Pro dashboard."
            }
        }"#;

        let def: ServerDefinition = serde_json::from_str(json)
            .expect("ServerDefinition with basic auth should deserialize without error");

        assert!(
            matches!(def.auth, Some(AuthConfig::Basic { .. })),
            "expected AuthConfig::Basic, got {:?}",
            def.auth
        );
    }

    /// Every auth type survives serialize -> deserialize (the disk cache and
    /// the frontend both see the serialized form), keeping `instructions`.
    #[test]
    fn test_auth_variants_round_trip() {
        let cases = [
            (AuthConfig::None, "none"),
            (
                AuthConfig::ApiKey {
                    instructions: Some("i".into()),
                },
                "api_key",
            ),
            (
                AuthConfig::OptionalApiKey {
                    instructions: Some("i".into()),
                },
                "optional_api_key",
            ),
            (
                AuthConfig::Basic {
                    instructions: Some("i".into()),
                },
                "basic",
            ),
            (AuthConfig::Oauth, "oauth"),
        ];
        for (auth, tag) in cases {
            let json = serde_json::to_value(&auth).unwrap();
            assert_eq!(json["type"], tag);

            let back: AuthConfig = serde_json::from_value(json).unwrap();
            let instructions = |a: &AuthConfig| match a {
                AuthConfig::ApiKey { instructions }
                | AuthConfig::OptionalApiKey { instructions }
                | AuthConfig::Basic { instructions } => instructions.clone(),
                AuthConfig::None | AuthConfig::Oauth => None,
            };
            assert_eq!(
                std::mem::discriminant(&back),
                std::mem::discriminant(&auth),
                "{tag} changed variant"
            );
            assert_eq!(
                instructions(&back),
                instructions(&auth),
                "{tag} lost instructions"
            );
        }
    }
}
