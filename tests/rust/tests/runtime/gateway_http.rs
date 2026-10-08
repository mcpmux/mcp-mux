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

const EVIL_ORIGIN: &str = "https://evil.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_pages_are_refused_on_every_route() {
    let gateway = LiveGateway::start().await;
    let http = reqwest::Client::new();

    // A cross-site request is refused before any handler runs.
    let register = http
        .post(format!("{}/oauth/register", gateway.base))
        .header("origin", EVIL_ORIGIN)
        .json(&serde_json::json!({
            "client_name": "page",
            "redirect_uris": ["http://127.0.0.1:9/cb"]
        }))
        .send()
        .await
        .expect("register");
    assert_eq!(register.status(), reqwest::StatusCode::FORBIDDEN);

    // Preflights from a web page get no CORS grant...
    let preflight = http
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/oauth/token", gateway.base),
        )
        .header("origin", EVIL_ORIGIN)
        .header("access-control-request-method", "POST")
        .send()
        .await
        .expect("preflight");
    assert!(preflight
        .headers()
        .get("access-control-allow-origin")
        .is_none());

    // ...while a tool served from this machine (e.g. the MCP Inspector) does.
    let local = "http://localhost:6274";
    let preflight = http
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/oauth/token", gateway.base),
        )
        .header("origin", local)
        .header("access-control-request-method", "POST")
        .send()
        .await
        .expect("preflight");
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok()),
        Some(local)
    );

    // Native clients (no Origin) keep working.
    let metadata = http
        .get(format!(
            "{}/.well-known/oauth-authorization-server",
            gateway.base
        ))
        .send()
        .await
        .expect("metadata");
    assert_eq!(metadata.status(), reqwest::StatusCode::OK);
    gateway.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unexpected_host_headers_are_refused() {
    let gateway = LiveGateway::start().await;
    let http = reqwest::Client::new();

    let response = http
        .get(format!(
            "{}/.well-known/oauth-authorization-server",
            gateway.base
        ))
        .header("host", "rebind.evil.example:45818")
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    gateway.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_management_is_not_served_over_http() {
    let gateway = LiveGateway::start().await;
    let http = reqwest::Client::new();

    for (method, path) in [
        (reqwest::Method::GET, "/oauth/clients"),
        (reqwest::Method::PUT, "/oauth/clients/mcp_12345678"),
        (reqwest::Method::DELETE, "/oauth/clients/mcp_12345678"),
        (reqwest::Method::GET, "/oauth/clients/mcp_12345678/features"),
    ] {
        let response = http
            .request(method.clone(), format!("{}{path}", gateway.base))
            .send()
            .await
            .expect("request");
        // Unknown paths fall through to the authenticated /mcp fallback, so
        // a removed route answers 401 (or 404/405), never with client data.
        let status = response.status();
        assert!(
            [
                reqwest::StatusCode::NOT_FOUND,
                reqwest::StatusCode::METHOD_NOT_ALLOWED,
                reqwest::StatusCode::UNAUTHORIZED,
            ]
            .contains(&status),
            "{method} {path} -> {status}"
        );
    }
    gateway.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authorize_only_answers_page_navigations() {
    let gateway = LiveGateway::start().await;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let url = format!(
        "{}/oauth/authorize?response_type=code&client_id=nobody&redirect_uri=http://127.0.0.1:9/cb",
        gateway.base
    );

    for dest in ["image", "iframe", "empty", "script"] {
        let response = http
            .get(&url)
            .header("sec-fetch-dest", dest)
            .send()
            .await
            .expect("authorize");
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN, "{dest}");
    }

    // A navigation reaches the handler (which then rejects the unknown client).
    let response = http
        .get(&url)
        .header("sec-fetch-dest", "document")
        .send()
        .await
        .expect("authorize");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    gateway.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_metadata_documents_are_never_fetched_from_local_addresses() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let gateway = LiveGateway::start().await;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let local_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&local_server)
        .await;

    for client_id in [
        format!("{}/client.json", local_server.uri()),
        format!(
            "https://localhost:{}/client.json",
            local_server.address().port()
        ),
        "https://169.254.169.254/latest/meta-data/".to_string(),
    ] {
        let enc: String = url::form_urlencoded::byte_serialize(client_id.as_bytes()).collect();
        let response = http
            .get(format!(
                "{}/oauth/authorize?response_type=code&client_id={enc}&redirect_uri=http://127.0.0.1:9/cb",
                gateway.base
            ))
            .send()
            .await
            .expect("authorize");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "{client_id}"
        );
    }
    gateway.stop().await;
    // Dropping the mock server verifies it received no request.
}
