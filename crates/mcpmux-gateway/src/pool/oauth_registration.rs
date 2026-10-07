//! Outbound Dynamic Client Registration (RFC 7591)
//!
//! McpMux registers itself with a backend MCP server's authorization server here
//! rather than through rmcp's `register_client`, whose request can't carry
//! `client_uri` or `logo_uri`. Without them a consent page shows McpMux with no
//! logo or homepage link (issue #141). The CIMD path doesn't come through here:
//! there the server reads the same fields from the published metadata document.
//!
//! The registration response's `client_secret`, if the server issued one, is
//! returned so the caller can store it, with when it expires and the
//! `token_endpoint_auth_method` the server registered the client with. rmcp only
//! keeps the secret in memory, so without that a confidential client loses it on
//! restart and token refresh fails with `invalid_client`.

use std::time::Duration;

use chrono::{DateTime, Utc};
use mcpmux_core::{branding, OutboundOAuthRegistration};
use reqwest::StatusCode;
use rmcp::transport::auth::AuthError;
use serde::{Deserialize, Serialize};
use tracing::warn;

const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);

/// A client registered with an authorization server
#[derive(Clone)]
pub struct RegisteredClient {
    pub client_id: String,
    /// `None` for a public client
    pub client_secret: Option<String>,
    /// When `client_secret` expires; `None` if it never does
    pub client_secret_expires_at: Option<DateTime<Utc>>,
    /// How the server registered the client to authenticate at the token endpoint
    /// (e.g. `client_secret_post`); `None` if the server didn't say
    pub token_endpoint_auth_method: Option<String>,
}

impl From<&OutboundOAuthRegistration> for RegisteredClient {
    fn from(registration: &OutboundOAuthRegistration) -> Self {
        Self {
            client_id: registration.client_id.clone(),
            client_secret: registration.client_secret.clone(),
            client_secret_expires_at: registration.client_secret_expires_at,
            token_endpoint_auth_method: registration.token_endpoint_auth_method.clone(),
        }
    }
}

impl std::fmt::Debug for RegisteredClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredClient")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[redacted]"),
            )
            .field("client_secret_expires_at", &self.client_secret_expires_at)
            .field(
                "token_endpoint_auth_method",
                &self.token_endpoint_auth_method,
            )
            .finish()
    }
}

/// RFC 7591 client metadata sent to the registration endpoint
#[derive(Debug, Serialize)]
struct RegistrationRequest<'a> {
    client_name: &'a str,
    redirect_uris: Vec<&'a str>,
    grant_types: [&'static str; 2],
    response_types: [&'static str; 1],
    /// McpMux is a native app on the user's machine and can't keep a secret, so it
    /// asks to be a public client. A server may still issue a secret.
    token_endpoint_auth_method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logo_uri: Option<String>,
    /// OIDC Dynamic Client Registration: lets servers that default to `web` accept
    /// the loopback redirect URI (MCP SEP-837)
    #[serde(skip_serializing_if = "Option::is_none")]
    application_type: Option<&'static str>,
}

impl<'a> RegistrationRequest<'a> {
    /// The full request McpMux sends
    fn branded(client_name: &'a str, redirect_uri: &'a str, scopes: &[String]) -> Self {
        Self {
            client_uri: Some(branding::outbound_oauth_client_uri()),
            logo_uri: Some(branding::outbound_oauth_logo_uri()),
            application_type: Some("native"),
            ..Self::minimal(client_name, redirect_uri, scopes)
        }
    }

    /// Only the fields rmcp sent before the branded request, for servers that
    /// reject the optional metadata
    fn minimal(client_name: &'a str, redirect_uri: &'a str, scopes: &[String]) -> Self {
        Self {
            client_name,
            redirect_uris: vec![redirect_uri],
            grant_types: ["authorization_code", "refresh_token"],
            response_types: ["code"],
            token_endpoint_auth_method: "none",
            scope: (!scopes.is_empty()).then(|| scopes.join(" ")),
            client_uri: None,
            logo_uri: None,
            application_type: None,
        }
    }
}

/// The fields McpMux uses from an RFC 7591 registration response. The optional
/// ones are read as raw JSON so a malformed value can't fail the registration.
#[derive(Deserialize)]
struct RegistrationResponse {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    client_secret_expires_at: Option<serde_json::Value>,
    #[serde(default)]
    token_endpoint_auth_method: Option<serde_json::Value>,
}

impl RegistrationResponse {
    fn into_client(self) -> RegisteredClient {
        // Some servers send `"client_secret": ""` for a public client. Sending an
        // empty secret would make the token request fail, so treat it as none.
        let client_secret = self.client_secret.filter(|s| !s.is_empty());
        // RFC 7591 §3.2.1: seconds since the epoch, or 0 if the secret never expires
        let client_secret_expires_at = client_secret
            .as_ref()
            .and(self.client_secret_expires_at.as_ref())
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
            .filter(|&secs| secs > 0)
            .and_then(|secs| DateTime::from_timestamp(secs, 0));
        let token_endpoint_auth_method = self
            .token_endpoint_auth_method
            .as_ref()
            .and_then(|v| v.as_str())
            .filter(|method| !method.is_empty())
            .map(str::to_owned);

        RegisteredClient {
            client_id: self.client_id,
            client_secret,
            client_secret_expires_at,
            token_endpoint_auth_method,
        }
    }
}

/// Whether a rejected registration may succeed without the optional client metadata.
///
/// RFC 7591 §3.2.2 answers a request it won't accept, such as one with
/// `invalid_client_metadata`, with HTTP 400; some servers use 422 for validation
/// errors. Other statuses (401/403 for a protected registration endpoint, 429,
/// 5xx) won't change with fewer fields, so they aren't retried.
fn rejects_client_metadata(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    )
}

