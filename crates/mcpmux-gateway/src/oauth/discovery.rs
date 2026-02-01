//! OAuth Discovery (OpenID Connect Discovery)
//!
//! Fetches OAuth/OIDC metadata from `.well-known` endpoints.

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// OAuth/OIDC Metadata from discovery endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthMetadata {
    /// Issuer identifier
    pub issuer: String,

    /// Authorization endpoint URL
    pub authorization_endpoint: String,

    /// Token endpoint URL
    pub token_endpoint: String,

    /// User info endpoint (optional)
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,

    /// Token revocation endpoint (optional)
    #[serde(default)]
    pub revocation_endpoint: Option<String>,

    /// Dynamic client registration endpoint (optional)
    #[serde(default)]
    pub registration_endpoint: Option<String>,

    /// JWKS URI for token validation (optional)
    #[serde(default)]
    pub jwks_uri: Option<String>,

    /// Supported response types
    #[serde(default)]
    pub response_types_supported: Vec<String>,

    /// Supported grant types
    #[serde(default)]
    pub grant_types_supported: Vec<String>,

    /// Supported scopes
    #[serde(default)]
    pub scopes_supported: Vec<String>,

    /// Supported PKCE code challenge methods
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,

    /// Supported token endpoint auth methods
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
}

impl OAuthMetadata {
    /// Check if PKCE is supported
    pub fn supports_pkce(&self) -> bool {
        self.code_challenge_methods_supported
            .contains(&"S256".to_string())
    }

    /// Check if a specific scope is supported
    pub fn supports_scope(&self, scope: &str) -> bool {
        self.scopes_supported.is_empty() || self.scopes_supported.contains(&scope.to_string())
    }
}

/// OAuth Discovery client
pub struct OAuthDiscovery {
    http_client: reqwest::Client,
}

impl OAuthDiscovery {
    /// Create a new discovery client
    pub fn new(http_client: reqwest::Client) -> Self {
        Self { http_client }
    }

    /// Fetch OAuth metadata from issuer
    pub async fn fetch(&self, issuer: &str) -> anyhow::Result<OAuthMetadata> {
        // Try OIDC discovery first
        let oidc_url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        debug!("Trying OIDC discovery: {}", oidc_url);

        match self.fetch_metadata(&oidc_url).await {
            Ok(metadata) => {
                info!("OIDC discovery successful for {}", issuer);
                return Ok(metadata);
            }
            Err(e) => {
                debug!("OIDC discovery failed: {}, trying OAuth AS metadata", e);
            }
        }

        // Fall back to OAuth Authorization Server metadata
        let oauth_url = format!(
            "{}/.well-known/oauth-authorization-server",
            issuer.trim_end_matches('/')
        );
        debug!("Trying OAuth AS discovery: {}", oauth_url);

        match self.fetch_metadata(&oauth_url).await {
            Ok(metadata) => {
                info!("OAuth AS discovery successful for {}", issuer);
                Ok(metadata)
            }
            Err(e) => {
                anyhow::bail!(
                    "OAuth discovery failed for {}: no valid metadata at OIDC or OAuth AS endpoints: {}",
                    issuer,
                    e
                )
            }
        }
    }

    /// Fetch metadata from a specific URL
    async fn fetch_metadata(&self, url: &str) -> anyhow::Result<OAuthMetadata> {
        let response = self
            .http_client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Discovery request failed: HTTP {}", response.status());
        }

        let metadata: OAuthMetadata = response.json().await?;
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_metadata() -> OAuthMetadata {
        OAuthMetadata {
            issuer: "https://example.com".to_string(),
            authorization_endpoint: "https://example.com/authorize".to_string(),
            token_endpoint: "https://example.com/token".to_string(),
            userinfo_endpoint: None,
            revocation_endpoint: None,
            registration_endpoint: None,
            jwks_uri: None,
            response_types_supported: vec!["code".to_string()],
            grant_types_supported: vec!["authorization_code".to_string()],
            scopes_supported: vec!["openid".to_string(), "profile".to_string()],
            code_challenge_methods_supported: vec!["S256".to_string()],
            token_endpoint_auth_methods_supported: vec!["client_secret_post".to_string()],
        }
    }

