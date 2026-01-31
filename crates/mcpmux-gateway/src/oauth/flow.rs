//! OAuth Authorization Flow
//!
//! Implements OAuth 2.1 Authorization Code flow with PKCE.

use super::{OAuthMetadata, OAuthToken, PkceChallenge};
use super::token::TokenResponse;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{debug, info};
use url::Url;

/// Authorization request to be opened in browser
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    /// Full authorization URL to open
    pub authorization_url: String,
    /// State parameter for CSRF protection
    pub state: String,
    /// PKCE verifier (keep secret, use in token exchange)
    pub pkce_verifier: String,
}

/// Callback parameters from authorization redirect
#[derive(Debug, Deserialize)]
pub struct AuthorizationCallback {
    /// Authorization code
    pub code: String,
    /// State for verification
    pub state: String,
    /// Error code (if authorization failed)
    pub error: Option<String>,
    /// Error description
    pub error_description: Option<String>,
}

impl AuthorizationCallback {
    /// Check if the callback contains an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get error message if present
    pub fn error_message(&self) -> Option<String> {
        self.error.as_ref().map(|e| {
            match &self.error_description {
                Some(desc) => format!("{}: {}", e, desc),
                None => e.clone(),
            }
        })
    }
}

/// OAuth flow handler
pub struct OAuthFlow {
    metadata: OAuthMetadata,
    client_id: String,
    client_secret: Option<String>,
}

impl OAuthFlow {
    /// Create a new OAuth flow
    pub fn new(
        metadata: OAuthMetadata,
        client_id: String,
        client_secret: Option<String>,
    ) -> Self {
        Self {
            metadata,
            client_id,
            client_secret,
        }
    }

    /// Create an authorization request URL
    pub fn create_authorization_request(
        &self,
        redirect_uri: &str,
        scopes: &[String],
    ) -> anyhow::Result<AuthorizationRequest> {
        // Generate state for CSRF protection
        let state = generate_state();
        
        // Generate PKCE challenge
        let pkce = PkceChallenge::generate();
        
        // Build authorization URL
        let mut url = Url::parse(&self.metadata.authorization_endpoint)?;
        
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &self.client_id);
            query.append_pair("redirect_uri", redirect_uri);
            query.append_pair("scope", &scopes.join(" "));
            query.append_pair("state", &state);
            
            // PKCE parameters
            query.append_pair("code_challenge", &pkce.challenge);
            query.append_pair("code_challenge_method", &pkce.method);
        }
        
        debug!("Created authorization URL: {}", url);
        
        Ok(AuthorizationRequest {
            authorization_url: url.to_string(),
            state,
            pkce_verifier: pkce.verifier,
        })
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(
        &self,
        http_client: &reqwest::Client,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: &str,
    ) -> anyhow::Result<OAuthToken> {
        info!("Exchanging authorization code for tokens");
        
        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code");
        params.insert("code", code);
        params.insert("redirect_uri", redirect_uri);
        params.insert("client_id", &self.client_id);
        params.insert("code_verifier", pkce_verifier);
        
        // Add client secret if available
        let client_secret;
        if let Some(secret) = &self.client_secret {
            client_secret = secret.clone();
            params.insert("client_secret", &client_secret);
        }
        
        let response = http_client
            .post(&self.metadata.token_endpoint)
            .form(&params)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Token exchange failed: HTTP {} - {}", status, body);
        }
        
        let token_response: TokenResponse = response.json().await?;
        info!("Token exchange successful");
        
        Ok(token_response.into())
    }

    /// Refresh an access token
    pub async fn refresh_token(
        &self,
        http_client: &reqwest::Client,
        refresh_token: &str,
    ) -> anyhow::Result<OAuthToken> {
        info!("Refreshing access token");
        
        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("refresh_token", refresh_token);
        params.insert("client_id", &self.client_id);
        
        // Add client secret if available
        let client_secret;
        if let Some(secret) = &self.client_secret {
            client_secret = secret.clone();
            params.insert("client_secret", &client_secret);
        }
        
        let response = http_client
            .post(&self.metadata.token_endpoint)
            .form(&params)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Token refresh failed: HTTP {} - {}", status, body);
        }
        
        let token_response: TokenResponse = response.json().await?;
        info!("Token refresh successful");
        
        Ok(token_response.into())
    }
}

/// Generate a random state parameter
fn generate_state() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_metadata() -> OAuthMetadata {
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
            scopes_supported: vec!["openid".to_string()],
            code_challenge_methods_supported: vec!["S256".to_string()],
            token_endpoint_auth_methods_supported: vec![],
        }
    }

    #[test]
    fn test_authorization_request() {
        let flow = OAuthFlow::new(
            test_metadata(),
            "test_client".to_string(),
            None,
        );

        let request = flow
            .create_authorization_request(
                "http://localhost:8080/callback",
                &["openid".to_string(), "profile".to_string()],
            )
            .unwrap();

        // Should contain required parameters
        assert!(request.authorization_url.contains("response_type=code"));
        assert!(request.authorization_url.contains("client_id=test_client"));
        assert!(request.authorization_url.contains("code_challenge="));
        assert!(request.authorization_url.contains("code_challenge_method=S256"));
        
        // State and verifier should be present
        assert!(!request.state.is_empty());
        assert!(!request.pkce_verifier.is_empty());
    }

    #[test]
    fn test_authorization_callback_error() {
        let callback = AuthorizationCallback {
            code: "".to_string(),
            state: "test".to_string(),
            error: Some("access_denied".to_string()),
            error_description: Some("User denied access".to_string()),
        };

        assert!(callback.is_error());
        assert_eq!(
            callback.error_message(),
            Some("access_denied: User denied access".to_string())
        );
    }
}
