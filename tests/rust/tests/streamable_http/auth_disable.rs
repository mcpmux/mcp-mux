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
    routing::{get, post},
    Router,
};
use mcpmux_core::{DomainEvent, ServerDiscoveryService, ServerLogManager};
use mcpmux_gateway::{
    mcp::mcp_oauth_middleware,
    server::{
        oauth_metadata, resource_metadata, AppState, DependenciesBuilder, GatewayDependencies,
        GatewayState, ServiceContainer,
    },
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
    base: String,
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

        let mcp_router = Router::new().route("/mcp", post(echo_client_id)).layer(
            middleware::from_fn_with_state(services.clone(), mcp_oauth_middleware),
        );

        // Mount the OAuth-discovery endpoints so we can assert they 404 when
        // inbound auth is disabled (don't advertise auth the gateway won't ask
        // for).
        let app_state = AppState {
            gateway_state: services.gateway_state.clone(),
            services: services.clone(),
            base_url: "http://127.0.0.1:0".to_string(),
        };
        let discovery_router = Router::new()
            .route(
                "/.well-known/oauth-protected-resource",
                get(resource_metadata),
            )
            // RFC 9728 resource-specific variant — this is the one editors like
            // VS Code probe first (`/.well-known/oauth-protected-resource/mcp`).
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(resource_metadata),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                get(oauth_metadata),
            )
            .with_state(app_state);
        let router = mcp_router.merge(discovery_router);

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

#[tokio::test]
async fn authless_gateway_does_not_advertise_oauth_discovery() {
    // With inbound auth disabled, the OAuth-discovery endpoints must 404 so MCP
    // clients don't start an OAuth flow against a gateway that accepts them
    // without a token.
    let h = Harness::start(true).await;
    let client = reqwest::Client::new();
    // Includes the RFC 9728 `/mcp` sub-path — the endpoint VS Code probes first
    // (its 200 was what pushed editors into an OAuth flow against an authless
    // gateway, leaving them stuck waiting on `initialize`).
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-authorization-server",
    ] {
        let resp = client
            .get(format!("{}{path}", h.base))
            .send()
            .await
            .expect("request");
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::NOT_FOUND,
            "{path} must 404 when auth is disabled"
        );
    }
}

#[tokio::test]
async fn auth_required_gateway_advertises_oauth_discovery() {
    // The default (auth required) still serves discovery so real OAuth works —
    // every endpoint, including the RFC 9728 sub-path.
    let h = Harness::start(false).await;
    let client = reqwest::Client::new();
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-authorization-server",
    ] {
        let resp = client
            .get(format!("{}{path}", h.base))
            .send()
            .await
            .expect("request");
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::OK,
            "{path} must be served when auth is required"
        );
    }
}

/// POST `tools/list` to the harness, optionally from a browser `Origin`.
async fn post_tools_list(h: &Harness, origin: Option<&str>) -> reqwest::StatusCode {
    let mut req = reqwest::Client::new()
        .post(&h.url)
        .header("content-type", "application/json")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    if let Some(origin) = origin {
        req = req.header("origin", origin);
    }
    req.send().await.expect("request").status()
}

#[tokio::test]
async fn authless_gateway_blocks_web_pages() {
    // Running without an access key is only safe if a website open in the
    // user's browser can't drive the gateway: browsers stamp cross-site
    // requests with `Origin`, native MCP clients don't.
    let h = Harness::start(true).await;
    for origin in ["https://evil.example", "null", "http://192.168.1.20:45818"] {
        assert_eq!(
            post_tools_list(&h, Some(origin)).await,
            reqwest::StatusCode::FORBIDDEN,
            "a request from {origin} must be blocked"
        );
    }
}

#[tokio::test]
async fn authless_gateway_still_serves_local_apps() {
    // Native clients (no Origin) and pages served from this machine (e.g. the
    // MCP Inspector on localhost) keep working.
    let h = Harness::start(true).await;
    for origin in [
        None,
        Some("http://localhost:6274"),
        Some("http://127.0.0.1:5173"),
    ] {
        assert_eq!(
            post_tools_list(&h, origin).await,
            reqwest::StatusCode::OK,
            "a request from {origin:?} must be accepted"
        );
    }
}

#[tokio::test]
async fn origin_guard_applies_when_auth_is_required_too() {
    // The guard runs before auth, so a web page gets 403 (not a 401 inviting
    // it to start OAuth) whether or not access keys are required.
    let h = Harness::start(false).await;
    assert_eq!(
        post_tools_list(&h, Some("https://evil.example")).await,
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(
        post_tools_list(&h, None).await,
        reqwest::StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn oversized_mcp_bodies_are_refused() {
    let h = Harness::start(true).await;
    let body = vec![b' '; mcpmux_gateway::mcp::MAX_MCP_REQUEST_BODY + 1];
    let status = reqwest::Client::new()
        .post(&h.url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .expect("request")
        .status();
    assert_eq!(status, reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    // Ordinary requests are unaffected.
    assert_eq!(post_tools_list(&h, None).await, reqwest::StatusCode::OK);
}

/// The `/mcp` cap also holds for a chunked body (no Content-Length).
#[tokio::test]
async fn oversized_chunked_mcp_bodies_are_refused() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let h = Harness::start(true).await;
    let addr = h.base.trim_start_matches("http://").to_string();
    let stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();
    // Read the answer while still sending: the server answers 413 as soon as
    // the cap is passed and closes, and on Windows that close can turn into a
    // reset that discards a response nobody had read yet.
    let read = tokio::spawn(async move {
        let mut response = vec![0u8; 64];
        reader
            .read(&mut response)
            .await
            .map(|n| String::from_utf8_lossy(&response[..n]).into_owned())
    });

    let mut write_failed = writer
        .write_all(
            format!(
                "POST /mcp HTTP/1.1\r\nhost: {addr}\r\ncontent-type: application/json\r\n\
                 transfer-encoding: chunked\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .is_err();
    let chunk = vec![b' '; 1024 * 1024];
    let header = format!("{:x}\r\n", chunk.len());
    for _ in 0..(mcpmux_gateway::mcp::MAX_MCP_REQUEST_BODY / chunk.len() + 1) {
        if write_failed {
            break; // the server already answered and closed
        }
        write_failed = writer.write_all(header.as_bytes()).await.is_err()
            || writer.write_all(&chunk).await.is_err()
            || writer.write_all(b"\r\n").await.is_err();
    }
    if !write_failed {
        let _ = writer.write_all(b"0\r\n\r\n").await;
    }

    match read.await.unwrap() {
        Ok(status_line) => assert!(status_line.starts_with("HTTP/1.1 413"), "{status_line}"),
        // The reset beat the status line: only acceptable when the server cut
        // the upload off, which it does only after refusing it.
        Err(e) => assert!(
            write_failed
                && matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ),
            "{e} (write failed: {write_failed})"
        ),
    }
}

/// Routes other than `/mcp` read their bodies before any auth, so the logging
/// middleware caps them.
#[tokio::test]
async fn oversized_bodies_on_other_routes_are_refused() {
    use axum::routing::post;
    let router = Router::new()
        .route("/oauth/token", post(|| async { "ok" }))
        .layer(axum::middleware::from_fn(
            mcpmux_gateway::server::logging_middleware::http_logging_middleware,
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let big = vec![b'a'; mcpmux_gateway::server::logging_middleware::MAX_NON_MCP_REQUEST_BODY + 1];
    let status = reqwest::Client::new()
        .post(&url)
        .body(big)
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let status = reqwest::Client::new()
        .post(&url)
        .body("grant_type=x")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, reqwest::StatusCode::OK);
}
