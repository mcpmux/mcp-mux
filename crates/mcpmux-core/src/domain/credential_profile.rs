//! CredentialProfile entity - central reusable credential storage
//!
//! A credential profile stores a named bundle of server input values/secrets
//! (e.g. "Work GitHub", "Personal Cloudflare") decoupled from any single Space.
//!
//! This enables:
//! - Reusing credentials across multiple Spaces without re-entry.
//! - Preserving credentials even when an MCP server is uninstalled from a Space.
//! - Single-point rotation of API keys and tokens.
//! - Managing multiple accounts for the same server.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Reusable credential profile stored in the central secrets store.
#[derive(Clone, Serialize, Deserialize)]
pub struct CredentialProfile {
    /// Unique profile ID
    pub id: Uuid,

    /// Human-friendly display name (e.g. "Work GitHub", "Personal Cloudflare")
    pub name: String,

    /// MCP server definition ID this profile belongs to (e.g. "com.github.mcp")
    pub server_id: String,

    /// Cached server display name for offline display (e.g. "GitHub")
    pub server_name: Option<String>,

    /// Secret/configuration values (plaintext at domain level, encrypted at storage level)
    pub values: HashMap<String, String>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Never prints raw secret values in debug logs so sensitive tokens are protected.
impl std::fmt::Debug for CredentialProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<&str> = self.values.keys().map(String::as_str).collect();
        keys.sort_unstable();
        f.debug_struct("CredentialProfile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("server_id", &self.server_id)
            .field("server_name", &self.server_name)
            .field("configured_keys", &keys)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl CredentialProfile {
    /// Create a new credential profile with a random UUID and current timestamps.
    pub fn new(
        name: impl Into<String>,
        server_id: impl Into<String>,
        values: HashMap<String, String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            server_id: server_id.into(),
            server_name: None,
            values,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set an optional cached server display name.
    pub fn with_server_name(mut self, server_name: impl Into<String>) -> Self {
        self.server_name = Some(server_name.into());
        self
    }

    /// Convert into a summary, calculating configured keys and assigning usage count.
    pub fn to_summary(&self, usage_count: usize) -> CredentialProfileSummary {
        let mut input_keys: Vec<String> = self.values.keys().cloned().collect();
        input_keys.sort();
        CredentialProfileSummary {
            id: self.id,
            name: self.name.clone(),
            server_id: self.server_id.clone(),
            server_name: self.server_name.clone(),
            input_keys,
            usage_count,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Non-sensitive summary for listing credential profiles without exposing secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialProfileSummary {
    /// Unique profile ID
    pub id: Uuid,

    /// Human-friendly display name
    pub name: String,

    /// MCP server definition ID
    pub server_id: String,

    /// Cached server display name
    pub server_name: Option<String>,

    /// Names of configured input keys (e.g. `["api_key", "token"]`)
    pub input_keys: Vec<String>,

    /// Number of installed servers currently referencing this profile
    pub usage_count: usize,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_profile_creation_and_summary() {
        let mut values = HashMap::new();
        values.insert("api_key".to_string(), "secret_123".to_string());
        values.insert("org_id".to_string(), "org_456".to_string());

        let profile = CredentialProfile::new("Work Cloudflare", "cloudflare", values)
            .with_server_name("Cloudflare MCP");

        assert_eq!(profile.name, "Work Cloudflare");
        assert_eq!(profile.server_id, "cloudflare");
        assert_eq!(profile.server_name.as_deref(), Some("Cloudflare MCP"));
        assert_eq!(profile.values.get("api_key").unwrap(), "secret_123");

        let summary = profile.to_summary(3);
        assert_eq!(summary.id, profile.id);
        assert_eq!(summary.name, "Work Cloudflare");
        assert_eq!(summary.server_id, "cloudflare");
        assert_eq!(summary.server_name.as_deref(), Some("Cloudflare MCP"));
        assert_eq!(summary.input_keys, vec!["api_key", "org_id"]);
        assert_eq!(summary.usage_count, 3);
    }

    #[test]
    fn test_credential_profile_debug_redacts_values() {
        let mut values = HashMap::new();
        values.insert("token".to_string(), "super_secret_token".to_string());

        let profile = CredentialProfile::new("Personal", "github", values);
        let debug_str = format!("{:?}", profile);

        assert!(!debug_str.contains("super_secret_token"));
        assert!(debug_str.contains("token"));
    }
}
