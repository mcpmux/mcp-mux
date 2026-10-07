//! Outbound Dynamic Client Registration tests (issue #141)
//!
//! When an authorization server doesn't support CIMD, McpMux registers itself with
//! RFC 7591 DCR. The request must carry McpMux's `client_uri` and `logo_uri` so the
//! consent page can show who is asking, and a `client_secret` the server issues
//! must be kept and sent on every later token request, including a refresh after
//! the app restarts.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use mcpmux_core::{branding, CredentialRepository, OutboundOAuthRepository};
use mcpmux_gateway::{OAuthCompleteEvent, OAuthInitResult, OutboundOAuthManager};
use tests::mocks::{MockCredentialRepository, MockOutboundOAuthRepository};
use uuid::Uuid;
use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const SERVER_ID: &str = "confidential-server";
const CLIENT_ID: &str = "dcr-client-id";
const CLIENT_SECRET: &str = "dcr-client-secret";

/// Mount RFC 9728 / RFC 8414 metadata for an authorization server that supports DCR
/// but not CIMD, and only authenticates clients with HTTP Basic (like Miro, Figma
/// and GitLab advertise).
async fn mount_dcr_metadata(mock_server: &MockServer) {
    let base = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource/mcp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": format!("{}/mcp", base),
            "authorization_servers": [base],
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": base,
            "authorization_endpoint": format!("{}/authorize", base),
            "token_endpoint": format!("{}/token", base),
            "registration_endpoint": format!("{}/register", base),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["client_secret_basic"],
            "scopes_supported": ["boards:read"],
        })))
        .mount(mock_server)
        .await;
}

fn registration_response(client_secret: Option<&str>) -> ResponseTemplate {
    let mut body = serde_json::json!({
        "client_id": CLIENT_ID,
        "redirect_uris": [],
    });
    if let Some(secret) = client_secret {
        body["client_secret"] = serde_json::json!(secret);
    }
    ResponseTemplate::new(201).set_body_json(body)
}

/// Tokens from the code exchange expire at once, so the next use refreshes them;
/// refreshed tokens last an hour
fn token_response(access_token: &str) -> ResponseTemplate {
    let expires_in = if access_token == "access-2" { 3600 } else { 1 };
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": expires_in,
        "refresh_token": "refresh-token",
    }))
}

fn basic_auth(client_id: &str, client_secret: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:{client_secret}"))
    )
}

/// Matches a request whose JSON body has none of `keys`
struct BodyLacksKeys(&'static [&'static str]);

impl wiremock::Match for BodyLacksKeys {
    fn matches(&self, request: &Request) -> bool {
        serde_json::from_slice::<serde_json::Value>(&request.body)
            .map(|body| self.0.iter().all(|key| body.get(*key).is_none()))
            .unwrap_or(false)
    }
}

struct Flow {
    manager: OutboundOAuthManager,
    credential_repo: Arc<MockCredentialRepository>,
    oauth_repo: Arc<MockOutboundOAuthRepository>,
    space_id: Uuid,
    server_url: String,
}

impl Flow {
    fn new(mock_server: &MockServer) -> Self {
        Self {
            manager: OutboundOAuthManager::new(),
            credential_repo: Arc::new(MockCredentialRepository::new()),
            oauth_repo: Arc::new(MockOutboundOAuthRepository::new()),
            space_id: Uuid::new_v4(),
            server_url: format!("{}/mcp", mock_server.uri()),
        }
    }

    async fn start(&self) -> anyhow::Result<url::Url> {
        let result = self
            .manager
            .start_oauth_flow(
                self.credential_repo.clone(),
                self.oauth_repo.clone(),
                self.space_id,
                SERVER_ID,
                &self.server_url,
            )
            .await?;
        match result {
            OAuthInitResult::Initiated { auth_url } => Ok(url::Url::parse(&auth_url)?),
            other => anyhow::bail!("expected OAuthInitResult::Initiated, got {:?}", other),
        }
    }

    /// Start a flow, then play the browser: send the authorization code to the
    /// loopback redirect URI and wait for McpMux to finish the token exchange
    async fn sign_in(&self) -> OAuthCompleteEvent {
        let mut events = self.manager.subscribe();
        let auth_url = self.start().await.expect("start_oauth_flow should succeed");

        let redirect_uri = query_param(&auth_url, "redirect_uri").expect("redirect_uri param");
        let state = query_param(&auth_url, "state").expect("state param");
        let mut callback = url::Url::parse(&redirect_uri).unwrap();
        callback
            .query_pairs_mut()
            .append_pair("code", "auth-code")
            .append_pair("state", &state);
        let response = reqwest::get(callback.as_str())
            .await
            .expect("loopback callback reachable");
        assert!(response.status().is_success());

        tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .expect("OAuth flow should complete")
            .expect("completion event")
    }

