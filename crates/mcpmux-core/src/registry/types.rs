//! Core types for the MCP Server Registry

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Transport type for connecting to MCP servers
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TransportType {
    /// Local process via stdio
    #[default]
    Stdio,
    /// Remote server via Streamable HTTP (MCP spec)
    Http,
}

/// Authentication type for the server
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    /// No authentication required
    #[default]
    None,
    /// API key or token (user input, used in header/env)
    ApiKey,
    /// API key is optional (UI can show skip)
    OptionalApiKey,
    /// OAuth 2.0/2.1 (server implements protocol)
    Oauth,
}

/// Authentication configuration (simplified schema)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthConfig {
    /// Authentication type
    #[serde(rename = "type")]
    pub auth_type: AuthType,

    /// Instructions for obtaining credentials (for api_key type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Input field type for user configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum InputType {
    /// Single-line text input
    #[default]
    Text,
    /// Multi-line text input
    Textarea,
    /// Password/secret input (masked)
    Password,
    /// Boolean toggle
    Boolean,
    /// Numeric input
    Number,
    /// Selection from predefined options
    Select,
    /// File path selection
    FilePath,
    /// Directory path selection
    DirectoryPath,
    /// URL input with validation
    Url,
}

/// Select option for dropdown inputs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectOption {
    /// Display label
    pub label: String,
    /// Actual value
    pub value: String,
    /// Optional description
    pub description: Option<String>,
}

/// Guidance for obtaining an input value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObtainGuidance {
    /// URL where user can obtain this value
    pub url: Option<String>,
    /// Step-by-step instructions (markdown supported)
    pub instructions: Option<String>,
    /// Label for the "obtain" button
    pub button_label: Option<String>,
}

/// Conditional visibility dependency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputDependency {
    /// ID of input this depends on
    pub input: String,
    /// Required value of the dependency
    pub value: Option<String>,
    /// Show if dependency is not empty
    pub not_empty: Option<bool>,
}

/// Destination type for dynamic input injection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DestinationType {
    /// Inject as command-line argument
    Arg,
    /// Inject as environment variable
    Env,
    /// Inject as HTTP header
    Header,
    /// Use placeholder substitution (default)
    Placeholder,
}

/// Destination for dynamic input injection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputDestination {
    /// Destination type
    #[serde(rename = "type")]
    pub dest_type: DestinationType,
    /// Key name for env/header types
    pub key: Option<String>,
    /// Format template (e.g., "Bearer ${value}", "--log-level=${value}")
    pub format: Option<String>,
    /// Position for arg type
    pub position: Option<String>,
}

/// Input field definition for server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDefinition {
    /// Unique identifier for this input (used in placeholders like ${input:id})
    pub id: String,

    /// Human-readable label
    pub label: String,

    /// Detailed description/instructions
    pub description: Option<String>,

    /// Input type
    #[serde(default, rename = "type")]
    pub r#type: InputType,

    /// Whether this input is required
    #[serde(default)]
    pub required: bool,

    /// Whether this is a secret/password (should be stored securely)
    #[serde(default)]
    pub secret: bool,

    /// Default value
    pub default: Option<String>,

    /// Placeholder text
    pub placeholder: Option<String>,

    /// Validation regex pattern
    pub pattern: Option<String>,

    /// Error message when pattern validation fails
    pub pattern_error: Option<String>,

    /// Minimum length for text inputs
    pub min_length: Option<usize>,

    /// Maximum length for text inputs
    pub max_length: Option<usize>,

    /// Options for select type
    #[serde(default)]
    pub options: Vec<SelectOption>,

    /// Guidance for obtaining the value
    pub obtain: Option<ObtainGuidance>,

    /// Conditional visibility based on another input
    pub depends_on: Option<InputDependency>,

    /// Destination for dynamic injection (args, env, header)
    pub destination: Option<InputDestination>,
}

/// Publisher/Vendor information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Publisher {
    /// Publisher name (e.g., "Cloudflare", "Anthropic")
    pub name: String,

    /// Publisher primary domain (e.g., "cloudflare.com")
    pub domain: Option<String>,

    /// Publisher website
    pub url: Option<String>,

    /// Publisher logo URL
    pub logo_url: Option<String>,

    /// Whether the publisher is verified by registry maintainers
    #[serde(default)]
    pub verified: Option<bool>,

    /// Whether the domain ownership is verified (DNS TXT record)
    #[serde(default)]
    pub domain_verified: Option<bool>,

    /// Whether this is an official integration by the service provider
    #[serde(default)]
    pub official: Option<bool>,
}

