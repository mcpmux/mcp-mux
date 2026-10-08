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
use mcpmux_core::{
    branding, CredentialRepository, CredentialType, OutboundOAuthRegistration,
    OutboundOAuthRepository,
};
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
    mount_dcr_metadata_with(mock_server, serde_json::json!({})).await;
}

/// `mount_dcr_metadata`, with `overrides` replacing fields of the authorization
/// server metadata
async fn mount_dcr_metadata_with(mock_server: &MockServer, overrides: serde_json::Value) {
    let base = mock_server.uri();
    let mut metadata = serde_json::json!({
        "issuer": base,
        "authorization_endpoint": format!("{}/authorize", base),
        "token_endpoint": format!("{}/token", base),
        "registration_endpoint": format!("{}/register", base),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic"],
        "scopes_supported": ["boards:read"],
    });
    for (key, value) in overrides.as_object().unwrap() {
        metadata[key] = value.clone();
    }

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
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
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

/// How the client sends its secret to the token endpoint (RFC 7591
/// `token_endpoint_auth_method`)
#[derive(Clone, Copy)]
enum ClientAuth {
    /// `client_secret_basic`: HTTP Basic
    Basic,
    /// `client_secret_post`: in the form body, with no Authorization header
    Post,
}

/// Matches a token request authenticated as CLIENT_ID / CLIENT_SECRET with exactly
/// the given method, as a server that enforces the registered method checks
struct Authenticates(ClientAuth);

impl wiremock::Match for Authenticates {
    fn matches(&self, request: &Request) -> bool {
        let authorization = request
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let body = String::from_utf8_lossy(&request.body);
        match self.0 {
            ClientAuth::Basic => {
                authorization == Some(basic_auth(CLIENT_ID, CLIENT_SECRET).as_str())
            }
            ClientAuth::Post => {
                authorization.is_none()
                    && body.contains(&format!("client_id={CLIENT_ID}"))
                    && body.contains(&format!("client_secret={CLIENT_SECRET}"))
            }
        }
    }
}

/// Mount a token endpoint that accepts only the confidential client authenticated
/// with `auth`, and rejects everything else with `invalid_client`
async fn mount_token_endpoint(mock_server: &MockServer, auth: ClientAuth) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(Authenticates(auth))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(token_response("access-1"))
        .mount(mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(Authenticates(auth))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(token_response("access-2"))
        .mount(mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(invalid_client())
        .with_priority(10)
        .mount(mock_server)
        .await;
}

fn invalid_client() -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "error": "invalid_client",
    }))
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
    credential_repo: Arc<dyn CredentialRepository>,
    oauth_repo: Arc<dyn OutboundOAuthRepository>,
    space_id: Uuid,
    server_url: String,
}

impl Flow {
    fn new(mock_server: &MockServer) -> Self {
        Self::with_oauth_repo(
            mock_server,
            Arc::new(MockOutboundOAuthRepository::new()),
            Uuid::new_v4(),
        )
    }

    fn with_oauth_repo(
        mock_server: &MockServer,
        oauth_repo: Arc<dyn OutboundOAuthRepository>,
        space_id: Uuid,
    ) -> Self {
        Self::with_repos(
            mock_server,
            Arc::new(MockCredentialRepository::new()),
            oauth_repo,
            space_id,
        )
    }

