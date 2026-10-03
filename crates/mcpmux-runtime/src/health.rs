//! Post-spawn `/health` probe.
//!
//! After `GatewayServer::spawn()` returns, the daemon (or any operator
//! tool) needs to confirm the listener is actually serving before it
//! declares "ready" to systemd or before any inbound MCP client tries to
//! connect. `wait_for_health` polls `/health` with backoff until success or
//! timeout.
//!
//! The gateway exposes `/health` as a public, unauthenticated route; this
//! is intentional and matches the existing production behavior.

use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, warn};

/// How long to wait and how often to retry before giving up.
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Loopback URL to probe, e.g. `http://127.0.0.1:45818`.
    pub url: String,
    /// Total time to spend retrying before returning `Unreachable`.
    pub timeout: Duration,
    /// Initial backoff between attempts. Doubles per failed attempt up to
    /// [`HealthCheckConfig::max_backoff`].
    pub initial_backoff: Duration,
    /// Cap on backoff growth between attempts.
    pub max_backoff: Duration,
}

impl HealthCheckConfig {
    pub fn new(url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            url: url.into(),
            timeout,
            initial_backoff: Duration::from_millis(25),
            max_backoff: Duration::from_millis(500),
        }
    }
}

/// Result of a `/health` probe.
#[derive(Debug)]
pub enum HealthStatus {
    /// The gateway responded 200 and reports itself healthy.
    Ok {
        version: Option<String>,
        round_trip: Duration,
    },
    /// Probes exhausted `timeout` without a 200 response.
    Unreachable { attempts: u32, last_error: String },
    /// The probe URL itself is malformed (e.g. the bind host didn't parse).
    InvalidUrl(String),
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    #[serde(default)]
    version: Option<String>,
    #[allow(dead_code)]
    status: Option<String>,
}

/// Poll `/health` until success or timeout.
///
/// `reqwest::Client::get` does not require a Tokio runtime, but we use the
/// shared client from the gateway deps when available in the caller. The
/// function here constructs a short-lived client; for tests the same code
/// path works because reqwest builds a default-tokio client.
pub async fn wait_for_health(config: &HealthCheckConfig) -> HealthStatus {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(c) => c,
        Err(e) => return HealthStatus::InvalidUrl(format!("client build failed: {}", e)),
    };

    let deadline = tokio::time::Instant::now() + config.timeout;
    let mut backoff = config.initial_backoff;
    let mut attempts: u32 = 0;
    let mut last_error = String::from("no attempts yet");

    while tokio::time::Instant::now() < deadline {
        attempts += 1;
        let started = tokio::time::Instant::now();
        match client.get(&config.url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let round_trip = started.elapsed();
                let body: Option<HealthResponse> = resp.json().await.ok();
                return HealthStatus::Ok {
                    version: body.and_then(|b| b.version),
                    round_trip,
                };
            }
            Ok(resp) => {
                last_error = format!("HTTP {}", resp.status());
                debug!(status = %resp.status(), "[health] non-2xx response");
            }
            Err(e) => {
                last_error = e.to_string();
                debug!(error = %e, "[health] probe failed");
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(config.max_backoff);
    }

    warn!(
        attempts,
        "[health] probe timed out without a successful response"
    );

    HealthStatus::Unreachable {
        attempts,
        last_error,
    }
}
