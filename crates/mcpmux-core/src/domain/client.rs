//! Client entity - AI clients that connect to McpMux
//!
//! A Client is the *identity* an approved connection uses (Cursor, VS Code,
//! Claude Desktop, etc.). Routing is driven entirely by WorkspaceBinding +
//! the session's Space; per-client FeatureSet grants and Space/FS pins no
//! longer exist.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Client represents an AI client (Cursor, VS Code, Claude Desktop, ...)
/// that has been approved to connect to the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    /// Unique identifier
    pub id: Uuid,

    /// Human-readable name
    pub name: String,

    /// Client type (cursor, vscode, claude, etc.)
    pub client_type: String,

    /// Access key for authentication (local only, never synced)
    #[serde(skip)]
    pub access_key: Option<String>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Last seen timestamp
    pub last_seen: Option<DateTime<Utc>>,
}

impl Client {
    /// Create a new client
    pub fn new(name: impl Into<String>, client_type: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            client_type: client_type.into(),
            access_key: None,
            created_at: now,
            updated_at: now,
            last_seen: None,
        }
    }

    /// Create a Cursor client
    pub fn cursor() -> Self {
        Self::new("Cursor", "cursor")
    }

    /// Create a VS Code client
    pub fn vscode() -> Self {
        Self::new("VS Code", "vscode")
    }

    /// Create a Claude Desktop client
    pub fn claude_desktop() -> Self {
        Self::new("Claude Desktop", "claude")
    }

    /// Generate a new access key
    pub fn generate_access_key(&mut self) {
        self.access_key = Some(format!("mcp_{}", Uuid::new_v4().simple()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = Client::cursor();
        assert_eq!(client.name, "Cursor");
        assert_eq!(client.client_type, "cursor");
    }

    #[test]
    fn test_access_key_generation() {
        let mut client = Client::vscode();
        assert!(client.access_key.is_none());
        client.generate_access_key();
        let key = client.access_key.as_ref().expect("key was generated");
        assert!(key.starts_with("mcp_"));
    }
}
