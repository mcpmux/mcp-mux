//! Client authentication for the gateway: JWT access/refresh token
//! creation and validation for the OAuth 2.0 flow. API keys are validated
//! by the inbound client repository (see `mcp::oauth_middleware`).

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::debug;

type HmacSha256 = Hmac<Sha256>;

// ============================================================================
// JWT Token Management (for OAuth 2.0)
// ============================================================================

/// Token claims structure (simplified JWT-like claims)
#[derive(Debug, Clone)]
pub struct TokenClaims {
    pub client_id: String,
    pub scope: Option<String>,
    pub exp: i64, // Expiration timestamp
    pub iat: i64, // Issued at timestamp
    /// `"access"` or `"refresh"`, as set when the token was issued.
    pub token_type: Option<String>,
}

/// Extractor for authenticated client claims (ISP pattern)
///
/// Usage in handlers: `claims: TokenClaims`
/// Handlers only receive claims, not entire auth context
impl<S> FromRequestParts<S> for TokenClaims
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<TokenClaims>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Missing authentication context"))
    }
}

/// Validate a token and extract claims
pub fn validate_token(token: &str, secret: &[u8]) -> Option<TokenClaims> {
    // Token format: base64(payload).base64(signature)
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        debug!(
            "[Auth] Invalid token format - expected 2 parts, got {}",
            parts.len()
        );
        return None;
    }

    let payload_b64 = parts[0];
    let signature_b64 = parts[1];

    // Verify signature
    let mut mac = HmacSha256::new_from_slice(secret).ok()?;
    mac.update(payload_b64.as_bytes());

    let expected_sig = base64_url_decode(signature_b64)?;
    if mac.verify_slice(&expected_sig).is_err() {
        debug!("[Auth] Invalid token signature");
        return None;
    }

    // Decode payload
    let payload_bytes = base64_url_decode(payload_b64)?;
    let payload_str = String::from_utf8(payload_bytes).ok()?;
    let claims: serde_json::Value = serde_json::from_str(&payload_str).ok()?;

    // Extract claims
    let client_id = claims.get("client_id")?.as_str()?.to_string();
    let scope = claims
        .get("scope")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let exp = claims.get("exp")?.as_i64()?;
    let iat = claims.get("iat")?.as_i64()?;
    let token_type = claims
        .get("token_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check expiration
    let now = chrono::Utc::now().timestamp();
    if now > exp {
        debug!("[Auth] Token expired at {}, now is {}", exp, now);
        return None;
    }

    Some(TokenClaims {
        client_id,
        scope,
        exp,
        iat,
        token_type,
    })
}

/// Validate a token that must be an access token. Refresh tokens are only
/// accepted at the token endpoint, never as a Bearer credential.
pub fn validate_access_token(token: &str, secret: &[u8]) -> Option<TokenClaims> {
    validate_token(token, secret).filter(|claims| claims.token_type.as_deref() == Some("access"))
}

/// Validate a token that must be a refresh token (for the refresh grant).
pub fn validate_refresh_token(token: &str, secret: &[u8]) -> Option<TokenClaims> {
    validate_token(token, secret).filter(|claims| claims.token_type.as_deref() == Some("refresh"))
}

/// Create a signed access token
pub fn create_access_token(
    client_id: &str,
    scope: Option<&str>,
    expires_in: i64,
    secret: &[u8],
) -> String {
    let now = chrono::Utc::now().timestamp();
    let exp = now + expires_in;

    let claims = serde_json::json!({
        "client_id": client_id,
        "scope": scope,
        "exp": exp,
        "iat": now,
        "token_type": "access"
    });

    sign_token(&claims.to_string(), secret)
}

/// Create a signed refresh token
pub fn create_refresh_token(client_id: &str, scope: Option<&str>, secret: &[u8]) -> String {
    let now = chrono::Utc::now().timestamp();
    // Refresh tokens expire in 30 days
    let exp = now + (30 * 24 * 60 * 60);

    let claims = serde_json::json!({
        "client_id": client_id,
        "scope": scope,
        "exp": exp,
        "iat": now,
        "token_type": "refresh"
    });

    sign_token(&claims.to_string(), secret)
}

/// Sign a payload and create token string
fn sign_token(payload: &str, secret: &[u8]) -> String {
    let payload_b64 = base64_url_encode(payload.as_bytes());

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(payload_b64.as_bytes());
    let signature = mac.finalize().into_bytes();

    let signature_b64 = base64_url_encode(&signature);

    format!("{}.{}", payload_b64, signature_b64)
}

/// Base64 URL-safe encoding (no padding)
fn base64_url_encode(data: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(data)
}

/// Base64 URL-safe decoding
fn base64_url_decode(s: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.decode(s).ok()
}

#[cfg(test)]
mod jwt_tests {
    use super::*;

    #[test]
    fn test_create_and_validate_token() {
        let secret = b"test_secret_key_32_bytes_long!!";
        let token = create_access_token("test_client", Some("mcp"), 3600, secret);

        let claims = validate_token(&token, secret);
        assert!(claims.is_some());

        let claims = claims.unwrap();
        assert_eq!(claims.client_id, "test_client");
        assert_eq!(claims.scope, Some("mcp".to_string()));
    }

    #[test]
    fn test_invalid_signature() {
        let secret1 = b"test_secret_key_32_bytes_long!!";
        let secret2 = b"different_secret_key_32_bytes!!";

        let token = create_access_token("test_client", None, 3600, secret1);
        let claims = validate_token(&token, secret2);

        assert!(claims.is_none());
    }

    #[test]
    fn access_and_refresh_tokens_are_not_interchangeable() {
        let secret = b"test_secret_key_32_bytes_long!!";
        let access = create_access_token("test_client", None, 3600, secret);
        let refresh = create_refresh_token("test_client", None, secret);

        assert!(validate_access_token(&access, secret).is_some());
        assert!(validate_access_token(&refresh, secret).is_none());
        assert!(validate_refresh_token(&refresh, secret).is_some());
        assert!(validate_refresh_token(&access, secret).is_none());
    }

    #[test]
    fn test_expired_token() {
        let secret = b"test_secret_key_32_bytes_long!!";
        // Create token that expired 1 hour ago
        let token = create_access_token("test_client", None, -3600, secret);

        let claims = validate_token(&token, secret);
        assert!(claims.is_none());
    }
}