    fn with_repos(
        mock_server: &MockServer,
        credential_repo: Arc<dyn CredentialRepository>,
        oauth_repo: Arc<dyn OutboundOAuthRepository>,
        space_id: Uuid,
    ) -> Self {
        Self {
            manager: OutboundOAuthManager::new(),
            credential_repo,
            oauth_repo,
            space_id,
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
        self.complete(&auth_url, &mut events).await
    }

    /// Play the browser for a started flow: send the authorization code to the
    /// loopback redirect URI and wait for McpMux to finish the token exchange
    async fn complete(
        &self,
        auth_url: &url::Url,
        events: &mut tokio::sync::broadcast::Receiver<OAuthCompleteEvent>,
    ) -> OAuthCompleteEvent {
        let redirect_uri = query_param(auth_url, "redirect_uri").expect("redirect_uri param");
        let state = query_param(auth_url, "state").expect("state param");
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

    async fn registration(&self) -> Option<OutboundOAuthRegistration> {
        self.oauth_repo
            .get(&self.space_id, SERVER_ID)
            .await
            .unwrap()
    }

    /// Change the saved registration, as an older release or an expired secret
    /// would have left it
    async fn edit_registration(&self, edit: impl FnOnce(&mut OutboundOAuthRegistration)) {
        let mut registration = self.registration().await.expect("registration saved");
        edit(&mut registration);
        self.oauth_repo.save(&registration).await.unwrap();
    }

    /// Sign out: tokens gone, registration kept
    async fn sign_out(&self) {
        self.credential_repo
            .clear_tokens(&self.space_id, SERVER_ID)
            .await
            .unwrap();
    }

    async fn has_tokens(&self) -> bool {
        self.credential_repo
            .get(&self.space_id, SERVER_ID, &CredentialType::AccessToken)
            .await
            .unwrap()
            .is_some()
    }

    /// Get an access token the way the gateway does after a restart: a new manager
    /// with nothing in memory, only what was stored
    async fn access_token_after_restart(&self) -> anyhow::Result<String> {
        OutboundOAuthManager::new()
            .get_access_token(
                self.credential_repo.clone(),
                self.oauth_repo.clone(),
                self.space_id,
                SERVER_ID,
                &self.server_url,
            )
            .await
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

/// When the request without the client metadata is rejected too, the error keeps
/// both reasons: the first one may be the one that matters
#[tokio::test]
async fn registration_error_keeps_both_rejections() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .and(body_partial_json(serde_json::json!({
            "logo_uri": branding::outbound_oauth_logo_uri(),
        })))
        .respond_with(ResponseTemplate::new(400).set_body_string("logo_uri not allowed"))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .and(BodyLacksKeys(&["logo_uri"]))
        .respond_with(ResponseTemplate::new(400).set_body_string("redirect_uri not allowed"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let err = Flow::new(&mock_server).start().await.unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("Client registration failed")
            && message.contains("logo_uri not allowed")
            && message.contains("redirect_uri not allowed"),
        "unexpected error: {message}"
    );
}

/// Only a request rejected as invalid (400/422) is retried without the client
/// metadata. A protected (401/403) or rate-limited (429) registration endpoint
/// would refuse the smaller request too.
#[tokio::test]
async fn registration_refusal_is_not_retried() {
    for status in [401, 403, 429] {
        let mock_server = MockServer::start().await;
        mount_dcr_metadata(&mock_server).await;

        Mock::given(method("POST"))
            .and(path("/register"))
            .respond_with(ResponseTemplate::new(status).set_body_string("refused"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let err = Flow::new(&mock_server).start().await.unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("Client registration failed") && message.contains(&status.to_string()),
            "HTTP {status}: unexpected error: {message}"
        );
        assert!(
            !message.contains("retried"),
            "HTTP {status} was retried: {message}"
        );
    }
}

#[tokio::test]
async fn registration_unprocessable_entity_is_retried() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .and(body_partial_json(serde_json::json!({
            "logo_uri": branding::outbound_oauth_logo_uri(),
        })))
        .respond_with(ResponseTemplate::new(422))
        .expect(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .and(BodyLacksKeys(&["logo_uri"]))
        .respond_with(registration_response(None))
        .expect(1)
        .mount(&mock_server)
        .await;

    assert!(Flow::new(&mock_server).start().await.is_ok());
}

/// rmcp's own registration checked that the server supports the authorization code
/// flow before registering. Registering first would leave an unused client at the
/// server on every attempt.
#[tokio::test]
async fn server_without_code_flow_is_not_registered_with() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata_with(
        &mock_server,
        serde_json::json!({"response_types_supported": ["token"]}),
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(None))
        .expect(0)
        .mount(&mock_server)
        .await;

    let err = Flow::new(&mock_server).start().await.unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("Client registration failed") && message.contains("code"),
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
    let access_token = flow
        .access_token_after_restart()
        .await
        .expect("refresh with the stored client secret should succeed");
    assert_eq!(access_token, "access-2");
}

/// A server can advertise both `client_secret_basic` and `client_secret_post` but
/// register the client as `client_secret_post` and enforce it. rmcp would pick
/// Basic from the advertised list; McpMux must send the secret the way the client
/// was registered, for the code exchange and for the refresh after a restart.
#[tokio::test]
async fn client_secret_is_sent_with_the_registered_auth_method() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata_with(
        &mock_server,
        serde_json::json!({
            "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        }),
    )
    .await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "client_secret_expires_at": 0,
            "token_endpoint_auth_method": "client_secret_post",
            "redirect_uris": [],
        })))
        .expect(1)
        .mount(&mock_server)
        .await;
    mount_token_endpoint(&mock_server, ClientAuth::Post).await;

    let flow = Flow::new(&mock_server);
    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);

