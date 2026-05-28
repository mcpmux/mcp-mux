//! Inbound OAuth end-to-end test — proves the AUTH-ENABLED flow is intact.
//!
//! Drives the real production handlers (no stubs) over HTTP against a gateway
//! whose inbound auth is REQUIRED:
//!
//!   DCR register → authorize (consent page) → consent approve → token (PKCE
//!   S256) → authenticated `/mcp` handshake (200), and a tokenless `/mcp` →
//!   401.
//!
//! This guards the guarantee that toggling/adding the "disable auth" feature
//! did not regress real OAuth: a client that completes the flow gets a token
//! that the `/mcp` middleware accepts, and a client without one is rejected.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use base64::Engine;
use mcpmux_core::{DomainEvent, ServerDiscoveryService, ServerLogManager, SpaceRepository};
use mcpmux_gateway::{
    consumers::MCPNotifier,
    mcp::{mcp_oauth_middleware, McpMuxGatewayHandler},
    server::{
        oauth_authorize, oauth_consent_approve, oauth_register, oauth_token, DependenciesBuilder,
        GatewayDependencies, GatewayState, ServiceContainer,
    },
};
use mcpmux_storage::{InboundClientRepository, SqliteSpaceRepository};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use sha2::Digest;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

/// A loopback redirect URI (RFC 8252). Uses a real (unused) port so a stray
/// redirect to it is a clean connect error rather than the invalid `:0`.
const REDIRECT: &str = "http://127.0.0.1:8765/callback";
/// A fixed PKCE verifier (43–128 chars per RFC 7636).
const CODE_VERIFIER: &str = "e2e_pkce_code_verifier_0123456789_abcdefghijklmno";

struct Harness {
    base: String,
    ct: CancellationToken,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.ct.cancel();
    }
}

impl Harness {
    /// Boot a real gateway with inbound auth REQUIRED (no `auth_disabled`), the
    /// JWT secret configured, and the public OAuth flow routes mounted next to
    /// the `/mcp` service guarded by the real `mcp_oauth_middleware`.
    async fn start() -> Self {
        let ct = CancellationToken::new();

        let test_db = TestDatabase::in_memory();
        let database = Arc::new(tokio::sync::Mutex::new(test_db.db));

        let feature_repo = Arc::new(MockServerFeatureRepository::new());
        let feature_set_repo = Arc::new(MockFeatureSetRepository::new());

        // Seed a default space so an authenticated client can resolve one.
        let space_repo = Arc::new(SqliteSpaceRepository::new(database.clone()));
        let space_id = Uuid::new_v4();
        let space = mcpmux_core::domain::Space {
            id: space_id,
            name: "Test Space".to_string(),
            icon: Some("test".to_string()),
            description: None,
            is_default: true,
            sort_order: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        SpaceRepository::create(&*space_repo, &space)
            .await
            .expect("create space");
        SpaceRepository::set_default(&*space_repo, &space_id)
            .await
            .expect("set default");

        let inbound_client_repo = Arc::new(InboundClientRepository::new(database.clone()));

        let deps = DependenciesBuilder::new()
            .with_installed_server_repo(Arc::new(MockInstalledServerRepository::new()))
            .with_credential_repo(Arc::new(MockCredentialRepository::new()))
            .with_backend_oauth_repo(Arc::new(MockOutboundOAuthRepository::new()))
            .with_feature_repo(feature_repo as Arc<dyn mcpmux_core::ServerFeatureRepository>)
            .with_feature_set_repo(feature_set_repo as Arc<dyn mcpmux_core::FeatureSetRepository>)
            .with_server_discovery(Arc::new(ServerDiscoveryService::new(
                std::path::PathBuf::from("test-data"),
                std::path::PathBuf::from("test-spaces"),
            )))
            .with_log_manager(Arc::new(ServerLogManager::new(
                mcpmux_core::LogConfig::default(),
            )))
            .with_database(database.clone())
            .build()
            .expect("build dependencies");
        let deps = GatewayDependencies {
            space_repo: space_repo as Arc<dyn mcpmux_core::SpaceRepository>,
            inbound_client_repo,
            ..deps
        };

        let (event_tx, _) = broadcast::channel::<DomainEvent>(256);

        // Auth is REQUIRED here (we never call set_auth_disabled). The JWT secret
        // is what the /oauth/token endpoint signs with and the /mcp middleware
        // validates against — they must be the same instance, so set it once.
        let mut gw_state = GatewayState::new(event_tx.clone());
        gw_state.set_base_url("http://127.0.0.1".to_string());
        gw_state.set_database(database.clone());
        gw_state.set_client_metadata_service(deps.client_metadata_service.clone());
        gw_state.set_jwt_secret(zeroize::Zeroizing::new(
            [7u8; mcpmux_storage::JWT_SECRET_SIZE],
        ));
        let gateway_state = Arc::new(tokio::sync::RwLock::new(gw_state));

        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state,
            None,
        ));

        let notifier = Arc::new(MCPNotifier::new(
            services.feature_set_resolver.clone(),
            services.pool_services.feature_service.clone(),
        ));
        notifier.clone().start(event_tx.subscribe());
        let handler = McpMuxGatewayHandler::new(services.clone(), notifier.clone());

