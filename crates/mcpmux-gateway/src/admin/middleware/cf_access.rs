//! Cloudflare Access JWT validation for the admin HTTP server.
//!
//! When `trust_cf_access` is enabled, requests must include a valid
//! `CF-Access-Jwt-Assertion` header signed by Cloudflare team certs, or
//! matching `CF-Access-Client-Id` / `CF-Access-Client-Secret` service-token
//! headers when `MCPMUX_CF_ACCESS_CLIENT_ID` and `MCPMUX_CF_ACCESS_CLIENT_SECRET`
//! are set in the environment.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use thiserror::Error;
use tracing::debug;

#[cfg(any(test, feature = "test-utils"))]
use std::sync::Arc;

use super::super::config::CF_ACCESS_JWT_HEADER;
use super::super::router::AdminState;

/// Service-token headers Cloudflare Access accepts at the edge.
pub const CF_ACCESS_CLIENT_ID_HEADER: &str = "cf-access-client-id";
pub const CF_ACCESS_CLIENT_SECRET_HEADER: &str = "cf-access-client-secret";

/// Env vars for optional origin-side service-token auth (tunnel smoke / automation).
pub const CF_ACCESS_CLIENT_ID_ENV: &str = "MCPMUX_CF_ACCESS_CLIENT_ID";
pub const CF_ACCESS_CLIENT_SECRET_ENV: &str = "MCPMUX_CF_ACCESS_CLIENT_SECRET";

#[cfg(any(test, feature = "test-utils"))]
/// PEM-encoded RSA public key used only by test helpers.
const TEST_RSA_PUBLIC_PEM: &str =
    include_str!("../../../../../tests/fixtures/cf_access_test_pubkey.pem");

#[cfg(any(test, feature = "test-utils"))]
/// PEM-encoded RSA private key used only by test helpers.
const TEST_RSA_PRIVATE_PEM: &str =
    include_str!("../../../../../tests/fixtures/cf_access_test_private.pem");

/// Errors from CF Access JWT validation.
#[derive(Debug, Error)]
pub enum CfAccessError {
    /// JWT header or signature could not be parsed or verified.
    #[error("invalid JWT: {0}")]
    InvalidJwt(String),
    /// No matching decoding key for the token `kid`.
    #[error("unknown key id: {0}")]
    UnknownKeyId(String),
    /// Cert fetch or configuration error.
    #[error("{0}")]
    Config(String),
}

/// Validated Cloudflare Access JWT claims (subset used for checks).
#[derive(Debug, Deserialize)]
pub struct CfAccessClaims {
    /// Subject (user email or service identity).
    pub sub: String,
    /// Issuer (`https://<team>.cloudflareaccess.com`).
    pub iss: String,
    /// Audience (application AUD tag).
    pub aud: serde_json::Value,
    /// Expiration (unix seconds).
    pub exp: i64,
}

impl std::fmt::Debug for CfAccessValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CfAccessValidator")
            .field("keys", &self.keys.len())
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .finish()
    }
}

/// Validates `CF-Access-Jwt-Assertion` tokens against Cloudflare team certs.
pub struct CfAccessValidator {
    keys: Vec<DecodingKey>,
    issuer: Option<String>,
    audience: Option<String>,
}

impl CfAccessValidator {
    /// Build a validator from PEM-encoded RSA public certificates.
    pub fn from_pem_certs(
        certs: Vec<String>,
        issuer: Option<String>,
        audience: Option<String>,
    ) -> Result<Self, CfAccessError> {
        let mut keys = Vec::new();
        for cert in certs {
            let key = DecodingKey::from_rsa_pem(cert.as_bytes())
                .map_err(|e| CfAccessError::Config(format!("invalid PEM cert: {e}")))?;
            keys.push(key);
        }
        if keys.is_empty() {
            return Err(CfAccessError::Config("no certificates provided".into()));
        }
        Ok(Self {
            keys,
            issuer,
            audience,
        })
    }

