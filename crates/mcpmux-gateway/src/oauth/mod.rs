//! OAuth 2.1 Implementation
//!
//! Provides OAuth 2.1 with PKCE for authenticating with remote MCP servers.

mod dcr;
mod discovery;
mod flow;
mod pkce;
mod token;

pub use dcr::{process_dcr_request, validate_redirect_uris, DcrError, DcrRequest, DcrResponse};
pub use discovery::{OAuthDiscovery, OAuthMetadata};
pub use flow::{AuthorizationCallback, AuthorizationRequest, OAuthFlow};
pub use pkce::PkceChallenge;
pub use token::{OAuthToken, TokenManager};

use serde::{Deserialize, Serialize};

/// OAuth configuration for a server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// Issuer URL (e.g., https://auth.atlassian.com)
    pub issuer: String,
    /// OAuth scopes to request
    pub scopes: Vec<String>,
    /// Client ID (from discovery or pre-configured)
    pub client_id: Option<String>,
    /// Client secret (from discovery or pre-configured)
    pub client_secret: Option<String>,
}

impl OAuthConfig {
    /// Create a new OAuth config with just the issuer
    pub fn new(issuer: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            scopes: vec!["openid".to_string()],
            client_id: None,
            client_secret: None,
        }
    }

    /// Add scopes
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Set client credentials
    pub fn with_client(mut self, client_id: String, client_secret: Option<String>) -> Self {
        self.client_id = Some(client_id);
        self.client_secret = client_secret;
        self
    }
}

/// OAuth Manager coordinates the entire OAuth lifecycle
pub struct OAuthManager {
    http_client: reqwest::Client,
    config: OAuthConfig,
    metadata: Option<OAuthMetadata>,
}

impl OAuthManager {
    /// Create a new OAuth manager
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            config,
            metadata: None,
        }
    }

    /// Discover OAuth endpoints from the issuer
    pub async fn discover(&mut self) -> anyhow::Result<&OAuthMetadata> {
        let discovery = OAuthDiscovery::new(self.http_client.clone());
        let metadata = discovery.fetch(&self.config.issuer).await?;
        self.metadata = Some(metadata);
        Ok(self.metadata.as_ref().unwrap())
    }

    /// Get cached metadata or discover
    pub async fn get_metadata(&mut self) -> anyhow::Result<&OAuthMetadata> {
        if self.metadata.is_none() {
            self.discover().await?;
        }
        Ok(self.metadata.as_ref().unwrap())
    }

    /// Start authorization flow
    pub async fn start_authorization(
        &mut self,
        redirect_uri: &str,
    ) -> anyhow::Result<AuthorizationRequest> {
        self.discover().await?;
        let metadata = self
            .metadata
            .clone()
            .ok_or_else(|| anyhow::anyhow!("OAuth metadata not available"))?;

        let client_id = self
            .config
            .client_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Client ID required for authorization"))?;

        let flow = OAuthFlow::new(metadata, client_id, self.config.client_secret.clone());

        flow.create_authorization_request(redirect_uri, &self.config.scopes)
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: &str,
    ) -> anyhow::Result<OAuthToken> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OAuth metadata not discovered"))?;

        let client_id = self
            .config
            .client_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Client ID required"))?;

        let flow = OAuthFlow::new(
            metadata.clone(),
            client_id.clone(),
            self.config.client_secret.clone(),
        );

        flow.exchange_code(&self.http_client, code, redirect_uri, pkce_verifier)
            .await
    }

    /// Refresh an access token
    pub async fn refresh_token(&self, refresh_token: &str) -> anyhow::Result<OAuthToken> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OAuth metadata not discovered"))?;

        let client_id = self
            .config
            .client_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Client ID required"))?;

        let flow = OAuthFlow::new(
            metadata.clone(),
            client_id.clone(),
            self.config.client_secret.clone(),
        );

        flow.refresh_token(&self.http_client, refresh_token).await
    }
}