    let registration = flow.registration().await.expect("registration saved");
    assert_eq!(
        registration.token_endpoint_auth_method.as_deref(),
        Some("client_secret_post")
    );
    assert_eq!(registration.client_secret_expires_at, None);
    // What's stored is the server's own metadata, not the narrowed list
    assert_eq!(
        registration.metadata.unwrap().additional_fields["token_endpoint_auth_methods_supported"],
        serde_json::json!(["client_secret_basic", "client_secret_post"])
    );

    let access_token = flow
        .access_token_after_restart()
        .await
        .expect("refresh with client_secret_post should succeed");
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
    flow.sign_out().await;
    let event = flow.sign_in().await;
    assert!(event.success, "second sign-in failed: {:?}", event.error);
}

/// A failed code exchange that isn't `invalid_client` (a server error, an expired
/// or reused code) says nothing about the registration, so it's kept and the next
/// sign-in reuses it instead of registering another client
#[tokio::test]
async fn reused_registration_is_kept_when_the_exchange_fails_otherwise() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(1)
        .mount(&mock_server)
        .await;
    // Exchanges, in order: success, 503, invalid_grant, success
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_response("access"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(2)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant",
        })))
        .up_to_n_times(1)
        .with_priority(3)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(Authenticates(ClientAuth::Basic))
        .respond_with(token_response("access"))
        .with_priority(4)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    assert!(flow.sign_in().await.success);
    flow.sign_out().await;

    for failure in ["503", "invalid_grant"] {
        let event = flow.sign_in().await;
        assert!(!event.success, "exchange should fail with {failure}");
        let registration = flow
            .registration()
            .await
            .unwrap_or_else(|| panic!("registration dropped after {failure}"));
        assert_eq!(registration.client_id, CLIENT_ID);
        assert_eq!(registration.client_secret.as_deref(), Some(CLIENT_SECRET));
    }

    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);
}

/// Records every registration saved, i.e. every row a concurrent connection could read
#[derive(Default)]
struct RecordingOAuthRepo {
    inner: MockOutboundOAuthRepository,
    saved: std::sync::Mutex<Vec<OutboundOAuthRegistration>>,
}

#[async_trait::async_trait]
impl OutboundOAuthRepository for RecordingOAuthRepo {
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
    ) -> anyhow::Result<Option<OutboundOAuthRegistration>> {
        self.inner.get(space_id, server_id).await
    }

    async fn save(&self, registration: &OutboundOAuthRegistration) -> anyhow::Result<()> {
        self.saved.lock().unwrap().push(registration.clone());
        self.inner.save(registration).await
    }

    async fn delete(&self, space_id: &Uuid, server_id: &str) -> anyhow::Result<()> {
        self.inner.delete(space_id, server_id).await
    }

    async fn list_for_space(
        &self,
        space_id: &Uuid,
    ) -> anyhow::Result<Vec<OutboundOAuthRegistration>> {
        self.inner.list_for_space(space_id).await
    }
}

