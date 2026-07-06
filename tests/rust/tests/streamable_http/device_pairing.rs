//! End-to-end device pairing: mint a token (as the desktop does), claim it over
//! HTTP at `/pair/claim` (as a new device does), and prove the returned API key
//! then authenticates a `/mcp` request through the REAL oauth middleware.
//!
//! Also covers the security invariants: an invalid/expired/reused token is
//! rejected, and the token is single-use.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use mcpmux_core::{DomainEvent, ServerDiscoveryService, ServerLogManager};
use mcpmux_gateway::{
    mcp::mcp_oauth_middleware,
    server::{
        pair_claim, AppState, DependenciesBuilder, GatewayDependencies, GatewayState,
        ServiceContainer,
    },
};
use mcpmux_storage::SqliteSpaceRepository;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

async fn echo_client_id(req: Request<Body>) -> Response {
    let cid = req
        .headers()
        .get("x-mcpmux-client-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    (StatusCode::OK, cid).into_response()
}

struct Harness {
    url: String,
    gateway_state: Arc<RwLock<GatewayState>>,
    ct: CancellationToken,
}

impl Harness {
    /// Boot a gateway exposing the pairing claim endpoint AND an auth-required
    /// `/mcp`, over the same database + state, so a claimed key can be checked
    /// against the real middleware.
    async fn start() -> Self {
        let ct = CancellationToken::new();
        let space_id = Uuid::new_v4();

        let test_db = TestDatabase::in_memory();
        let database = Arc::new(tokio::sync::Mutex::new(test_db.db));

        let space_repo = Arc::new(SqliteSpaceRepository::new(database.clone()));
        let space = mcpmux_core::domain::Space {
            id: space_id,
            name: "Test Space".to_string(),
            icon: None,
            description: None,
            is_default: true,
            sort_order: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        mcpmux_core::SpaceRepository::create(&*space_repo, &space)
            .await
            .expect("create space");
        mcpmux_core::SpaceRepository::set_default(&*space_repo, &space_id)
            .await
            .expect("set default");

        let deps = DependenciesBuilder::new()
            .with_installed_server_repo(Arc::new(MockInstalledServerRepository::new()))
            .with_credential_repo(Arc::new(MockCredentialRepository::new()))
            .with_backend_oauth_repo(Arc::new(MockOutboundOAuthRepository::new()))
            .with_feature_repo(Arc::new(MockServerFeatureRepository::new())
                as Arc<dyn mcpmux_core::ServerFeatureRepository>)
            .with_feature_set_repo(Arc::new(MockFeatureSetRepository::new())
                as Arc<dyn mcpmux_core::FeatureSetRepository>)
            .with_server_discovery(Arc::new(ServerDiscoveryService::new(
                std::path::PathBuf::from("test-data"),
                std::path::PathBuf::from("test-spaces"),
            )))
            .with_log_manager(Arc::new(ServerLogManager::new(
                mcpmux_core::LogConfig::default(),
            )))
            .with_database(database)
            .build()
            .expect("build dependencies");
        let deps = GatewayDependencies {
            space_repo: space_repo as Arc<dyn mcpmux_core::SpaceRepository>,
            ..deps
        };

        let (event_tx, _) = broadcast::channel::<DomainEvent>(64);
        let mut gw_state = GatewayState::new(event_tx.clone());
        gw_state.set_base_url("http://127.0.0.1:0".to_string());
        gw_state
            .set_auth_disabled(false)
            .expect("enabling auth is always allowed");
        let gateway_state = Arc::new(RwLock::new(gw_state));

        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state.clone(),
        ));

        let app_state = AppState {
            gateway_state: gateway_state.clone(),
            services: services.clone(),
            base_url: "http://127.0.0.1:0".to_string(),
        };

        let router = Router::new()
            .route("/pair/claim", post(pair_claim))
            .with_state(app_state)
            .merge(Router::new().route("/mcp", post(echo_client_id)).layer(
                middleware::from_fn_with_state(services.clone(), mcp_oauth_middleware),
            ));

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
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        Self {
            url: format!("http://127.0.0.1:{port}"),
            gateway_state,
            ct,
        }
    }

    /// Mint a token the way the desktop `mint_pairing_token` command does.
    async fn mint(&self) -> String {
        self.gateway_state
            .read()
            .await
            .pairing_tokens()
            .mint(std::time::Duration::from_secs(300))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.ct.cancel();
    }
}

#[tokio::test]
async fn claim_issues_a_working_api_key() {
    let h = Harness::start().await;
    let token = h.mint().await;
    let client = reqwest::Client::new();

    // Claim the token → get an API key for this "device".
    let resp = client
        .post(format!("{}/pair/claim", h.url))
        .json(&serde_json::json!({ "token": token, "device_name": "My phone" }))
        .send()
        .await
        .expect("claim");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let api_key = body["api_key"].as_str().expect("api_key in response");
    assert!(api_key.starts_with("mcpk_"));
    assert_eq!(body["client_name"], "My phone");
    assert!(body["endpoint"].as_str().unwrap().ends_with("/mcp"));

    // The issued key authenticates a real /mcp request.
    let resp = client
        .post(format!("{}/mcp", h.url))
        .header("authorization", format!("Bearer {api_key}"))
        .header("content-type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .send()
        .await
        .expect("mcp");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "the paired key must authenticate /mcp"
    );
    let injected_client_id = resp.text().await.unwrap();
    assert!(
        injected_client_id.starts_with("mcp_"),
        "middleware injected the paired client's id: {injected_client_id:?}"
    );
}

#[tokio::test]
async fn token_is_single_use() {
    let h = Harness::start().await;
    let token = h.mint().await;
    let client = reqwest::Client::new();

    let first = client
        .post(format!("{}/pair/claim", h.url))
        .json(&serde_json::json!({ "token": token }))
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), reqwest::StatusCode::OK);

    // Reusing the same token is rejected.
    let second = client
        .post(format!("{}/pair/claim", h.url))
        .json(&serde_json::json!({ "token": token }))
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalid_token_rejected() {
    let h = Harness::start().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/pair/claim", h.url))
        .json(&serde_json::json!({ "token": "mcppair_bogus" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}
