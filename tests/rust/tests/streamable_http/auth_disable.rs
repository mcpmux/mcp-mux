//! End-to-end proof that the gateway is *truly* authless when the
//! `gateway.auth_disabled` toggle is on.
//!
//! Unlike `gateway_notifications.rs` (which bypasses auth with a test
//! middleware), this drives the **real** `mcp_oauth_middleware` over HTTP and
//! sends requests with **no** `Authorization` header:
//!   - auth disabled → the request is accepted and an anonymous client identity
//!     is injected (200, not 401),
//!   - auth required (default) → the same tokenless request is rejected (401).

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
use mcpmux_storage::SqliteSpaceRepository;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

/// Minimal `/mcp` handler that echoes the gateway-injected client id so the
/// test can confirm the middleware ran and assigned an identity.
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
    ct: CancellationToken,
}

impl Harness {
    /// Boot a gateway exposing `/mcp` behind the REAL oauth middleware, with the
    /// inbound-auth toggle set to `auth_disabled`.
    async fn start(auth_disabled: bool) -> Self {
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
        // No JWT secret needed: these tests send no token, so the auth-required
        // path 401s before the secret is ever consulted.
        gw_state.set_auth_disabled(auth_disabled);
        let gateway_state = Arc::new(tokio::sync::RwLock::new(gw_state));

        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state,
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
            ct,
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.ct.cancel();
    }
}

#[tokio::test]
async fn authless_gateway_accepts_request_without_token() {
    let h = Harness::start(true).await;
    let resp = reqwest::Client::new()
        .post(&h.url)
        .header("content-type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .send()
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "auth-disabled gateway must accept a tokenless request"
    );
    // The middleware injected an anonymous identity rather than rejecting.
    let body = resp.text().await.unwrap();
    assert_eq!(body, "mcpmux-anonymous");
}

#[tokio::test]
async fn auth_required_gateway_rejects_request_without_token() {
    let h = Harness::start(false).await;
    let resp = reqwest::Client::new()
        .post(&h.url)
        .header("content-type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .send()
        .await
        .expect("request");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "default gateway must reject a tokenless request"
    );
}
