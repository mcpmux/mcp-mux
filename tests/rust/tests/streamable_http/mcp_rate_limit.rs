//! Integration coverage for the `/mcp` per-peer rate limiter
//! (`McpRateLimiter` + `mcp_rate_limit_middleware`), exercised over real HTTP
//! with `ConnectInfo` so peer-IP keying is live.
//!
//! We don't boot the whole gateway here — the limiter is transport-level and
//! independent of MCP semantics, so a tiny stand-in `/mcp` handler (whose
//! status we control via a header) lets us assert, against the SAME middleware
//! the gateway installs on a network bind:
//!   - a burst over the per-(peer, credential) cap gets 429 + `Retry-After`,
//!   - a peer that produces repeated 401s is thrown into lockout (429).

use axum::{
    extract::Request,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use mcpmux_gateway::server::rate_limit::{
    mcp_rate_limit_middleware, McpRateLimitConfig, McpRateLimiter,
};
use std::net::SocketAddr;
use tokio_util::sync::CancellationToken;

/// Stand-in `/mcp`: echoes 200 normally, or the status named by the
/// `x-test-status` header (used to simulate the OAuth middleware's 401).
async fn fake_mcp(req: Request) -> Response {
    if let Some(code) = req
        .headers()
        .get("x-test-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u16>().ok())
    {
        return (StatusCode::from_u16(code).unwrap(), "forced").into_response();
    }
    (StatusCode::OK, "ok").into_response()
}

struct Harness {
    url: String,
    ct: CancellationToken,
}

impl Harness {
    async fn start(config: McpRateLimitConfig) -> Self {
        let ct = CancellationToken::new();
        let limiter = McpRateLimiter::new(config);
        let app = Router::new()
            .route("/mcp", post(fake_mcp))
            .layer(middleware::from_fn(mcp_rate_limit_middleware))
            .layer(axum::Extension(limiter));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        let ct_clone = ct.clone();
        tokio::spawn(async move {
            // into_make_service_with_connect_info supplies ConnectInfo so the
            // limiter keys on the real peer IP (as the gateway does).
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
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
async fn burst_over_request_cap_returns_429_with_retry_after() {
    let h = Harness::start(McpRateLimitConfig {
        max_requests: 3,
        window: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .await;
    let client = reqwest::Client::new();

    // First 3 (same peer, same/absent credential) are allowed.
    for _ in 0..3 {
        let resp = client.post(&h.url).send().await.expect("req");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
    }
    // 4th trips the cap.
    let resp = client.post(&h.url).send().await.expect("req");
    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let retry = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .expect("Retry-After header present and numeric");
    assert!(
        retry >= 1,
        "Retry-After must be a positive number of seconds"
    );
}

#[tokio::test]
async fn distinct_credentials_get_separate_buckets() {
    let h = Harness::start(McpRateLimitConfig {
        max_requests: 2,
        window: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .await;
    let client = reqwest::Client::new();

    // Exhaust key A.
    for _ in 0..2 {
        assert_eq!(
            client
                .post(&h.url)
                .header("authorization", "Bearer AAA")
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::OK
        );
    }
    assert_eq!(
        client
            .post(&h.url)
            .header("authorization", "Bearer AAA")
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::TOO_MANY_REQUESTS
    );
    // A different credential from the same peer is still fine.
    assert_eq!(
        client
            .post(&h.url)
            .header("authorization", "Bearer BBB")
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );
}

#[tokio::test]
async fn repeated_401s_lock_the_peer_out() {
    let h = Harness::start(McpRateLimitConfig {
        // Keep the request cap out of the way; exercise the auth damper.
        max_requests: 1000,
        window: std::time::Duration::from_secs(60),
        max_auth_failures: 3,
        auth_failure_window: std::time::Duration::from_secs(60),
        lockout: std::time::Duration::from_secs(60),
    })
    .await;
    let client = reqwest::Client::new();

    // Three forced 401s reach the threshold.
    for _ in 0..3 {
        let resp = client
            .post(&h.url)
            .header("x-test-status", "401")
            .send()
            .await
            .expect("req");
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    }
    // The next request from this peer is pre-emptively throttled — even a
    // would-be-valid one — until the lockout expires.
    let resp = client.post(&h.url).send().await.expect("req");
    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().get("retry-after").is_some());
}