/// Whatever row a connection reads during or after the sign-in, the confidential
/// client is there with its secret and metadata. When the tokens arrive, rmcp's
/// credential store saves a bare registration for a client_id it doesn't know; if
/// the full one were saved only after the exchange, a connection in between would
/// refresh without the secret and fall back to "sign in required".
#[tokio::test]
async fn new_client_is_never_stored_without_its_secret() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(1)
        .mount(&mock_server)
        .await;
    mount_token_endpoint(&mock_server, ClientAuth::Basic).await;

    let repo = Arc::new(RecordingOAuthRepo::default());
    let flow = Flow::with_oauth_repo(&mock_server, repo.clone(), Uuid::new_v4());
    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);

    let saved = repo.saved.lock().unwrap().clone();
    assert!(!saved.is_empty());
    for registration in &saved {
        assert_eq!(registration.client_id, CLIENT_ID);
        assert_eq!(
            registration.client_secret.as_deref(),
            Some(CLIENT_SECRET),
            "a registration was stored without its secret: {registration:?}"
        );
        assert!(registration.metadata.is_some(), "stored without metadata");
    }
}

/// A new client is saved before its code exchange, so a sign-in that fails for
/// another reason than `invalid_client` leaves it for the next attempt to reuse,
/// instead of registering another client at the server
#[tokio::test]
async fn new_client_is_reused_after_a_failed_exchange() {
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
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;
    mount_token_endpoint(&mock_server, ClientAuth::Basic).await;

    let flow = Flow::new(&mock_server);
    let event = flow.sign_in().await;
    assert!(!event.success, "the exchange should fail with the 503");
    let registration = flow.registration().await.expect("new client kept");
    assert_eq!(registration.client_secret.as_deref(), Some(CLIENT_SECRET));

    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);
}

/// A registration whose secret expires is reused until it does; after that, the
/// next sign-in registers a new client instead of sending the expired secret
#[tokio::test]
async fn registration_with_an_expired_secret_is_not_reused() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;

    let expires_at = chrono::Utc::now().timestamp() + 86_400;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "client_secret_expires_at": expires_at,
            "redirect_uris": [],
        })))
        .expect(2)
        .mount(&mock_server)
        .await;
    mount_token_endpoint(&mock_server, ClientAuth::Basic).await;

    let flow = Flow::new(&mock_server);
    assert!(flow.sign_in().await.success);
    assert_eq!(
        flow.registration().await.unwrap().client_secret_expires_at,
        chrono::DateTime::from_timestamp(expires_at, 0)
    );

    // Not expired yet: reused (no second registration)
    flow.sign_out().await;
    assert!(flow.sign_in().await.success);

    // Expired: registered again
    flow.edit_registration(|reg| {
        reg.client_secret_expires_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));
    })
    .await;
    flow.sign_out().await;
    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);
    assert!(
        flow.registration().await.unwrap().client_secret_expires_at > Some(chrono::Utc::now()),
        "the new registration replaces the expired one"
    );
}

