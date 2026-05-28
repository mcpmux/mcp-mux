//! Network-bind advertising: when the gateway is bound to a non-loopback
//! address (`network_bind = true`) the OAuth/MCP discovery metadata must
//! advertise the host the client actually reached it on (the request `Host`
//! header) so a remote client gets a resolvable URL instead of `localhost`.
//! On a loopback bind the static base URL is used regardless of `Host`.
//!
//! Drives the real `oauth_metadata` / `resource_metadata` handlers over HTTP.

use axum::{routing::get, Router};
use mcpmux_core::{DomainEvent, ServerDiscoveryService, ServerLogManager};
use mcpmux_gateway::server::{
    oauth_metadata, resource_metadata, AppState, DependenciesBuilder, GatewayDependencies,
    GatewayState, ServiceContainer,
};
use mcpmux_storage::SqliteSpaceRepository;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

struct Harness {
    base: String,
    ct: CancellationToken,
}

impl Harness {
    /// Boot a gateway serving only the OAuth-discovery endpoints, with inbound
    /// auth enabled (so metadata is advertised) and the given `network_bind`.
    /// `public_base_url` stays unset so the Host-derivation path is exercised.
    async fn start(network_bind: bool) -> Self {
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
        gw_state.set_network_bind(network_bind);
        // public_base_url stays None; auth stays enabled so metadata is served.
        let gateway_state = Arc::new(tokio::sync::RwLock::new(gw_state));

        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state,
            None,
        ));

        let app_state = AppState {
            gateway_state: services.gateway_state.clone(),
            services: services.clone(),
            base_url: "http://127.0.0.1:0".to_string(),
        };
        let router = Router::new()
            .route(
                "/.well-known/oauth-authorization-server",
                get(oauth_metadata),
            )
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(resource_metadata),
            )
            .with_state(app_state);

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
            base: format!("http://127.0.0.1:{port}"),
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
async fn network_bind_advertises_the_request_host() {
    let h = Harness::start(true).await;
    let client = reqwest::Client::new();
    // The address a remote client reached us on (different from the loopback
    // socket we actually bound). The advertised metadata must reflect this.
    let lan = "mcpmux.lan:8080";

    let meta: serde_json::Value = client
        .get(format!("{}/.well-known/oauth-authorization-server", h.base))
        .header(reqwest::header::HOST, lan)
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(meta["issuer"], format!("http://{lan}"));
    assert_eq!(
        meta["authorization_endpoint"],
        format!("http://{lan}/oauth/authorize")
    );
    assert_eq!(meta["token_endpoint"], format!("http://{lan}/oauth/token"));

    let res: serde_json::Value = client
        .get(format!(
            "{}/.well-known/oauth-protected-resource/mcp",
            h.base
        ))
        .header(reqwest::header::HOST, lan)
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(res["resource"], format!("http://{lan}/mcp"));
    assert_eq!(res["authorization_servers"][0], format!("http://{lan}"));
}

#[tokio::test]
async fn loopback_bind_ignores_request_host() {
    let h = Harness::start(false).await;
    let meta: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/.well-known/oauth-authorization-server", h.base))
        .header(reqwest::header::HOST, "mcpmux.lan:8080")
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    // network_bind = false → the configured base URL is advertised, Host ignored.
    assert_eq!(meta["issuer"], "http://127.0.0.1:0");
}
