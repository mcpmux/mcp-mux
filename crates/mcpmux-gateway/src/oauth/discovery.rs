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
        self.code_challenge_methods_supported.contains(&"S256".to_string())
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
        let oidc_url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
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

    #[test]
    fn test_metadata_pkce_support() {
        let metadata = OAuthMetadata {
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
        };

        assert!(metadata.supports_pkce());
        assert!(metadata.supports_scope("openid"));
        assert!(metadata.supports_scope("profile"));
    }
}