/// A confidential client saved before McpMux stored client secrets fails the code
/// exchange with `invalid_client` on every reuse. That registration and the tokens
/// issued to it are dropped, so the next sign-in registers a new client instead of
/// staying stuck. Such a row can't be told apart from a public client's, so this
/// takes two sign-ins once after upgrading.
#[tokio::test]
async fn reused_registration_without_its_secret_is_dropped_after_invalid_client() {
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
        .respond_with(invalid_client())
        .with_priority(10)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    assert!(flow.sign_in().await.success);

    // Simulate a registration saved by an older release: same client, no secret.
    // Its tokens stay: the refresh without the secret fails, which is why the
    // user signs in again.
    flow.edit_registration(|reg| reg.client_secret = None).await;
    assert!(flow.has_tokens().await);

    let event = flow.sign_in().await;
    assert!(!event.success, "exchange without the secret should fail");
    assert!(
        flow.registration().await.is_none(),
        "the unusable registration should be dropped"
    );
    assert!(
        !flow.has_tokens().await,
        "the tokens issued to the dropped client should go with it"
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

    let access_token = flow
        .access_token_after_restart()
        .await
        .expect("public client refresh should succeed");
    assert_eq!(access_token, "access-2");
}

/// After a master key reset, the client secret and the tokens no longer decrypt.
/// The next sign-in registers a fresh client on its first attempt and stores it with
/// its secret, instead of reusing the client without its secret and failing.
#[tokio::test]
async fn unreadable_registration_is_replaced_on_the_first_sign_in() {
    use mcpmux_core::SpaceRepository;
    use mcpmux_storage::{
        generate_master_key, FieldEncryptor, SqliteCredentialRepository,
        SqliteOutboundOAuthRepository, SqliteSpaceRepository,
    };
    use tokio::sync::Mutex;

    let encryptor = || Arc::new(FieldEncryptor::new(&generate_master_key().unwrap()).unwrap());
    let test_db = tests::db::TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let space = tests::fixtures::test_space("Work");
    SqliteSpaceRepository::new(db.clone())
        .create(&space)
        .await
        .unwrap();

    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(2)
        .mount(&mock_server)
        .await;
    mount_token_endpoint(&mock_server, ClientAuth::Basic).await;

    let flow_with_key = |key: Arc<FieldEncryptor>| {
        Flow::with_repos(
            &mock_server,
            Arc::new(SqliteCredentialRepository::new(db.clone(), key.clone())),
            Arc::new(SqliteOutboundOAuthRepository::new(db.clone(), key)),
            space.id,
        )
    };
    let flow = flow_with_key(encryptor());
    assert!(flow.sign_in().await.success);

    // A different master key: neither the stored secret nor the tokens decrypt
    let flow = flow_with_key(encryptor());
    assert!(flow.oauth_repo.get(&space.id, SERVER_ID).await.is_err());
    assert!(flow
        .credential_repo
        .get(&space.id, SERVER_ID, &CredentialType::AccessToken)
        .await
        .is_err());

    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);
    let registration = flow.registration().await.expect("readable again");
    assert_eq!(registration.client_secret.as_deref(), Some(CLIENT_SECRET));
    assert!(registration.metadata.is_some());

    let access_token = flow
        .access_token_after_restart()
        .await
        .expect("refresh with the new client's secret should succeed");
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
/// sent the way the client was registered, and connects with the new token.
/// `edit` changes the saved registration before the reconnect.
async fn assert_http_transport_reconnects(
    auth: ClientAuth,
    edit: impl FnOnce(&mut OutboundOAuthRegistration),
) {
    use mcpmux_gateway::pool::transport::{HttpTransport, Transport};
    use mcpmux_gateway::TransportConnectResult;

    let (auth_methods, registered_method) = match auth {
        ClientAuth::Basic => (serde_json::json!(["client_secret_basic"]), None),
        ClientAuth::Post => (
            serde_json::json!(["client_secret_basic", "client_secret_post"]),
            Some("client_secret_post"),
        ),
    };

    let mock_server = MockServer::start().await;
    mount_dcr_metadata_with(
        &mock_server,
        serde_json::json!({"token_endpoint_auth_methods_supported": auth_methods}),
    )
    .await;

    let mut registration = serde_json::json!({
        "client_id": CLIENT_ID,
        "client_secret": CLIENT_SECRET,
        "redirect_uris": [],
    });
    if let Some(method) = registered_method {
        registration["token_endpoint_auth_method"] = serde_json::json!(method);
    }
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(registration))
        .mount(&mock_server)
        .await;
    mount_token_endpoint(&mock_server, auth).await;
    mount_mcp_endpoint(&mock_server, "access-2").await;

    let flow = Flow::new(&mock_server);
    let event = flow.sign_in().await;
    assert!(event.success, "sign-in failed: {:?}", event.error);
    flow.edit_registration(edit).await;

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

#[tokio::test]
async fn http_transport_reconnects_with_stored_client_secret() {
    assert_http_transport_reconnects(ClientAuth::Basic, |_| {}).await;
}

#[tokio::test]
async fn http_transport_reconnects_with_client_secret_post() {
    assert_http_transport_reconnects(ClientAuth::Post, |_| {}).await;
}

/// The redirect URI isn't used by a refresh, so a stored one that isn't a URL
/// mustn't stop the reconnect
#[tokio::test]
async fn http_transport_reconnects_despite_an_unusable_redirect_uri() {
    assert_http_transport_reconnects(ClientAuth::Basic, |reg| {
        reg.redirect_uri = Some("not a url".to_string());
    })
    .await;
}

/// A sign-in that fails with invalid_client drops only the client it used. If
/// another sign-in registered a different client in the meantime, that one stays.
#[tokio::test]
async fn invalid_client_keeps_a_client_registered_meanwhile() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(invalid_client())
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    let mut events = flow.manager.subscribe();
    let auth_url = flow.start().await.expect("start_oauth_flow should succeed");
    flow.edit_registration(|registration| {
        registration.client_id = "client-from-a-newer-sign-in".to_string();
    })
    .await;

    let event = flow.complete(&auth_url, &mut events).await;
    assert!(!event.success);
    let registration = flow.registration().await.expect("newer client kept");
    assert_eq!(registration.client_id, "client-from-a-newer-sign-in");
}

