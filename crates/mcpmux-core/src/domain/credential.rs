//! Credential entity - secure credential storage
//!
//! Each credential is a typed entry: one row per (space, server, type).
//! This allows separate lifecycle management for access tokens vs refresh tokens,
//! and keeps metadata (expiry, scope) as plaintext while only encrypting the secret value.
//!
//! Note: OAuth client registration (client_id, endpoints) is stored separately
//! in the `oauth_clients` table via OAuthClient entity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Type of credential entry.
///
/// Extensible: add new variants for session tokens, client certificates, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    /// OAuth access token (~1h lifetime)
    AccessToken,
    /// OAuth refresh token (~90d lifetime)
    RefreshToken,
    /// Simple API key (no expiry)
    ApiKey,
    /// Basic auth username
    BasicAuthUser,
    /// Basic auth password
    BasicAuthPass,
}

impl CredentialType {
    /// Convert to database string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AccessToken => "access_token",
            Self::RefreshToken => "refresh_token",
            Self::ApiKey => "api_key",
            Self::BasicAuthUser => "basic_auth_user",
            Self::BasicAuthPass => "basic_auth_pass",
        }
    }

    /// Parse from database string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "access_token" => Some(Self::AccessToken),
            "refresh_token" => Some(Self::RefreshToken),
            "api_key" => Some(Self::ApiKey),
            "basic_auth_user" => Some(Self::BasicAuthUser),
            "basic_auth_pass" => Some(Self::BasicAuthPass),
            _ => None,
        }
    }

    /// Whether this type represents an OAuth token (access or refresh).
    pub fn is_oauth(&self) -> bool {
        matches!(self, Self::AccessToken | Self::RefreshToken)
    }
}

impl fmt::Display for CredentialType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Individual credential entry — one per (space, server, type).
///
/// The `value` field contains the secret (token, key, password) in plaintext
/// at the domain level. Encryption is handled by the storage layer.
///
/// Metadata fields (expires_at, token_type, scope) are non-sensitive and
/// stored as plaintext in the database for queryability.
#[derive(Debug, Clone)]
pub struct Credential {
    /// Space this credential belongs to
    pub space_id: Uuid,

    /// Server this credential is for
    pub server_id: String,

    /// Type of credential (access_token, refresh_token, api_key, etc.)
    pub credential_type: CredentialType,

    /// The secret value (plaintext at domain level, encrypted at storage level)
    pub value: String,

    /// When this credential expires (plaintext in DB for queryability)
    pub expires_at: Option<DateTime<Utc>>,

    /// Token type, e.g. "Bearer" (only for access_token)
    pub token_type: Option<String>,

    /// OAuth scope (only for access_token)
    pub scope: Option<String>,

    /// When the credential was created
    pub created_at: DateTime<Utc>,

    /// When the credential was last updated
    pub updated_at: DateTime<Utc>,

    /// When the credential was last used
    pub last_used: Option<DateTime<Utc>>,
}

impl Credential {
    /// Create a new API key credential.
    pub fn api_key(space_id: Uuid, server_id: impl Into<String>, key: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            space_id,
            server_id: server_id.into(),
            credential_type: CredentialType::ApiKey,
            value: key.into(),
            expires_at: None,
            token_type: None,
            scope: None,
            created_at: now,
            updated_at: now,
            last_used: None,
        }
    }

    /// Create an OAuth access token credential.
    pub fn access_token(
        space_id: Uuid,
        server_id: impl Into<String>,
        token: impl Into<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            space_id,
            server_id: server_id.into(),
            credential_type: CredentialType::AccessToken,
            value: token.into(),
            expires_at,
            token_type: Some("Bearer".to_string()),
            scope: None,
            created_at: now,
            updated_at: now,
            last_used: None,
        }
    }

    /// Create an OAuth refresh token credential.
    pub fn refresh_token(
        space_id: Uuid,
        server_id: impl Into<String>,
        token: impl Into<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            space_id,
            server_id: server_id.into(),
            credential_type: CredentialType::RefreshToken,
            value: token.into(),
            expires_at,
            token_type: None,
            scope: None,
            created_at: now,
            updated_at: now,
            last_used: None,
        }
    }

    /// Check if this credential is expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => exp < Utc::now(),
            None => false,
        }
    }

    /// Check if this is an OAuth credential (access or refresh token).
    pub fn is_oauth(&self) -> bool {
        self.credential_type.is_oauth()
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
        assert_eq!(cred.credential_type, CredentialType::ApiKey);
        assert_eq!(cred.value, "ghp_xxx");
        assert!(!cred.is_expired());
        assert!(!cred.is_oauth());
    }

    #[test]
    fn test_access_token_credential() {
        let space_id = Uuid::new_v4();
        let cred = Credential::access_token(
            space_id,
            "atlassian",
            "access_token_xyz",
            Some(Utc::now() + chrono::Duration::hours(1)),
        );

        assert_eq!(cred.credential_type, CredentialType::AccessToken);
        assert!(!cred.is_expired());
        assert!(cred.is_oauth());
        assert_eq!(cred.token_type, Some("Bearer".to_string()));
    }

    #[test]
    fn test_refresh_token_credential() {
        let space_id = Uuid::new_v4();
        let cred = Credential::refresh_token(space_id, "atlassian", "refresh_xyz", None);

        assert_eq!(cred.credential_type, CredentialType::RefreshToken);
        assert!(!cred.is_expired()); // No expiry set
        assert!(cred.is_oauth());
    }

    #[test]
    fn test_expired_credential() {
        let space_id = Uuid::new_v4();
        let cred = Credential::access_token(
            space_id,
            "atlassian",
            "access_token",
            Some(Utc::now() - chrono::Duration::hours(1)),
        );

        assert!(cred.is_expired());
    }

    #[test]
    fn test_credential_type_roundtrip() {
        for ct in [
            CredentialType::AccessToken,
            CredentialType::RefreshToken,
            CredentialType::ApiKey,
            CredentialType::BasicAuthUser,
            CredentialType::BasicAuthPass,
        ] {
            let s = ct.as_str();
            let parsed = CredentialType::parse(s).unwrap();
            assert_eq!(ct, parsed);
        }
    }
}