/// Register McpMux as an OAuth client at `registration_endpoint`.
///
/// Sends `client_name`, `client_uri` and `logo_uri` so the consent page can show
/// who is asking. If the server rejects that request as invalid (HTTP 400 or 422),
/// it retries once with only the fields McpMux sent before, so a strict server
/// that doesn't know the extra metadata still works.
pub async fn register_client(
    registration_endpoint: &str,
    client_name: &str,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<RegisteredClient, AuthError> {
    let http = reqwest::Client::builder()
        .timeout(REGISTRATION_TIMEOUT)
        .build()
        .map_err(|e| AuthError::RegistrationFailed(format!("HTTP client error: {}", e)))?;

    let branded = RegistrationRequest::branded(client_name, redirect_uri, scopes);
    match post_registration(&http, registration_endpoint, &branded).await {
        Err(rejection @ RegistrationError::Rejected { status, .. })
            if rejects_client_metadata(status) =>
        {
            warn!(
                "[OAuth] Registration with client metadata rejected ({}); \
                 retrying without client_uri/logo_uri/application_type",
                rejection
            );
            let minimal = RegistrationRequest::minimal(client_name, redirect_uri, scopes);
            post_registration(&http, registration_endpoint, &minimal)
                .await
                .map_err(|retry_error| {
                    // Keep both reasons: the first one may be the one that matters
                    AuthError::RegistrationFailed(format!(
                        "{}; retried without client_uri/logo_uri/application_type: {}",
                        rejection, retry_error
                    ))
                })
        }
        result => result.map_err(|e| AuthError::RegistrationFailed(e.to_string())),
    }
}

enum RegistrationError {
    Rejected { status: StatusCode, body: String },
    Other(String),
}

impl std::fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistrationError::Rejected { status, body } => write!(f, "HTTP {}: {}", status, body),
            RegistrationError::Other(msg) => f.write_str(msg),
        }
    }
}

