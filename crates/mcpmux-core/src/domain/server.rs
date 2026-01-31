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
        file_path: PathBuf 
    },
    /// Loaded from the bundled registry.json (Legacy/Default)
    #[default]
    Bundled,
    /// Loaded from a remote or custom registry (API, NPM, etc.)
    Registry {
        url: String,
        name: String,
    },
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