        let mut http_cfg = StreamableHttpServerConfig::default();
        http_cfg.stateful_mode = true;
        http_cfg.json_response = false;
        http_cfg.sse_keep_alive = Some(Duration::from_secs(15));
        http_cfg.cancellation_token = ct.child_token();
        let mcp_service = StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            http_cfg,
        );

        let mcp_routes =
            Router::new()
                .nest_service("/mcp", mcp_service)
                .layer(middleware::from_fn_with_state(
                    services.clone(),
                    mcp_oauth_middleware,
                ));
        // Public OAuth flow routes, mounted exactly as production does. The
        // consent-approve endpoint is the same handler production gates behind
        // MCPMUX_E2E_TEST; mounting it directly keeps the test self-contained.
        let oauth_routes = Router::new()
            .route("/oauth/register", post(oauth_register))
            .route("/oauth/authorize", get(oauth_authorize))
            .route("/oauth/token", post(oauth_token))
            .route("/oauth/consent/approve", post(oauth_consent_approve))
            .with_state(services.gateway_state.clone());
        let router = mcp_routes.merge(oauth_routes);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        let ct_clone = ct.clone();
        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { ct_clone.cancelled().await })
                .await
                .unwrap();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            base: format!("http://127.0.0.1:{port}"),
            ct,
        }
    }
}

/// S256 PKCE challenge for [`CODE_VERIFIER`].
fn code_challenge() -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(CODE_VERIFIER.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Slice out the value following `start`, up to any of `ends` (or end of string).
fn between(s: &str, start: &str, ends: &[char]) -> Option<String> {
    let i = s.find(start)? + start.len();
    let rest = &s[i..];
    let j = rest.find(|c| ends.contains(&c)).unwrap_or(rest.len());
    Some(rest[..j].to_string())
}

/// Pull a single query-param value out of a URL.
fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split('?').nth(1)?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next()? == key {
            return Some(it.next().unwrap_or("").to_string());
        }
    }
    None
}

const INIT_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"e2e","version":"1.0"}}}"#;

#[tokio::test(flavor = "multi_thread")]
async fn auth_enabled_full_oauth_flow_then_authenticated_mcp() {
    let h = Harness::start().await;
    // Don't auto-follow redirects: the OAuth steps return their own responses
    // (consent HTML, JSON), and a stray follow to the client redirect_uri would
    // mask the real status.
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");

    // 1. Dynamic client registration.
    let reg: serde_json::Value = http
        .post(format!("{}/oauth/register", h.base))
        .json(&serde_json::json!({
            "client_name": "e2e",
            "redirect_uris": [REDIRECT],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await
        .expect("register request")
        .json()
        .await
        .expect("register json");
    let client_id = reg["client_id"].as_str().expect("client_id").to_string();

    // 2. Authorization request → branded consent page carrying the request_id.
    // Build the query manually (redirect_uri needs percent-encoding; the S256
    // challenge is already URL-safe base64).
    let challenge = code_challenge();
    let redirect_enc: String = url::form_urlencoded::byte_serialize(REDIRECT.as_bytes()).collect();
    let authorize_url = format!(
        "{}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope=mcp&state=st-123&code_challenge={}&code_challenge_method=S256",
        h.base, client_id, redirect_enc, challenge,
    );
    let authorize = http
        .get(&authorize_url)
        .send()
        .await
        .expect("authorize request");
    let status = authorize.status();
    let location = authorize
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let html = authorize.text().await.unwrap();
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "authorize should render the consent page; got {status}, location={location:?}, body={}",
        &html.chars().take(300).collect::<String>()
    );
    let request_id =
        between(&html, "request_id=", &['"', '&', ' ', '\'']).expect("request_id in consent HTML");

    // 3. Approve consent → redirect URL with the authorization code.
    let approve: serde_json::Value = http
        .post(format!("{}/oauth/consent/approve", h.base))
        .json(&serde_json::json!({ "request_id": request_id, "approved": true }))
        .send()
        .await
        .expect("approve request")
        .json()
        .await
        .expect("approve json");
    let redirect_url = approve["redirect_url"].as_str().expect("redirect_url");
    let code = query_param(redirect_url, "code").expect("authorization code");

    // 4. Token exchange with the PKCE verifier.
    let token: serde_json::Value = http
        .post(format!("{}/oauth/token", h.base))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT),
            ("client_id", client_id.as_str()),
            ("code_verifier", CODE_VERIFIER),
        ])
        .send()
        .await
        .expect("token request")
        .json()
        .await
        .expect("token json");
    let access_token = token["access_token"]
        .as_str()
        .expect("access_token in token response")
        .to_string();

    // 5. Authenticated MCP handshake — the minted token must be accepted.
    let authed = http
        .post(format!("{}/mcp", h.base))
        .header("authorization", format!("Bearer {access_token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(INIT_BODY)
        .send()
        .await
        .expect("authed mcp request");
    assert_eq!(
        authed.status(),
        reqwest::StatusCode::OK,
        "a valid OAuth token must authenticate the MCP handshake"
    );

    // 6. The same handshake without a token is rejected (auth IS required).
    let denied = http
        .post(format!("{}/mcp", h.base))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(INIT_BODY)
        .send()
        .await
        .expect("tokenless mcp request");
    assert_eq!(
        denied.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "a tokenless request must 401 when auth is enabled"
    );
}
