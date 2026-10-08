//! HTTP-surface tests against the real gateway router.
//!
//! These go through `GatewayServer::spawn()`, so they exercise the middleware
//! stack exactly as `build_router` wires it (unlike tests that mount handlers
//! on their own `Router`).

use mcpmux_gateway::{GatewayConfig, GatewayServerHandle};

use super::Fixture;

/// A spawned gateway on its own data dir; shuts down on drop of the handle.
struct LiveGateway {
    base: String,
    handle: Option<GatewayServerHandle>,
    _runtime: std::sync::Arc<mcpmux_runtime::Runtime>,
    _fixture: Fixture,
}

impl LiveGateway {
    async fn start() -> Self {
        let fixture = Fixture::new();
        let runtime = super::runtime_builder()
            .with_data_dir(fixture.data_dir())
            .build()
            .await
            .expect("runtime build");
        let server = runtime
            .build_gateway_server(GatewayConfig {
                host: "127.0.0.1".to_string(),
                // Port 0: the OS picks a free port (nextest runs each test in
                // its own process, so a shared port counter can collide).
                port: 0,
                public_base_url: None,
                enable_cors: true,
            })
            .await
            .expect("gateway build");
        let mut handle = server.spawn();
        let addr = handle.wait_until_bound().await.expect("gateway bound");
        Self {
            base: format!("http://{addr}"),
            handle: Some(handle),
            _runtime: runtime,
            _fixture: fixture,
        }
    }

    async fn stop(mut self) {
        if let Some(handle) = self.handle.take() {
            mcpmux_runtime::shutdown_gateway_handle(handle).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oauth_token_endpoint_is_rate_limited() {
    let gateway = LiveGateway::start().await;
    let http = reqwest::Client::new();

    let mut statuses = Vec::new();
    for _ in 0..70 {
        let response = http
            .post(format!("{}/oauth/token", gateway.base))
            .form(&[("grant_type", "authorization_code"), ("code", "nope")])
            .send()
            .await
            .expect("token request");
        statuses.push(response.status());
    }

    assert_eq!(statuses[0], reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        statuses.last().copied(),
        Some(reqwest::StatusCode::TOO_MANY_REQUESTS),
        "the limiter (60/min on /oauth/token) must engage"
    );
    gateway.stop().await;
}
