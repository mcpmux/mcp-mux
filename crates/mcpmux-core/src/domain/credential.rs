//! Credential entity - secure credential storage
//!
//! Note: OAuth client registration (client_id, endpoints) is stored separately
//! in the `oauth_clients` table via OAuthClient entity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Credential type - stores tokens/keys, NOT client registration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialValue {
    /// Simple API key
    ApiKey { key: String },

    /// OAuth tokens (client registration is in oauth_clients table)
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        token_type: String,
        scope: Option<String>,
    },

    /// Basic authentication
    BasicAuth { username: String, password: String },
}

/// Credential for a specific (Space, Server) combination.
///
/// Credentials are stored locally only, never synced to cloud.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    /// Space ID
    pub space_id: Uuid,

    /// Server ID
    pub server_id: String,

    /// The credential value
    pub value: CredentialValue,

    /// When the credential was created
    pub created_at: DateTime<Utc>,

    /// When the credential was last updated
    pub updated_at: DateTime<Utc>,

    /// When the credential was last used
    pub last_used: Option<DateTime<Utc>>,
}

impl Credential {
    /// Create a new API key credential
    pub fn api_key(space_id: Uuid, server_id: impl Into<String>, key: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            space_id,
            server_id: server_id.into(),
            value: CredentialValue::ApiKey { key: key.into() },
            created_at: now,
            updated_at: now,
            last_used: None,
        }
    }

    /// Create an OAuth credential (tokens only, client registration is separate)
    pub fn oauth(
        space_id: Uuid,
        server_id: impl Into<String>,
        access_token: impl Into<String>,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            space_id,
            server_id: server_id.into(),
            value: CredentialValue::OAuth {
                access_token: access_token.into(),
                refresh_token,
                expires_at,
                token_type: "Bearer".to_string(),
                scope: None,
            },
            created_at: now,
            updated_at: now,
            last_used: None,
        }
    }

    /// Get the credential key for storage lookup
    pub fn key(&self) -> String {
        format!("{}:{}", self.space_id, self.server_id)
    }

    /// Check if this credential is expired (for OAuth)
    pub fn is_expired(&self) -> bool {
        match &self.value {
            CredentialValue::OAuth {
                expires_at: Some(exp),
                ..
            } => *exp < Utc::now(),
            _ => false,
        }
    }

    /// Check if this credential can be refreshed
    pub fn can_refresh(&self) -> bool {
        matches!(
            &self.value,
            CredentialValue::OAuth {
                refresh_token: Some(_),
                ..
            }
        )
    }

    /// Update the access token (after refresh)
    pub fn update_token(&mut self, access_token: String, expires_at: Option<DateTime<Utc>>) {
        if let CredentialValue::OAuth {
            access_token: ref mut at,
            expires_at: ref mut exp,
            ..
        } = self.value
        {
            *at = access_token;
            *exp = expires_at;
            self.updated_at = Utc::now();
        }
    }

    /// Check if this is an OAuth credential
    pub fn is_oauth(&self) -> bool {
        matches!(self.value, CredentialValue::OAuth { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_credential() {
        let space_id = Uuid::new_v4();
        let cred = Credential::api_key(space_id, "github", "ghp_xxx");

        assert_eq!(cred.server_id, "github");
        assert!(matches!(cred.value, CredentialValue::ApiKey { .. }));
        assert!(!cred.is_expired());
        assert!(!cred.can_refresh());
    }

    #[test]
    fn test_oauth_credential() {
        let space_id = Uuid::new_v4();
        let cred = Credential::oauth(
            space_id,
            "atlassian",
            "access_token",
            Some("refresh_token".to_string()),
            Some(Utc::now() + chrono::Duration::hours(1)),
        );

        assert!(!cred.is_expired());
        assert!(cred.can_refresh());
    }

    #[test]
    fn test_expired_credential() {
        let space_id = Uuid::new_v4();
        let cred = Credential::oauth(
            space_id,
            "atlassian",
            "access_token",
            None,
            Some(Utc::now() - chrono::Duration::hours(1)),
        );

        assert!(cred.is_expired());
        assert!(!cred.can_refresh());
    }
}
