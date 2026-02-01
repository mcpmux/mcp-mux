//! Client authentication for the gateway
//!
//! Validates client access keys and manages client sessions.
//! Also provides JWT token creation/validation for OAuth 2.0 flow.

use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{header, request::Parts, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::server::GatewayState;

type HmacSha256 = Hmac<Sha256>;

/// Authenticated client from request
#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    /// Client ID
    pub client_id: Uuid,
    /// Access key used
    pub access_key: String,
}

/// Access key authentication extractor
pub struct AccessKeyAuth(pub AuthenticatedClient);

impl<S> FromRequestParts<S> for AccessKeyAuth
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Try to get Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok());

        let access_key = match auth_header {
            Some(header) if header.starts_with("Bearer ") => {
                header.strip_prefix("Bearer ").unwrap().to_string()
            }
            Some(header) if header.starts_with("MCP-Key ") => {
                header.strip_prefix("MCP-Key ").unwrap().to_string()
            }
            _ => {
                // Try X-MCP-Access-Key header
                parts
                    .headers
                    .get("X-MCP-Access-Key")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from)
                    .ok_or((StatusCode::UNAUTHORIZED, "Missing access key"))?
            }
        };

        debug!("Access key authentication attempt");

        // For now, create a mock client
        // TODO: Validate against actual access keys in state
        let client = AuthenticatedClient {
            client_id: Uuid::nil(), // Will be resolved from state
            access_key,
        };

        Ok(AccessKeyAuth(client))
    }
}

/// Access key format and validation
#[derive(Debug, Clone)]
pub struct AccessKey {
    /// The raw key string
    pub key: String,
    /// Client ID this key belongs to
    pub client_id: Uuid,
    /// Optional expiry time
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AccessKey {
    /// Generate a new access key for a client
    pub fn generate(client_id: Uuid) -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let random_bytes: [u8; 24] = rng.gen();
        let key = format!(
            "mcp_{}",
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                random_bytes
            )
        );

        Self {
            key,
            client_id,
            expires_at: None,
        }
    }

    /// Generate an access key with expiry
    pub fn generate_with_expiry(client_id: Uuid, duration: chrono::Duration) -> Self {
        let mut key = Self::generate(client_id);
        key.expires_at = Some(chrono::Utc::now() + duration);
        key
    }

    /// Check if the key is expired
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => chrono::Utc::now() >= expires_at,
            None => false,
        }
    }

    /// Validate key format
    pub fn is_valid_format(key: &str) -> bool {
        key.starts_with("mcp_") && key.len() >= 36
    }
}

/// Access key validator
pub struct AccessKeyValidator {
    state: Arc<RwLock<GatewayState>>,
}

impl AccessKeyValidator {
    pub fn new(state: Arc<RwLock<GatewayState>>) -> Self {
        Self { state }
    }

    /// Validate an access key and return the client ID
    pub async fn validate(&self, key: &str) -> Option<Uuid> {
        if !AccessKey::is_valid_format(key) {
            warn!("Invalid access key format");
            return None;
        }

        let state = self.state.read().await;
        state.validate_access_key(key)
    }

    /// Register a new access key
    pub async fn register(&self, key: AccessKey) {
        let mut state = self.state.write().await;
        state.register_access_key(key.key, key.client_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_key_generation() {
        let client_id = Uuid::new_v4();
        let key = AccessKey::generate(client_id);

        assert!(key.key.starts_with("mcp_"));
        assert!(key.key.len() >= 36);
        assert_eq!(key.client_id, client_id);
        assert!(!key.is_expired());
    }

    #[test]
    fn test_access_key_format_validation() {
        assert!(AccessKey::is_valid_format(
            "mcp_abcdefghijklmnopqrstuvwxyz123456"
        ));
        assert!(!AccessKey::is_valid_format("invalid_key"));
        assert!(!AccessKey::is_valid_format("mcp_short"));
    }

    #[test]
    fn test_access_key_uniqueness() {
        let client_id = Uuid::new_v4();
        let key1 = AccessKey::generate(client_id);
        let key2 = AccessKey::generate(client_id);

        // Each generation should produce unique key
        assert_ne!(key1.key, key2.key);
    }

    #[test]
    fn test_access_key_with_expiry() {
        let client_id = Uuid::new_v4();
        let key = AccessKey::generate_with_expiry(client_id, chrono::Duration::hours(1));

        assert!(key.expires_at.is_some());
        assert!(!key.is_expired());
    }

    #[test]
    fn test_access_key_expired() {
        let client_id = Uuid::new_v4();
        // Create key that expired 1 hour ago
        let mut key = AccessKey::generate(client_id);
        key.expires_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));

        assert!(key.is_expired());
    }

