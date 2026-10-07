//! Outbound OAuth Registration - McpMux's OAuth credentials with backend servers
//!
//! OUTBOUND: McpMux acting as OAuth CLIENT connecting TO backend MCP servers

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Cached OAuth metadata from server discovery.
///
/// Stored during initial OAuth flow to avoid re-discovery on reconnect.
/// RMCP's metadata discovery can fail for servers that don't follow the exact
/// MCP spec path conventions (e.g., Cloudflare serves metadata at root, not path-suffixed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOAuthMetadata {
    /// Authorization endpoint URL (required)
    pub authorization_endpoint: String,
    /// Token endpoint URL (required)  
    pub token_endpoint: String,
    /// Dynamic client registration endpoint (optional)
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    /// Issuer identifier (optional)
    #[serde(default)]
    pub issuer: Option<String>,
    /// JWKS URI for token validation (optional)
    #[serde(default)]
    pub jwks_uri: Option<String>,
    /// Supported scopes (optional)
    #[serde(default)]
    pub scopes_supported: Option<Vec<String>>,
    /// Supported response types (optional)
    #[serde(default)]
    pub response_types_supported: Option<Vec<String>>,
    /// Additional fields from discovery (for forward compatibility)
    #[serde(default, flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

/// McpMux's OUTBOUND OAuth client registration WITH a backend MCP server.
///
/// When connecting to OAuth-protected servers (e.g., Cloudflare, Atlassian),
/// McpMux registers as an OAuth client with them via DCR.
///
/// This stores:
/// - The client_id McpMux received from their DCR
/// - The redirect_uri used during DCR (includes port)
/// - The server_url for AuthorizationManager creation
///
/// The redirect_uri is stored to detect port changes across app restarts.
/// If the callback server port changes, we must re-DCR since OAuth providers
/// validate redirect_uri exactly.
///
/// Separate from tokens (in credentials table) so:
/// - Logout clears tokens but keeps registration
/// - Re-auth uses existing client_id without new DCR (if port matches)
#[derive(Clone, Serialize, Deserialize)]
pub struct OutboundOAuthRegistration {
    /// Unique ID for this registration
    pub id: Uuid,

    /// Space ID
    pub space_id: Uuid,

    /// Server ID (from registry)
    pub server_id: String,

    /// Base URL used for OAuth discovery
    pub server_url: String,

    /// Client ID from Dynamic Client Registration
    pub client_id: String,

    /// Client secret from Dynamic Client Registration, when the authorization
    /// server issued one (a confidential client). It must be sent on every token
    /// request, including refreshes after an app restart. Stored encrypted.
    #[serde(default)]
    pub client_secret: Option<String>,

    /// Redirect URI used during DCR (e.g., "http://127.0.0.1:9876/callback")
    /// Must match when reusing client_id, otherwise re-DCR is needed.
    #[serde(default)]
    pub redirect_uri: Option<String>,

    /// Cached OAuth metadata from initial discovery.
    /// Stored to avoid RMCP's metadata discovery failures on non-spec-compliant servers.
    #[serde(default)]
    pub metadata: Option<StoredOAuthMetadata>,

    /// When the client was registered
    pub created_at: DateTime<Utc>,

    /// Last update time
    pub updated_at: DateTime<Utc>,
}

impl OutboundOAuthRegistration {
    /// Create a new registration from DCR response
    pub fn new(
        space_id: Uuid,
        server_id: impl Into<String>,
        server_url: impl Into<String>,
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            space_id,
            server_id: server_id.into(),
            server_url: server_url.into(),
            client_id: client_id.into(),
            client_secret: None,
            redirect_uri: Some(redirect_uri.into()),
            metadata: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a new registration with metadata
    pub fn with_metadata(
        space_id: Uuid,
        server_id: impl Into<String>,
        server_url: impl Into<String>,
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
        metadata: StoredOAuthMetadata,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            space_id,
            server_id: server_id.into(),
            server_url: server_url.into(),
            client_id: client_id.into(),
            client_secret: None,
            redirect_uri: Some(redirect_uri.into()),
            metadata: Some(metadata),
            created_at: now,
            updated_at: now,
        }
    }

    /// Set the client secret issued by Dynamic Client Registration
    pub fn with_client_secret(mut self, client_secret: Option<String>) -> Self {
        self.client_secret = client_secret;
        self
    }

    /// Check if this registration can be reused with the given redirect_uri
    pub fn matches_redirect_uri(&self, redirect_uri: &str) -> bool {
        self.redirect_uri
            .as_ref()
            .map(|r| r == redirect_uri)
            .unwrap_or(false)
    }
}

/// Redacts `client_secret` so the registration can be logged with `{:?}`.
impl std::fmt::Debug for OutboundOAuthRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundOAuthRegistration")
            .field("id", &self.id)
            .field("space_id", &self.space_id)
            .field("server_id", &self.server_id)
            .field("server_url", &self.server_url)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[redacted]"),
            )
            .field("redirect_uri", &self.redirect_uri)
            .field("metadata", &self.metadata)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_client_secret() {
        let reg = OutboundOAuthRegistration::new(
            Uuid::new_v4(),
            "server",
            "https://mcp.example.com/mcp",
            "client-123",
            "http://127.0.0.1:45819/oauth2redirect",
        )
        .with_client_secret(Some("s3cr3t-value".to_string()));

        let debug = format!("{:?}", reg);
        assert!(!debug.contains("s3cr3t-value"), "secret leaked: {debug}");
        assert!(debug.contains("[redacted]"));
        assert!(debug.contains("client-123"));
    }

    #[test]
    fn debug_output_shows_missing_client_secret_as_none() {
        let reg = OutboundOAuthRegistration::new(
            Uuid::new_v4(),
            "server",
            "https://mcp.example.com/mcp",
            "client-123",
            "http://127.0.0.1:45819/oauth2redirect",
        );

        assert!(format!("{:?}", reg).contains("client_secret: None"));
    }
}
