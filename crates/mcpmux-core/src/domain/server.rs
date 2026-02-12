use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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
    pub r#type: String, // "text", "password", etc.
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
    ApiKey { instructions: Option<String> },
    OptionalApiKey { instructions: Option<String> },
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