    /// Fetch team certs from Cloudflare and build a validator.
    ///
    /// CF's `/cdn-cgi/access/certs` endpoint returns three representations of
    /// the same signing material:
    /// - `keys` — JWKS format (`{kid, kty, alg, use, e, n}`) — standard JWT key
    ///   format directly consumable by `DecodingKey::from_jwk`
    /// - `public_cert` / `public_certs` — X.509 certificates in PEM form
    ///
    /// We use `keys` because `jsonwebtoken::DecodingKey::from_rsa_pem` does
    /// NOT support X.509 certificate PEM — only PKCS#1 RSA Public Key or
    /// PKCS#8 SubjectPublicKeyInfo. Feeding it a full X.509 cert produces a
    /// malformed key that fails every signature verification with
    /// `InvalidSignature`. JWKS sidesteps that entirely.
    pub async fn from_team_domain(
        team_domain: &str,
        audience: Option<String>,
    ) -> Result<Self, CfAccessError> {
        let url = format!("https://{team_domain}.cloudflareaccess.com/cdn-cgi/access/certs");
        let issuer = format!("https://{team_domain}.cloudflareaccess.com");
        let response = reqwest::get(&url)
            .await
            .map_err(|e| CfAccessError::Config(format!("cert fetch failed: {e}")))?;
        if !response.status().is_success() {
            return Err(CfAccessError::Config(format!(
                "cert fetch returned {}",
                response.status()
            )));
        }
        let body: CertsResponse = response
            .json()
            .await
            .map_err(|e| CfAccessError::Config(format!("cert JSON parse failed: {e}")))?;
        if body.keys.is_empty() {
            return Err(CfAccessError::Config(
                "CF certs response has no JWKS keys".into(),
            ));
        }
        let mut keys = Vec::with_capacity(body.keys.len());
        for jwk in body.keys {
            let key = DecodingKey::from_jwk(&jwk)
                .map_err(|e| CfAccessError::Config(format!("invalid JWK from CF Access: {e}")))?;
            keys.push(key);
        }
        Ok(Self {
            keys,
            issuer: Some(issuer),
            audience,
        })
    }

    /// Validate a JWT string and return decoded claims.
    ///
    /// `validate_aud` is gated on whether an audience is configured. The
    /// `jsonwebtoken` crate defaults `validate_aud` to `true`, which rejects
    /// tokens carrying an `aud` claim when the validator's audience set is
    /// empty — even when the signature and issuer are valid. CF Access JWTs
    /// always carry `aud` (the application UUID), so leaving the default in
    /// place breaks every token when the operator has not pasted the AUD tag.
    pub fn validate(&self, token: &str) -> Result<CfAccessClaims, CfAccessError> {
        let header = decode_header(token).map_err(|e| CfAccessError::InvalidJwt(e.to_string()))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = true;
        validation.validate_aud = self.audience.is_some();
        if let Some(ref iss) = self.issuer {
            validation.set_issuer(&[iss.as_str()]);
        }
        if let Some(ref aud) = self.audience {
            validation.set_audience(&[aud.as_str()]);
        }

        let mut last_err: Option<CfAccessError> = None;
        for key in &self.keys {
            match decode::<CfAccessClaims>(token, key, &validation) {
                Ok(token_data) => {
                    debug!(sub = %token_data.claims.sub, "CF Access JWT validated");
                    return Ok(token_data.claims);
                }
                Err(e) => {
                    last_err = Some(CfAccessError::InvalidJwt(e.to_string()));
                }
            }
        }

        if let Some(err) = last_err {
            return Err(err);
        }

        let kid = header.kid.unwrap_or_else(|| "unknown".into());
        Err(CfAccessError::UnknownKeyId(kid))
    }
}

/// Cloudflare Access `/cdn-cgi/access/certs` response shape.
///
/// We deserialize only `keys` (the JWKS) and ignore the X.509 `public_cert` /
/// `public_certs` fields. See `from_team_domain` for why.
#[derive(Debug, Deserialize)]
struct CertsResponse {
    #[serde(default)]
    keys: Vec<jsonwebtoken::jwk::Jwk>,
}

