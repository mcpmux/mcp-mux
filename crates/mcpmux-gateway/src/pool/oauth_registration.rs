//! Outbound Dynamic Client Registration (RFC 7591)
//!
//! McpMux registers itself with a backend MCP server's authorization server here
//! rather than through rmcp's `register_client`, whose request can't carry
//! `client_uri` or `logo_uri`. Without them a consent page shows McpMux with no
//! logo or homepage link (issue #141). The CIMD path doesn't come through here:
//! there the server reads the same fields from the published metadata document.
//!
//! The registration response's `client_secret`, if the server issued one, is
//! returned so the caller can store it. rmcp only keeps it in memory, so without
//! that a confidential client loses its secret on restart and token refresh
//! fails with `invalid_client`.

use std::time::Duration;

use mcpmux_core::branding;
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
}

impl std::fmt::Debug for RegisteredClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredClient")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[redacted]"),
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

#[derive(Deserialize)]
struct RegistrationResponse {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

/// Register McpMux as an OAuth client at `registration_endpoint`.
///
/// Sends `client_name`, `client_uri` and `logo_uri` so the consent page can show
/// who is asking. If the server rejects that request with a 4xx, it retries once
/// with only the fields McpMux sent before, so a strict server that doesn't know
/// the extra metadata still works.
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
        Err(RegistrationError::Rejected { status, body }) if status.is_client_error() => {
            warn!(
                "[OAuth] Registration with client metadata rejected (HTTP {}: {}); \
                 retrying without client_uri/logo_uri/application_type",
                status, body
            );
            let minimal = RegistrationRequest::minimal(client_name, redirect_uri, scopes);
            post_registration(&http, registration_endpoint, &minimal)
                .await
                .map_err(RegistrationError::into_auth_error)
        }
        result => result.map_err(RegistrationError::into_auth_error),
    }
}

enum RegistrationError {
    Rejected {
        status: reqwest::StatusCode,
        body: String,
    },
    Other(String),
}

impl RegistrationError {
    fn into_auth_error(self) -> AuthError {
        match self {
            RegistrationError::Rejected { status, body } => {
                AuthError::RegistrationFailed(format!("HTTP {}: {}", status, body))
            }
            RegistrationError::Other(msg) => AuthError::RegistrationFailed(msg),
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

    Ok(RegisteredClient {
        client_id: parsed.client_id,
        // Some servers send `"client_secret": ""` for a public client. Sending an
        // empty secret would make the token request fail, so treat it as none.
        client_secret: parsed.client_secret.filter(|s| !s.is_empty()),
    })
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

    #[test]
    fn registered_client_debug_redacts_secret() {
        let client = RegisteredClient {
            client_id: "client-123".to_string(),
            client_secret: Some("s3cr3t-value".to_string()),
        };
        let debug = format!("{:?}", client);
        assert!(!debug.contains("s3cr3t-value"));
        assert!(debug.contains("client-123"));
    }
}
