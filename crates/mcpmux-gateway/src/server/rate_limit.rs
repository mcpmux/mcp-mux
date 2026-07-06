//! Rate limiting middleware for the gateway.
//!
//! Two limiters live here:
//!   * [`RateLimiter`] — the original per-path limiter for OAuth endpoints
//!     (all clients share a bucket per path; fine on loopback).
//!   * [`McpRateLimiter`] — a peer-aware limiter for the `/mcp` endpoint,
//!     installed only on a network bind. It caps request rate per
//!     (peer-IP, credential) and damps credential-stuffing by throttling a
//!     peer that produces repeated `401`s. On loopback the gateway stays
//!     unlimited (bulk local workflows must not be throttled).

use axum::{
    extract::{ConnectInfo, Request},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for a rate-limited route.
#[derive(Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed within the window.
    pub max_requests: u32,
    /// Time window duration.
    pub window: Duration,
}

/// Shared rate limiter state (clone-friendly via Arc).
#[derive(Clone)]
pub struct RateLimiter {
    /// Map from path prefix → (window_start, request_count).
    buckets: Arc<DashMap<String, (Instant, u32)>>,
    /// Configuration per route prefix.
    rules: Arc<Vec<(String, RateLimitConfig)>>,
}

impl RateLimiter {
    pub fn new(rules: Vec<(String, RateLimitConfig)>) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            rules: Arc::new(rules),
        }
    }

    /// Check if the request should be rate limited.
    /// Returns `true` if the request is within limits (allowed).
    fn check(&self, path: &str) -> bool {
        for (prefix, config) in self.rules.iter() {
            if path.starts_with(prefix) {
                let mut entry = self
                    .buckets
                    .entry(prefix.clone())
                    .or_insert_with(|| (Instant::now(), 0));
                let (window_start, count) = entry.value_mut();

                if window_start.elapsed() >= config.window {
                    // Reset window
                    *window_start = Instant::now();
                    *count = 1;
                    return true;
                }

                if *count >= config.max_requests {
                    return false; // Rate limited
                }

                *count += 1;
                return true;
            }
        }
        true // No matching rule, allow
    }
}

/// Axum middleware function for rate limiting.
pub async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    let limiter = request.extensions().get::<RateLimiter>().cloned();

    if let Some(limiter) = limiter {
        let path = request.uri().path().to_string();
        if !limiter.check(&path) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded. Please try again later.",
            )
                .into_response();
        }
    }

    next.run(request).await
}

// ---------------------------------------------------------------------------
// MCP endpoint limiter (network binds only)
// ---------------------------------------------------------------------------

/// Tuning for [`McpRateLimiter`].
#[derive(Clone, Debug)]
pub struct McpRateLimitConfig {
    /// Max `/mcp` requests per (peer-IP, credential) per [`Self::window`].
    pub max_requests: u32,
    /// Sliding window for the request cap.
    pub window: Duration,
    /// Consecutive `401`s from one peer-IP within [`Self::auth_failure_window`]
    /// before that peer is put in lockout.
    pub max_auth_failures: u32,
    /// Window over which auth failures accumulate (reset on success or expiry).
    pub auth_failure_window: Duration,
    /// How long a peer stays throttled once it trips the auth-failure limit.
    pub lockout: Duration,
}