    async fn registration(&self) -> Option<mcpmux_core::OutboundOAuthRegistration> {
        self.oauth_repo
            .get(&self.space_id, SERVER_ID)
            .await
            .unwrap()
    }
}

fn query_param(url: &url::Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

#[tokio::test]
async fn registration_request_carries_mcpmux_logo_and_homepage() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .and(body_partial_json(serde_json::json!({
            "client_name": branding::outbound_oauth_client_name(),
            "client_uri": branding::outbound_oauth_client_uri(),
            "logo_uri": branding::outbound_oauth_logo_uri(),
            "application_type": "native",
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "scope": "boards:read",
        })))
        .respond_with(registration_response(None))
        .expect(1)
        .mount(&mock_server)
        .await;

    let auth_url = Flow::new(&mock_server).start().await.unwrap();

    assert_eq!(
        query_param(&auth_url, "client_id").as_deref(),
        Some(CLIENT_ID)
    );
    assert!(query_param(&auth_url, "code_challenge").is_some());
}

#[tokio::test]
async fn registration_retries_without_client_metadata_when_rejected() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    // A strict server that rejects metadata it doesn't know
    Mock::given(method("POST"))
        .and(path("/register"))
        .and(body_partial_json(serde_json::json!({
            "logo_uri": branding::outbound_oauth_logo_uri(),
        })))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_client_metadata",
        })))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .and(BodyLacksKeys(&[
            "logo_uri",
            "client_uri",
            "application_type",
        ]))
        .respond_with(registration_response(None))
        .expect(1)
        .mount(&mock_server)
        .await;

    let auth_url = Flow::new(&mock_server).start().await.unwrap();

    assert_eq!(
        query_param(&auth_url, "client_id").as_deref(),
        Some(CLIENT_ID)
    );
}

#[tokio::test]
async fn registration_error_when_server_rejects_both_requests() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
        .expect(2)
        .mount(&mock_server)
        .await;

    let err = Flow::new(&mock_server).start().await.unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("Client registration failed") && message.contains("403"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn registration_server_error_is_not_retried() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&mock_server)
        .await;

    assert!(Flow::new(&mock_server).start().await.is_err());
}

/// The full lifecycle of a confidential client: the secret from DCR is used for
/// the code exchange, saved with the registration, and used again for the refresh
/// after a restart (a new OutboundOAuthManager reading from storage).
#[tokio::test]
async fn client_secret_is_kept_for_refresh_after_restart() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(header(
            "authorization",
            basic_auth(CLIENT_ID, CLIENT_SECRET),
        ))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(token_response("access-1"))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(header(
            "authorization",
            basic_auth(CLIENT_ID, CLIENT_SECRET),
        ))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(token_response("access-2"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);

    let registration = flow.registration().await.expect("registration saved");
    assert_eq!(registration.client_id, CLIENT_ID);
    assert_eq!(registration.client_secret.as_deref(), Some(CLIENT_SECRET));
    assert!(registration.metadata.is_some());

    // Restart: a fresh manager with nothing in memory, only what was stored
    let restarted = OutboundOAuthManager::new();
    let access_token = restarted
        .get_access_token(
            flow.credential_repo.clone(),
            flow.oauth_repo.clone(),
            flow.space_id,
            SERVER_ID,
            &flow.server_url,
        )
        .await
        .expect("refresh with the stored client secret should succeed");
    assert_eq!(access_token, "access-2");
}

/// Signing in again reuses the saved registration, and must send its secret
#[tokio::test]
async fn reused_registration_sends_its_client_secret() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(header(
            "authorization",
            basic_auth(CLIENT_ID, CLIENT_SECRET),
        ))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(token_response("access"))
        .expect(2)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    assert!(flow.sign_in().await.success);

    // Sign out (tokens gone, registration kept), then sign in again
    flow.credential_repo
        .clear_tokens(&flow.space_id, SERVER_ID)
        .await
        .unwrap();
    let event = flow.sign_in().await;
    assert!(event.success, "second sign-in failed: {:?}", event.error);
}