async fn post_registration(
    http: &reqwest::Client,
    registration_endpoint: &str,
    request: &RegistrationRequest<'_>,
) -> Result<RegisteredClient, RegistrationError> {
    let response = http
        .post(registration_endpoint)
        .json(request)
        .send()
        .await
        .map_err(|e| RegistrationError::Other(format!("HTTP request error: {}", e)))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "cannot get error details".to_string());
        return Err(RegistrationError::Rejected { status, body });
    }

    let parsed: RegistrationResponse = response
        .json()
        .await
        .map_err(|e| RegistrationError::Other(format!("analyze response error: {}", e)))?;

    Ok(parsed.into_client())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_json(request: &RegistrationRequest<'_>) -> serde_json::Value {
        serde_json::to_value(request).unwrap()
    }

    #[test]
    fn branded_request_carries_client_identity() {
        let scopes = vec!["boards:read".to_string(), "boards:write".to_string()];
        let json = to_json(&RegistrationRequest::branded(
            "McpMux (Work)",
            "http://127.0.0.1:45819/oauth2redirect",
            &scopes,
        ));

        assert_eq!(json["client_name"], "McpMux (Work)");
        assert_eq!(json["client_uri"], branding::outbound_oauth_client_uri());
        assert_eq!(json["logo_uri"], branding::outbound_oauth_logo_uri());
        assert_eq!(json["application_type"], "native");
        assert_eq!(
            json["redirect_uris"],
            serde_json::json!(["http://127.0.0.1:45819/oauth2redirect"])
        );
        assert_eq!(
            json["grant_types"],
            serde_json::json!(["authorization_code", "refresh_token"])
        );
        assert_eq!(json["response_types"], serde_json::json!(["code"]));
        assert_eq!(json["token_endpoint_auth_method"], "none");
        assert_eq!(json["scope"], "boards:read boards:write");
    }

    #[test]
    fn minimal_request_matches_the_previous_rmcp_request() {
        let json = to_json(&RegistrationRequest::minimal(
            "McpMux",
            "http://127.0.0.1:45819/oauth2redirect",
            &[],
        ));

        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "client_name",
                "grant_types",
                "redirect_uris",
                "response_types",
                "token_endpoint_auth_method"
            ]
        );
    }

    fn parse_response(json: serde_json::Value) -> RegisteredClient {
        serde_json::from_value::<RegistrationResponse>(json)
            .unwrap()
            .into_client()
    }

    #[test]
    fn response_with_secret_expiry_and_auth_method() {
        let client = parse_response(serde_json::json!({
            "client_id": "client-123",
            "client_secret": "s3cr3t-value",
            "client_secret_expires_at": 1_900_000_000,
            "token_endpoint_auth_method": "client_secret_post",
        }));

        assert_eq!(client.client_id, "client-123");
        assert_eq!(client.client_secret.as_deref(), Some("s3cr3t-value"));
        assert_eq!(
            client.client_secret_expires_at,
            DateTime::from_timestamp(1_900_000_000, 0)
        );
        assert_eq!(
            client.token_endpoint_auth_method.as_deref(),
            Some("client_secret_post")
        );
    }

    #[test]
    fn secret_expiry_of_zero_means_never() {
        let client = parse_response(serde_json::json!({
            "client_id": "client-123",
            "client_secret": "s3cr3t-value",
            "client_secret_expires_at": 0,
        }));
        assert_eq!(client.client_secret_expires_at, None);
    }

    #[test]
    fn secret_expiry_sent_as_a_string_is_read() {
        let client = parse_response(serde_json::json!({
            "client_id": "client-123",
            "client_secret": "s3cr3t-value",
            "client_secret_expires_at": "1900000000",
        }));
        assert_eq!(
            client.client_secret_expires_at,
            DateTime::from_timestamp(1_900_000_000, 0)
        );
    }

    #[test]
    fn malformed_optional_fields_do_not_fail_the_registration() {
        let client = parse_response(serde_json::json!({
            "client_id": "client-123",
            "client_secret": "s3cr3t-value",
            "client_secret_expires_at": {"unexpected": true},
            "token_endpoint_auth_method": 42,
        }));
        assert_eq!(client.client_id, "client-123");
        assert_eq!(client.client_secret_expires_at, None);
        assert_eq!(client.token_endpoint_auth_method, None);
    }

    #[test]
    fn public_client_has_no_secret_or_expiry() {
        let client = parse_response(serde_json::json!({
            "client_id": "client-123",
            "client_secret": "",
            "client_secret_expires_at": 1_900_000_000,
            "token_endpoint_auth_method": "none",
        }));
        assert_eq!(client.client_secret, None);
        assert_eq!(client.client_secret_expires_at, None);
        assert_eq!(client.token_endpoint_auth_method.as_deref(), Some("none"));
    }

    #[test]
    fn only_invalid_request_statuses_are_retried() {
        assert!(rejects_client_metadata(StatusCode::BAD_REQUEST));
        assert!(rejects_client_metadata(StatusCode::UNPROCESSABLE_ENTITY));
        for status in [
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert!(!rejects_client_metadata(status), "{status} retried");
        }
    }

    #[test]
    fn registered_client_debug_redacts_secret() {
        let client = RegisteredClient {
            client_id: "client-123".to_string(),
            client_secret: Some("s3cr3t-value".to_string()),
            client_secret_expires_at: None,
            token_endpoint_auth_method: None,
        };
        let debug = format!("{:?}", client);
        assert!(!debug.contains("s3cr3t-value"));
        assert!(debug.contains("client-123"));
    }
}