/// Axum middleware: require valid CF Access JWT when enabled in config.
pub async fn cf_access_middleware(
    State(state): State<AdminState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.trust_cf_access {
        return next.run(request).await;
    }

    let Some(validator) = state.cf_validator.as_ref() else {
        return cf_access_unauthorized("CF Access validation not configured");
    };

    let token = headers
        .get(CF_ACCESS_JWT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match token {
        Some(jwt) => match validator.validate(jwt) {
            Ok(_) => next.run(request).await,
            Err(e) => {
                debug!(error = %e, "CF Access JWT rejected");
                cf_access_unauthorized("invalid CF Access token")
            }
        },
        None if service_token_matches(&headers) => {
            debug!("CF Access service token accepted from env-configured credentials");
            next.run(request).await
        }
        None => cf_access_unauthorized("missing CF-Access-Jwt-Assertion"),
    }
}

/// Return true when request service-token headers match env-configured credentials.
pub fn service_token_matches(headers: &HeaderMap) -> bool {
    let Ok(expected_id) = std::env::var(CF_ACCESS_CLIENT_ID_ENV) else {
        return false;
    };
    let Ok(expected_secret) = std::env::var(CF_ACCESS_CLIENT_SECRET_ENV) else {
        return false;
    };
    if expected_id.is_empty() || expected_secret.is_empty() {
        return false;
    }

    let Some(id) = header_value(headers, CF_ACCESS_CLIENT_ID_HEADER) else {
        return false;
    };
    let Some(secret) = header_value(headers, CF_ACCESS_CLIENT_SECRET_HEADER) else {
        return false;
    };

    constant_time_eq(id, &expected_id) && constant_time_eq(secret, &expected_secret)
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    left.as_bytes().ct_eq(right.as_bytes()).into()
}

fn cf_access_unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}

/// Test-only validator backed by the repo fixture key pair.
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub fn test_validator() -> Arc<CfAccessValidator> {
    Arc::new(
        CfAccessValidator::from_pem_certs(
            vec![TEST_RSA_PUBLIC_PEM.to_string()],
            Some("https://test.cloudflareaccess.com".into()),
            Some("test-audience".into()),
        )
        .expect("test validator"),
    )
}

/// Test-only signed JWT accepted by [`test_validator`].
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub fn test_valid_jwt() -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let claims = serde_json::json!({
        "sub": "test@example.com",
        "iss": "https://test.cloudflareaccess.com",
        "aud": "test-audience",
        "exp": chrono::Utc::now().timestamp() + 3600,
    });
    let key = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_PEM.as_bytes()).expect("test private key");
    encode(&Header::new(Algorithm::RS256), &claims, &key).expect("sign test jwt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_test_fixture_jwt() {
        let validator = test_validator();
        let jwt = test_valid_jwt();
        assert!(validator.validate(&jwt).is_ok());
    }

    #[test]
    fn validate_rejects_missing_signature() {
        let validator = test_validator();
        let err = validator.validate("not.a.jwt").unwrap_err();
        assert!(matches!(err, CfAccessError::InvalidJwt(_)));
    }

    #[test]
    fn validate_rejects_wrong_audience() {
        let validator = CfAccessValidator::from_pem_certs(
            vec![TEST_RSA_PUBLIC_PEM.to_string()],
            Some("https://test.cloudflareaccess.com".into()),
            Some("other-audience".into()),
        )
        .unwrap();
        let jwt = test_valid_jwt();
        let err = validator.validate(&jwt).unwrap_err();
        assert!(matches!(err, CfAccessError::InvalidJwt(_)));
    }

    #[test]
    fn from_pem_certs_rejects_empty_list() {
        let err = CfAccessValidator::from_pem_certs(vec![], None, None).unwrap_err();
        assert!(matches!(err, CfAccessError::Config(_)));
    }

    #[test]
    fn from_pem_certs_rejects_invalid_pem() {
        let err =
            CfAccessValidator::from_pem_certs(vec!["not-a-cert".into()], None, None).unwrap_err();
        assert!(matches!(err, CfAccessError::Config(_)));
    }

    #[test]
    fn service_token_matches_env_headers() {
        std::env::set_var(CF_ACCESS_CLIENT_ID_ENV, "svc-id");
        std::env::set_var(CF_ACCESS_CLIENT_SECRET_ENV, "svc-secret");

        let mut headers = HeaderMap::new();
        headers.insert(CF_ACCESS_CLIENT_ID_HEADER, "svc-id".parse().unwrap());
        headers.insert(
            CF_ACCESS_CLIENT_SECRET_HEADER,
            "svc-secret".parse().unwrap(),
        );
        assert!(service_token_matches(&headers));

        headers.insert(CF_ACCESS_CLIENT_SECRET_HEADER, "wrong".parse().unwrap());
        assert!(!service_token_matches(&headers));

        std::env::remove_var(CF_ACCESS_CLIENT_ID_ENV);
        std::env::remove_var(CF_ACCESS_CLIENT_SECRET_ENV);
    }
}