/// A confidential client saved before McpMux stored client secrets fails the code
/// exchange on every reuse. That registration is dropped, so the next sign-in
/// registers a new client instead of staying stuck.
#[tokio::test]
async fn reused_registration_without_its_secret_is_dropped_after_failed_exchange() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(2)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(header(
            "authorization",
            basic_auth(CLIENT_ID, CLIENT_SECRET),
        ))
        .respond_with(token_response("access"))
        .mount(&mock_server)
        .await;
    // Anything without the client's credentials is rejected
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "invalid_client",
        })))
        .with_priority(10)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    assert!(flow.sign_in().await.success);

    // Simulate a registration saved by an older release: same client, no secret
    let mut legacy = flow.registration().await.unwrap();
    legacy.client_secret = None;
    flow.oauth_repo.save(&legacy).await.unwrap();
    flow.credential_repo
        .clear_tokens(&flow.space_id, SERVER_ID)
        .await
        .unwrap();

    let event = flow.sign_in().await;
    assert!(!event.success, "exchange without the secret should fail");
    assert!(
        flow.registration().await.is_none(),
        "the unusable registration should be dropped"
    );

    // The next sign-in registers again (second /register call) and succeeds
    let event = flow.sign_in().await;
    assert!(event.success, "fresh sign-in failed: {:?}", event.error);
    assert_eq!(
        flow.registration().await.unwrap().client_secret.as_deref(),
        Some(CLIENT_SECRET)
    );
}

/// A public client (no secret in the DCR response, as Miro, GitLab, Linear and
/// Atlassian answer) still authenticates with its client_id alone
#[tokio::test]
async fn public_client_refreshes_without_a_secret() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some("")))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(format!("client_id={CLIENT_ID}")))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(token_response("access-1"))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(format!("client_id={CLIENT_ID}")))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(token_response("access-2"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);
    assert_eq!(flow.registration().await.unwrap().client_secret, None);

    let access_token = OutboundOAuthManager::new()
        .get_access_token(
            flow.credential_repo.clone(),
            flow.oauth_repo.clone(),
            flow.space_id,
            SERVER_ID,
            &flow.server_url,
        )
        .await
        .expect("public client refresh should succeed");
    assert_eq!(access_token, "access-2");
}

/// Mount a minimal streamable-HTTP MCP endpoint at `/mcp` that only answers
/// requests bearing `access_token`
async fn mount_mcp_endpoint(mock_server: &MockServer, access_token: &str) {
    let bearer = format!("Bearer {access_token}");

    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(header("authorization", bearer.as_str()))
        .and(body_partial_json(
            serde_json::json!({"method": "initialize"}),
        ))
        .respond_with(|request: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": body["id"],
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "serverInfo": {"name": "mock-mcp", "version": "1.0.0"},
                },
            }))
        })
        .expect(1)
        .mount(mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(header("authorization", bearer.as_str()))
        .and(body_partial_json(
            serde_json::json!({"method": "notifications/initialized"}),
        ))
        .respond_with(ResponseTemplate::new(202))
        .mount(mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mcp"))
        .respond_with(ResponseTemplate::new(405))
        .mount(mock_server)
        .await;
}

/// The gateway's reconnect path: after a restart, HttpTransport rebuilds the OAuth
/// client from storage, refreshes the expired token with the stored client secret,
/// and connects with the new token
#[tokio::test]
async fn http_transport_reconnects_with_stored_client_secret() {
    use mcpmux_gateway::pool::transport::{HttpTransport, Transport};
    use mcpmux_gateway::TransportConnectResult;

    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(header(
            "authorization",
            basic_auth(CLIENT_ID, CLIENT_SECRET),
        ))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(token_response("access-1"))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(header(
            "authorization",
            basic_auth(CLIENT_ID, CLIENT_SECRET),
        ))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(token_response("access-2"))
        .expect(1)
        .mount(&mock_server)
        .await;
    mount_mcp_endpoint(&mock_server, "access-2").await;

    let flow = Flow::new(&mock_server);
    assert!(flow.sign_in().await.success);

    let transport = HttpTransport::new(
        flow.server_url.clone(),
        std::collections::HashMap::new(),
        flow.space_id,
        SERVER_ID.to_string(),
        flow.credential_repo.clone(),
        flow.oauth_repo.clone(),
        None,
        Duration::from_secs(10),
        None,
    );

    match transport.connect().await {
        TransportConnectResult::Connected(client) => drop(client),
        TransportConnectResult::OAuthRequired { .. } => {
            panic!("refresh failed; McpMux asked to sign in again")
        }
        TransportConnectResult::Failed(e) => panic!("connect failed: {e}"),
    }
}
