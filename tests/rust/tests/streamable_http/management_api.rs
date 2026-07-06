//! Integration coverage for the authenticated management API: bearer-token
//! gating, the typed `/admin/api/*` endpoints, the command-mirror RPC
//! (`/admin/api/rpc/<command>` — exact Tauri shapes, 501 desktop-only, 404
//! unknown), and the SSE event stream. Drives the real `management_router`
//! over HTTP against a live gateway state.

use axum::Router;
use futures::StreamExt;
use mcpmux_core::{DomainEvent, ServerDiscoveryService, ServerLogManager};
use mcpmux_gateway::server::{
    management::management_router, DependenciesBuilder, GatewayDependencies, GatewayState,
    ServiceContainer,
};
use mcpmux_storage::{SqliteFeatureSetRepository, SqliteSpaceRepository};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tests::db::TestDatabase;
use tests::mocks::*;

const TOKEN: &str = "test-admin-token";

struct Harness {
    url: String,
    ct: CancellationToken,
}

impl Harness {
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
            // Real (SQLite) feature-set repo so create_feature_set persists and
            // a binding referencing it doesn't dangle.
            .with_feature_set_repo(Arc::new(SqliteFeatureSetRepository::new(database.clone()))
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
        let gateway_state = Arc::new(RwLock::new(gw_state));
        let services = Arc::new(ServiceContainer::initialize(
            &deps,
            event_tx.clone(),
            gateway_state.clone(),
        ));
        let app_state = mcpmux_gateway::server::AppState {
            gateway_state,
            services,
            base_url: "http://127.0.0.1:0".to_string(),
        };

        let router: Router = management_router(app_state, Arc::new(TOKEN.to_string()));

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
            ct,
        }
    }
    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("authorization", format!("Bearer {TOKEN}"))
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.ct.cancel();
    }
}

#[tokio::test]
async fn typed_endpoints_require_token_and_serve_data() {
    let h = Harness::start().await;
    let c = reqwest::Client::new();

    // 401 without a token.
    let r = c
        .get(format!("{}/admin/api/info", h.url))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::UNAUTHORIZED);

    // 200 + posture with a token.
    let info: serde_json::Value = h
        .auth(c.get(format!("{}/admin/api/info", h.url)))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(info["version"].is_string());
    assert_eq!(info["auth_required"], true);

    // Spaces list includes the Space we created.
    let spaces: serde_json::Value = h
        .auth(c.get(format!("{}/admin/api/spaces", h.url)))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(spaces["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "Test Space"));
}

#[tokio::test]
async fn rpc_mirror_shapes_and_error_codes() {
    let h = Harness::start().await;
    let c = reqwest::Client::new();
    let rpc = |cmd: &str| format!("{}/admin/api/rpc/{cmd}", h.url);

    // list_spaces returns a RAW Space[] (Tauri shape), not wrapped.
    let spaces: serde_json::Value = h
        .auth(c.post(rpc("list_spaces")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(spaces.is_array(), "list_spaces must be a raw array");
    assert!(spaces
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "Test Space"));

    // get_gateway_status returns the exact shape.
    let status: serde_json::Value = h
        .auth(c.post(rpc("get_gateway_status")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for key in ["running", "url", "active_sessions", "connected_backends"] {
        assert!(status.get(key).is_some(), "missing {key} in status");
    }

    // Desktop-only command → 501; unknown → 404; no token → 401.
    assert_eq!(
        h.auth(c.post(rpc("start_gateway")))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_IMPLEMENTED
    );
    assert_eq!(
        h.auth(c.post(rpc("bogus_command")))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    assert_eq!(
        c.post(rpc("list_spaces")).send().await.unwrap().status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn rpc_create_space_feature_set_binding_core_loop() {
    let h = Harness::start().await;
    let c = reqwest::Client::new();
    let rpc = |cmd: &str| format!("{}/admin/api/rpc/{cmd}", h.url);

    // Create a Space.
    let space: serde_json::Value = h
        .auth(
            c.post(rpc("create_space"))
                .json(&serde_json::json!({ "name": "RPC Space" })),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let space_id = space["id"].as_str().unwrap().to_string();
    assert_eq!(space["name"], "RPC Space");

    // Create a FeatureSet in it (via { input }).
    let fs: serde_json::Value = h
        .auth(c.post(rpc("create_feature_set")).json(&serde_json::json!({
            "input": { "name": "RPC FS", "space_id": space_id }
        })))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let fs_id = fs["id"].as_str().unwrap().to_string();

    // Create an id-mapping to that FS.
    let binding: serde_json::Value = h
        .auth(
            c.post(rpc("create_workspace_binding"))
                .json(&serde_json::json!({
                    "input": {
                        "workspace_root": "rpc-ws",
                        "space_id": space_id,
                        "feature_set_ids": [fs_id],
                        "binding_type": "id"
                    }
                })),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(binding["workspace_root"], "rpc-ws");
    let binding_id = binding["id"].as_str().unwrap().to_string();

    // The binding shows up in the list.
    let bindings: serde_json::Value = h
        .auth(c.post(rpc("list_workspace_bindings")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(bindings
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["workspace_root"] == "rpc-ws"));

    // Delete it via the typed DELETE endpoint.
    let del = h
        .auth(c.delete(format!("{}/admin/api/bindings/{binding_id}", h.url)))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn sse_delivers_ui_named_events() {
    let h = Harness::start().await;
    let c = reqwest::Client::new();

    // Open the SSE stream (token via query — the EventSource path).
    let resp = c
        .get(format!("{}/admin/api/events?token={TOKEN}", h.url))
        .send()
        .await
        .expect("sse connect");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .contains("text/event-stream"));

    let mut stream = resp.bytes_stream();

    // Trigger an event by creating a Space over RPC.
    let c2 = c.clone();
    let url = h.url.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = c2
            .post(format!("{url}/admin/api/rpc/create_space"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .json(&serde_json::json!({ "name": "SSE Space" }))
            .send()
            .await;
    });

    // Read until we see the mapped UI event name.
    let mut buf = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let got = loop {
        if tokio::time::Instant::now() > deadline {
            break false;
        }
        match tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                if buf.contains("event: space-changed") && buf.contains("space_created") {
                    break true;
                }
            }
            _ => continue,
        }
    };
    assert!(got, "expected a space-changed SSE event; got: {buf:?}");
}