impl Default for McpRateLimitConfig {
    fn default() -> Self {
        Self {
            // Generous for real clients (initialize + tools/list + calls), tight
            // enough to blunt a flood: a scanner hammering /mcp trips fast.
            max_requests: 240,
            window: Duration::from_secs(60),
            max_auth_failures: 10,
            auth_failure_window: Duration::from_secs(60),
            lockout: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Copy)]
struct Bucket {
    window_start: Instant,
    count: u32,
}

#[derive(Clone, Copy)]
struct FailureBucket {
    window_start: Instant,
    count: u32,
    /// When `Some`, the peer is locked out until this instant.
    locked_until: Option<Instant>,
}

/// Peer-aware limiter for `/mcp`. Cheap to clone (Arc-backed maps).
#[derive(Clone)]
pub struct McpRateLimiter {
    requests: Arc<DashMap<String, Bucket>>,
    failures: Arc<DashMap<String, FailureBucket>>,
    config: McpRateLimitConfig,
}

/// Outcome of an admission check.
enum Admit {
    Ok,
    /// Reject with 429 and this `Retry-After` (seconds).
    Limited(u64),
}

impl McpRateLimiter {
    pub fn new(config: McpRateLimitConfig) -> Self {
        Self {
            requests: Arc::new(DashMap::new()),
            failures: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Stable, non-reversible tag for a credential so different API keys from
    /// the same IP get separate request buckets without us storing the token.
    /// A tokenless request buckets as `anon` (per-IP flood protection).
    fn credential_tag(auth_header: Option<&str>) -> String {
        match auth_header {
            Some(v) if !v.is_empty() => {
                let mut h = DefaultHasher::new();
                v.hash(&mut h);
                format!("{:016x}", h.finish())
            }
            _ => "anon".to_string(),
        }
    }

    /// Check whether a peer is currently locked out for auth failures.
    fn check_lockout(&self, peer_ip: &str) -> Admit {
        if let Some(entry) = self.failures.get(peer_ip) {
            if let Some(until) = entry.locked_until {
                let now = Instant::now();
                if until > now {
                    return Admit::Limited((until - now).as_secs().max(1));
                }
            }
        }
        Admit::Ok
    }

    /// Admit (and count) a request keyed by peer-IP + credential.
    fn check_request(&self, key: &str) -> Admit {
        let now = Instant::now();
        let mut entry = self.requests.entry(key.to_string()).or_insert(Bucket {
            window_start: now,
            count: 0,
        });
        if entry.window_start.elapsed() >= self.config.window {
            entry.window_start = now;
            entry.count = 1;
            return Admit::Ok;
        }
        if entry.count >= self.config.max_requests {
            let retry = (self.config.window - entry.window_start.elapsed()).as_secs() + 1;
            return Admit::Limited(retry);
        }
        entry.count += 1;
        Admit::Ok
    }

    /// Record a `401` for a peer, arming a lockout once the threshold is hit.
    fn record_auth_failure(&self, peer_ip: &str) {
        let now = Instant::now();
        let mut entry = self
            .failures
            .entry(peer_ip.to_string())
            .or_insert(FailureBucket {
                window_start: now,
                count: 0,
                locked_until: None,
            });
        // Reset the counting window if it has elapsed (and we're not mid-lockout).
        if entry.locked_until.is_none()
            && entry.window_start.elapsed() >= self.config.auth_failure_window
        {
            entry.window_start = now;
            entry.count = 0;
        }
        entry.count += 1;
        if entry.count >= self.config.max_auth_failures {
            entry.locked_until = Some(now + self.config.lockout);
        }
    }

    /// Clear a peer's failure state after a successful (non-401) response.
    fn clear_auth_failures(&self, peer_ip: &str) {
        self.failures.remove(peer_ip);
    }
}

fn too_many(retry_after_secs: u64, msg: &'static str) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_after_secs.to_string())],
        msg,
    )
        .into_response()
}

/// Middleware for `/mcp`, installed only on network binds. Order matters: this
/// wraps the OAuth middleware so it can observe the `401` a rejected request
/// produces and feed the credential-stuffing damper.
pub async fn mcp_rate_limit_middleware(request: Request, next: Next) -> Response {
    let Some(limiter) = request.extensions().get::<McpRateLimiter>().cloned() else {
        return next.run(request).await;
    };

    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        // No ConnectInfo (embedded/test server) → single shared bucket.
        .unwrap_or_else(|| "unknown".to_string());

    // 1) Locked out for repeated auth failures?
    if let Admit::Limited(retry) = limiter.check_lockout(&peer_ip) {
        return too_many(
            retry,
            "Too many failed authentication attempts. Try again later.",
        );
    }

    // 2) Per-(peer, credential) request cap.
    let cred = McpRateLimiter::credential_tag(
        request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
    );
    let key = format!("{peer_ip}|{cred}");
    if let Admit::Limited(retry) = limiter.check_request(&key) {
        return too_many(retry, "Rate limit exceeded. Please slow down.");
    }

    // 3) Run, then feed the auth-failure damper from the response status.
    let response = next.run(request).await;
    if response.status() == StatusCode::UNAUTHORIZED {
        limiter.record_auth_failure(&peer_ip);
    } else if response.status().is_success() {
        limiter.clear_auth_failures(&peer_ip);
    }
    response
}

/// Create the default rate limiter for OAuth endpoints.
pub fn default_oauth_rate_limiter() -> RateLimiter {
    RateLimiter::new(vec![
        (
            "/oauth/authorize".to_string(),
            RateLimitConfig {
                max_requests: 30,
                window: Duration::from_secs(60),
            },
        ),
        (
            "/authorize".to_string(),
            RateLimitConfig {
                max_requests: 30,
                window: Duration::from_secs(60),
            },
        ),
        (
            "/oauth/token".to_string(),
            RateLimitConfig {
                max_requests: 60,
                window: Duration::from_secs(60),
            },
        ),
        (
            "/oauth/register".to_string(),
            RateLimitConfig {
                max_requests: 20,
                window: Duration::from_secs(60),
            },
        ),
        (
            "/oauth/clients".to_string(),
            RateLimitConfig {
                max_requests: 30,
                window: Duration::from_secs(60),
            },
        ),
        (
            // Token-gated device pairing — tight cap blunts token guessing.
            "/pair/claim".to_string(),
            RateLimitConfig {
                max_requests: 20,
                window: Duration::from_secs(60),
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_cfg() -> McpRateLimitConfig {
        McpRateLimitConfig {
            max_requests: 3,
            window: Duration::from_secs(60),
            max_auth_failures: 3,
            auth_failure_window: Duration::from_secs(60),
            lockout: Duration::from_secs(60),
        }
    }

    #[test]
    fn request_cap_is_per_peer_and_credential() {
        let rl = McpRateLimiter::new(small_cfg());
        // Peer A, key K1: 3 allowed, 4th limited.
        for _ in 0..3 {
            assert!(matches!(rl.check_request("a|k1"), Admit::Ok));
        }
        assert!(matches!(rl.check_request("a|k1"), Admit::Limited(_)));
        // A different credential from the same peer is a separate bucket.
        assert!(matches!(rl.check_request("a|k2"), Admit::Ok));
        // A different peer is unaffected.
        assert!(matches!(rl.check_request("b|k1"), Admit::Ok));
    }

    #[test]
    fn retry_after_is_positive_when_limited() {
        let rl = McpRateLimiter::new(small_cfg());
        for _ in 0..3 {
            let _ = rl.check_request("a|k1");
        }
        match rl.check_request("a|k1") {
            Admit::Limited(secs) => assert!(secs >= 1),
            Admit::Ok => panic!("expected limited"),
        }
    }

    #[test]
    fn repeated_auth_failures_lock_the_peer_out() {
        let rl = McpRateLimiter::new(small_cfg());
        // Below threshold: not locked.
        rl.record_auth_failure("1.2.3.4");
        rl.record_auth_failure("1.2.3.4");
        assert!(matches!(rl.check_lockout("1.2.3.4"), Admit::Ok));
        // Threshold reached → lockout with a Retry-After.
        rl.record_auth_failure("1.2.3.4");
        assert!(matches!(rl.check_lockout("1.2.3.4"), Admit::Limited(_)));
        // A different peer is unaffected.
        assert!(matches!(rl.check_lockout("5.6.7.8"), Admit::Ok));
    }

    #[test]
    fn success_clears_auth_failures() {
        let rl = McpRateLimiter::new(small_cfg());
        rl.record_auth_failure("1.2.3.4");
        rl.record_auth_failure("1.2.3.4");
        rl.clear_auth_failures("1.2.3.4");
        // Counter reset: three more are needed to lock out again.
        rl.record_auth_failure("1.2.3.4");
        rl.record_auth_failure("1.2.3.4");
        assert!(matches!(rl.check_lockout("1.2.3.4"), Admit::Ok));
    }

    #[test]
    fn credential_tag_separates_tokens_and_buckets_anon() {
        let a = McpRateLimiter::credential_tag(Some("Bearer aaa"));
        let b = McpRateLimiter::credential_tag(Some("Bearer bbb"));
        assert_ne!(a, b, "different tokens must map to different buckets");
        assert_eq!(McpRateLimiter::credential_tag(None), "anon");
        assert_eq!(McpRateLimiter::credential_tag(Some("")), "anon");
        // Same token is stable across calls.
        assert_eq!(a, McpRateLimiter::credential_tag(Some("Bearer aaa")));
    }
}