    #[test]
    fn test_metadata_pkce_support() {
        let metadata = create_test_metadata();

        assert!(metadata.supports_pkce());
        assert!(metadata.supports_scope("openid"));
        assert!(metadata.supports_scope("profile"));
    }

    #[test]
    fn test_metadata_no_pkce_support() {
        let mut metadata = create_test_metadata();
        metadata.code_challenge_methods_supported = vec!["plain".to_string()];

        assert!(!metadata.supports_pkce());
    }

    #[test]
    fn test_metadata_empty_pkce_methods() {
        let mut metadata = create_test_metadata();
        metadata.code_challenge_methods_supported = vec![];

        assert!(!metadata.supports_pkce());
    }

    #[test]
    fn test_metadata_scope_not_supported() {
        let metadata = create_test_metadata();

        assert!(!metadata.supports_scope("email"));
        assert!(!metadata.supports_scope("admin"));
    }

    #[test]
    fn test_metadata_empty_scopes_allows_all() {
        let mut metadata = create_test_metadata();
        metadata.scopes_supported = vec![];

        // Empty scopes list means all scopes are allowed
        assert!(metadata.supports_scope("anything"));
        assert!(metadata.supports_scope("custom_scope"));
    }

    #[test]
    fn test_metadata_json_deserialization() {
        let json = r#"{
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "response_types_supported": ["code", "token"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "scopes_supported": ["openid", "profile", "email"],
            "code_challenge_methods_supported": ["S256", "plain"]
        }"#;

        let metadata: OAuthMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(metadata.issuer, "https://auth.example.com");
        assert_eq!(
            metadata.authorization_endpoint,
            "https://auth.example.com/authorize"
        );
        assert_eq!(metadata.token_endpoint, "https://auth.example.com/token");
        assert!(metadata.supports_pkce());
        assert!(metadata.supports_scope("email"));
        assert_eq!(metadata.grant_types_supported.len(), 2);
    }

    #[test]
    fn test_metadata_json_with_optional_fields() {
        let json = r#"{
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "userinfo_endpoint": "https://auth.example.com/userinfo",
            "revocation_endpoint": "https://auth.example.com/revoke",
            "registration_endpoint": "https://auth.example.com/register",
            "jwks_uri": "https://auth.example.com/.well-known/jwks.json"
        }"#;

        let metadata: OAuthMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(
            metadata.userinfo_endpoint,
            Some("https://auth.example.com/userinfo".to_string())
        );
        assert_eq!(
            metadata.revocation_endpoint,
            Some("https://auth.example.com/revoke".to_string())
        );
        assert_eq!(
            metadata.registration_endpoint,
            Some("https://auth.example.com/register".to_string())
        );
        assert_eq!(
            metadata.jwks_uri,
            Some("https://auth.example.com/.well-known/jwks.json".to_string())
        );
    }

    #[test]
    fn test_metadata_json_minimal() {
        // Only required fields
        let json = r#"{
            "issuer": "https://minimal.example.com",
            "authorization_endpoint": "https://minimal.example.com/auth",
            "token_endpoint": "https://minimal.example.com/token"
        }"#;

        let metadata: OAuthMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(metadata.issuer, "https://minimal.example.com");
        assert!(metadata.userinfo_endpoint.is_none());
        assert!(metadata.scopes_supported.is_empty());
        assert!(metadata.code_challenge_methods_supported.is_empty());
        // Empty scopes = all allowed
        assert!(metadata.supports_scope("any"));
        // No S256 = no PKCE
        assert!(!metadata.supports_pkce());
    }
}
