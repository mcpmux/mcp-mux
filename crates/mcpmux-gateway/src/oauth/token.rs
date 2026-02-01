//! OAuth Token types and management
//!
//! Handles token storage, refresh, and expiry.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// OAuth token response from token endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    /// Access token for API calls
    pub access_token: String,

    /// Token type (usually "Bearer")
    pub token_type: String,

    /// Refresh token for getting new access tokens
    pub refresh_token: Option<String>,

    /// Token expiry time
    pub expires_at: Option<DateTime<Utc>>,

    /// Scopes granted
    #[serde(default)]
    pub scope: Option<String>,

    /// ID token (for OIDC)
    pub id_token: Option<String>,
}

/// Token response from OAuth server
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub scope: Option<String>,
    pub id_token: Option<String>,
}

impl From<TokenResponse> for OAuthToken {
    fn from(response: TokenResponse) -> Self {
        let expires_at = response
            .expires_in
            .map(|secs| Utc::now() + Duration::seconds(secs));

        Self {
            access_token: response.access_token,
            token_type: response.token_type,
            refresh_token: response.refresh_token,
            expires_at,
            scope: response.scope,
            id_token: response.id_token,
        }
    }
}

impl OAuthToken {
    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => Utc::now() >= expires_at,
            None => false, // No expiry = never expires
        }
    }

    /// Check if the token will expire soon (within buffer time)
    pub fn expires_soon(&self, buffer_seconds: i64) -> bool {
        match self.expires_at {
            Some(expires_at) => Utc::now() + Duration::seconds(buffer_seconds) >= expires_at,
            None => false,
        }
    }

    /// Check if the token can be refreshed
    pub fn can_refresh(&self) -> bool {
        self.refresh_token.is_some()
    }

    /// Get the authorization header value
    pub fn authorization_header(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }

    /// Get scopes as a vector
    pub fn scopes(&self) -> Vec<String> {
        self.scope
            .as_ref()
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default()
    }
}

/// Token manager for handling token lifecycle
pub struct TokenManager {
    /// Buffer time before expiry to trigger refresh (in seconds)
    refresh_buffer: i64,
}

impl Default for TokenManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenManager {
    /// Create a new token manager
    pub fn new() -> Self {
        Self {
            refresh_buffer: 300, // 5 minutes before expiry
        }
    }

    /// Set the refresh buffer time
    pub fn with_refresh_buffer(mut self, seconds: i64) -> Self {
        self.refresh_buffer = seconds;
        self
    }

    /// Check if a token needs refresh
    pub fn needs_refresh(&self, token: &OAuthToken) -> bool {
        token.can_refresh() && token.expires_soon(self.refresh_buffer)
    }

    /// Check if a token is usable (not expired)
    pub fn is_usable(&self, token: &OAuthToken) -> bool {
        !token.is_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_expiry() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            scope: Some("openid profile".to_string()),
            id_token: None,
        };

        assert!(!token.is_expired());
        assert!(!token.expires_soon(300));
        assert!(token.can_refresh());
    }

    #[test]
    fn test_token_expired() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: Some(Utc::now() - Duration::hours(1)),
            scope: None,
            id_token: None,
        };

        assert!(token.is_expired());
        assert!(!token.can_refresh());
    }

    #[test]
    fn test_token_scopes() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: None,
            scope: Some("openid profile email".to_string()),
            id_token: None,
        };

        let scopes = token.scopes();
        assert_eq!(scopes.len(), 3);
        assert!(scopes.contains(&"openid".to_string()));
        assert!(scopes.contains(&"profile".to_string()));
        assert!(scopes.contains(&"email".to_string()));
    }

    #[test]
    fn test_authorization_header() {
        let token = OAuthToken {
            access_token: "abc123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_at: None,
            scope: None,
            id_token: None,
        };

        assert_eq!(token.authorization_header(), "Bearer abc123");
    }
}