/// Only the error code counts: an invalid_grant whose description or error_uri
/// mentions invalid_client says nothing about the client
#[tokio::test]
async fn invalid_client_in_an_error_description_keeps_the_client() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant",
            "error_description": "code was issued to invalid_client 42",
            "error_uri": "https://auth.example/errors/invalid_client",
        })))
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    let event = flow.sign_in().await;
    assert!(!event.success);
    let registration = flow.registration().await.expect("registration kept");
    assert_eq!(registration.client_id, CLIENT_ID);
    assert_eq!(registration.client_secret.as_deref(), Some(CLIENT_SECRET));
}

/// A registration store whose saves fail, e.g. a locked database
#[derive(Default)]
struct UnwritableOAuthRepo(MockOutboundOAuthRepository);

#[async_trait::async_trait]
impl OutboundOAuthRepository for UnwritableOAuthRepo {
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
    ) -> anyhow::Result<Option<OutboundOAuthRegistration>> {
        self.0.get(space_id, server_id).await
    }

    async fn save(&self, _registration: &OutboundOAuthRegistration) -> anyhow::Result<()> {
        anyhow::bail!("database is locked")
    }

    async fn delete(&self, space_id: &Uuid, server_id: &str) -> anyhow::Result<()> {
        self.0.delete(space_id, server_id).await
    }

    async fn list_for_space(
        &self,
        space_id: &Uuid,
    ) -> anyhow::Result<Vec<OutboundOAuthRegistration>> {
        self.0.list_for_space(space_id).await
    }
}

/// A new confidential client that can't be saved fails the sign-in: otherwise the
/// tokens would be stored with a bare registration, and every refresh would go out
/// without the secret
#[tokio::test]
async fn new_confidential_client_that_cannot_be_saved_fails_the_sign_in() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .mount(&mock_server)
        .await;

    let flow = Flow::with_oauth_repo(
        &mock_server,
        Arc::new(UnwritableOAuthRepo::default()),
        Uuid::new_v4(),
    );
    let error = flow.start().await.expect_err("sign-in should fail");
    assert!(
        error
            .to_string()
            .contains("Failed to save the new client registration"),
        "{error}"
    );
}

/// A public client has nothing to lose but its metadata, so the sign-in goes on
#[tokio::test]
async fn new_public_client_is_used_even_if_it_cannot_be_saved() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(registration_response(None))
        .mount(&mock_server)
        .await;

    let flow = Flow::with_oauth_repo(
        &mock_server,
        Arc::new(UnwritableOAuthRepo::default()),
        Uuid::new_v4(),
    );
    flow.start().await.expect("sign-in should start");
}

/// The registration request goes only to the registration endpoint the server
/// advertised; a redirect elsewhere is not followed
#[tokio::test]
async fn registration_redirect_is_not_followed() {
    let mock_server = MockServer::start().await;
    mount_dcr_metadata(&mock_server).await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("Location", format!("{}/elsewhere", mock_server.uri())),
        )
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/elsewhere"))
        .respond_with(registration_response(Some(CLIENT_SECRET)))
        .expect(0)
        .mount(&mock_server)
        .await;

    let flow = Flow::new(&mock_server);
    let error = flow.start().await.expect_err("registration should fail");
    assert!(error.to_string().contains("307"), "{error}");
    assert!(flow.registration().await.is_none());
}
