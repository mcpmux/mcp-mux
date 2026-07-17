//! End-to-end proof that the gateway authenticates a request bearing a
//! host-issued **API key** through the REAL `mcp_oauth_middleware`:
//!   - a live key in `Authorization: Bearer mcpk_…` is accepted (200) and the
//!     middleware injects the owning client's id,
//!   - an unknown key is rejected (401),
//!   - a revoked key is rejected (401).
//!
//! This is the headless/remote auth path that needs no interactive consent —
//! the secure way to connect when the gateway is exposed over the network.

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
    server::{DependenciesBuilder, GatewayDependencies, GatewayState, ServiceContainer},
};
use mcpmux_storage::{
    InboundClient, InboundClientRepository, RegistrationType, SqliteSpaceRepository,
};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

/// Minimal `/mcp` handler that echoes the gateway-injected client id so the
/// test can confirm the middleware authenticated and assigned the right identity.
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
    client_repo: Arc<InboundClientRepository>,
    client_id: String,
    api_key: String,
    key_id: String,
    ct: CancellationToken,
}

impl Harness {
    /// Boot a gateway exposing `/mcp` behind the REAL oauth middleware with auth
    /// REQUIRED, and pre-register a Preregistered client + one API key over the
    /// same database the middleware validates against.
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

        let client_repo = Arc::new(InboundClientRepository::new(database.clone()));
        let client_id = format!("mcp_{}", &Uuid::new_v4().simple().to_string()[..8]);
        let now = chrono::Utc::now().to_rfc3339();
        let client = InboundClient {
            client_id: client_id.clone(),
            registration_type: RegistrationType::Preregistered,
            client_name: "headless-bot".to_string(),
            client_alias: None,
            redirect_uris: vec![],
            grant_types: vec![],
            response_types: vec![],
            token_endpoint_auth_method: "none".to_string(),
            scope: None,
            approved: true,
            logo_uri: None,
            client_uri: None,
            software_id: None,
            software_version: None,
            metadata_url: None,
            metadata_cached_at: None,
            metadata_cache_ttl: None,
            last_seen: None,
            created_at: now.clone(),
            updated_at: now,
            reports_roots: false,
            roots_capability_known: false,
            machine_id: None,
        };
        client_repo.save_client(&client).await.expect("save client");
        let key_id = Uuid::new_v4().to_string();
        let api_key = format!("mcpk_{}", Uuid::new_v4().simple());
        let prefix: String = api_key.chars().take(13).collect();
        client_repo
            .create_api_key(&key_id, &client_id, &api_key, &prefix, None, None)
            .await
            .expect("create api key");

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
        gw_state.set_auth_disabled(false);
        let gateway_state = Arc::new(tokio::sync::RwLock::new(gw_state));

        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state,
            None,
        ));

        let router = Router::new().route("/mcp", post(echo_client_id)).layer(
            middleware::from_fn_with_state(services.clone(), mcp_oauth_middleware),
        );

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
            url: format!("http://127.0.0.1:{port}/mcp"),
            client_repo,
            client_id,
            api_key,
            key_id,
            ct,
        }
    }

    async fn post_with_bearer(&self, token: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(&self.url)
            .header("content-type", "application/json")
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .send()
            .await
            .expect("request")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.ct.cancel();
    }
}

#[tokio::test]
async fn api_key_authenticates_and_injects_client_id() {
    let h = Harness::start().await;
    let resp = h.post_with_bearer(&h.api_key).await;
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "a live API key must authenticate against an auth-required gateway"
    );
    let body = resp.text().await.unwrap();
    assert_eq!(
        body, h.client_id,
        "the middleware injects the key's owning client id"
    );
}

#[tokio::test]
async fn unknown_api_key_is_rejected() {
    let h = Harness::start().await;
    let resp = h.post_with_bearer("mcpk_not_a_real_key").await;
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "an unknown key must be rejected"
    );
}

#[tokio::test]
async fn revoked_api_key_is_rejected() {
    let h = Harness::start().await;
    assert_eq!(
        h.post_with_bearer(&h.api_key).await.status(),
        reqwest::StatusCode::OK
    );
    h.client_repo
        .revoke_api_key(&h.key_id)
        .await
        .expect("revoke");
    let resp = h.post_with_bearer(&h.api_key).await;
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "a revoked key must be rejected"
    );
}