/// Category for organizing servers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ServerCategory {
    /// Developer tools (GitHub, GitLab, etc.)
    DeveloperTools,
    /// Cloud platforms (AWS, GCP, Azure)
    Cloud,
    /// Databases and data storage
    Database,
    /// Communication (Slack, Discord, Email)
    Communication,
    /// Productivity (Notion, Google Docs)
    Productivity,
    /// Search and web browsing
    Search,
    /// AI and machine learning
    Ai,
    /// Security and authentication
    Security,
    /// Analytics and monitoring
    Analytics,
    /// File systems and storage
    FileSystem,
    /// Version control
    VersionControl,
    /// CI/CD and automation
    Automation,
    /// E-commerce and payments
    Ecommerce,
    /// Social media
    Social,
    /// Finance and banking
    Finance,
    /// Other/uncategorized
    Other,
    /// Custom category
    #[serde(untagged)]
    Custom(String),
}

/// Quality/trust indicators for a server
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityIndicators {
    /// Official integration by the service provider
    #[serde(default)]
    pub official: bool,

    /// Verified by the registry maintainers
    #[serde(default)]
    pub verified: bool,

    /// Featured/recommended
    #[serde(default)]
    pub featured: bool,

    /// Community rating (0-5)
    pub rating: Option<f32>,

    /// Number of installs/users
    pub install_count: Option<u64>,

    /// GitHub stars (if open source)
    pub github_stars: Option<u64>,
}

/// Links related to the server
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerLinks {
    /// Homepage URL
    pub homepage: Option<String>,

    /// Documentation URL
    pub documentation: Option<String>,

    /// Source repository URL
    pub repository: Option<String>,

    /// Issue tracker URL
    pub issues: Option<String>,

    /// Changelog URL
    pub changelog: Option<String>,

    /// Support URL or email
    pub support: Option<String>,
}

/// Platform compatibility
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    MacOS,
    Linux,
    All,
}

/// Environment variable definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVarDefinition {
    /// Environment variable name
    pub name: String,

    /// Description
    pub description: Option<String>,

    /// Whether required
    #[serde(default)]
    pub required: bool,

    /// Whether this is a secret
    #[serde(default)]
    pub secret: bool,

    /// Default value (use ${input:xxx} for user input)
    pub default: Option<String>,

    /// Reference to input definition
    pub input_ref: Option<String>,
}

/// Header definition for HTTP-based servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderDefinition {
    /// Header name
    pub name: String,

    /// Header value (can use ${input:xxx} placeholders)
    pub value: String,

    /// Whether this is a secret header (e.g., Authorization)
    #[serde(default)]
    pub secret: bool,
}

/// Transport metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransportMetadata {
    /// Input definitions for this transport
    #[serde(default)]
    pub inputs: Vec<InputDefinition>,
}

/// Transport configuration for the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TransportConfig {
    /// STDIO connection (local process)
    Stdio {
        /// Command to execute
        command: String,

        /// Command arguments (can use ${input:xxx} placeholders)
        #[serde(default)]
        args: Vec<String>,

        /// Environment variables
        #[serde(default)]
        env: HashMap<String, String>,

        /// Working directory
        cwd: Option<String>,

        /// Transport metadata (inputs, etc.)
        #[serde(default)]
        metadata: TransportMetadata,
    },

    /// HTTP connection (Streamable HTTP)
    Http {
        /// Server URL (can use ${input:xxx} placeholders)
        url: String,

        /// HTTP headers
        #[serde(default)]
        headers: HashMap<String, String>,

        /// Transport metadata (inputs, etc.)
        #[serde(default)]
        metadata: TransportMetadata,
    },
}

impl TransportConfig {
    /// Get the transport type
    pub fn transport_type(&self) -> TransportType {
        match self {
            Self::Stdio { .. } => TransportType::Stdio,
            Self::Http { .. } => TransportType::Http,
        }
    }
}