    #[test]
    fn test_access_key_no_expiry_never_expires() {
        let client_id = Uuid::new_v4();
        let key = AccessKey::generate(client_id);

        // Key without expiry should never be expired
        assert!(key.expires_at.is_none());
        assert!(!key.is_expired());
    }
}

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
    })
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

/// Authentication middleware for MCP endpoints.
///
/// OAuth authentication middleware
///
/// Responsibility: Validate JWT tokens and inject claims into request context
/// Follows SRP: Only handles authentication, not authorization
pub async fn oauth_auth_middleware(
    axum::extract::State(state): axum::extract::State<Arc<RwLock<GatewayState>>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Skip auth for OPTIONS (CORS preflight)
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }

    let gateway_state = state.read().await;

    // Get base URL and JWT secret
    let base_url = gateway_state.base_url.clone();
    let Some(secret) = gateway_state.get_jwt_secret() else {
        warn!("[Auth] No JWT secret configured - rejecting all requests");
        return unauthorized_response_with_url(
            &base_url,
            "server_error",
            "Server not configured for authentication",
        );
    };

    // Extract Authorization header
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => {
            let token = &auth[7..];

            // Validate token
            match validate_token(token, secret) {
                Some(claims) => {
                    debug!("[Auth] Valid token for client: {}", claims.client_id);

                    // Inject claims into request extensions (DIP: provide abstraction for handlers)
                    request.extensions_mut().insert(claims);

                    // Token valid - proceed with request
                    drop(gateway_state);
                    next.run(request).await
                }
                None => {
                    warn!("[Auth] Invalid or expired token");
                    unauthorized_response_with_url(
                        &base_url,
                        "invalid_token",
                        "Token is invalid or expired",
                    )
                }
            }
        }
        Some(_) => {
            warn!("[Auth] Invalid Authorization header format");
            unauthorized_response_with_url(
                &base_url,
                "invalid_request",
                "Invalid Authorization header format",
            )
        }
        None => {
            info!("[Auth] No Authorization header - returning 401 with OAuth discovery info");
            unauthorized_response_with_url(&base_url, "invalid_token", "Missing access token")
        }
    }
}

/// Generate 401 Unauthorized response with OAuth metadata.
///
/// Per RFC 9728, the WWW-Authenticate header should include `resource_metadata`
/// parameter pointing to the OAuth Protected Resource Metadata endpoint.
fn unauthorized_response_with_url(base_url: &str, error: &str, description: &str) -> Response {
    // RFC 9728: Protected Resource Metadata URL
    let resource_metadata_url = format!("{}/.well-known/oauth-protected-resource", base_url);

    // WWW-Authenticate header per RFC 9728
    let www_authenticate = format!(
        r#"Bearer realm="McpMux Gateway", error="{}", error_description="{}", resource_metadata="{}""#,
        error, description, resource_metadata_url
    );

    let body = serde_json::json!({
        "error": error,
        "error_description": description,
        "resource_metadata": resource_metadata_url,
    });

    info!(
        "[Auth] Returning 401 with resource_metadata={}",
        resource_metadata_url
    );

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, www_authenticate)],
        axum::Json(body),
    )
        .into_response()
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
    fn test_expired_token() {
        let secret = b"test_secret_key_32_bytes_long!!";
        // Create token that expired 1 hour ago
        let token = create_access_token("test_client", None, -3600, secret);

        let claims = validate_token(&token, secret);
        assert!(claims.is_none());
    }
}
