//! Simple per-route rate limiting middleware for the gateway.
//!
//! Uses a DashMap to track request counts per (route prefix, peer IP) in a
//! fixed window. On a loopback-only gateway every caller shares 127.0.0.1;
//! on a network bind each peer gets its own budget.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use std::net::{IpAddr, SocketAddr};
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

/// Bucket key: (route prefix, peer IP when known; IPv6 peers by /64).
type BucketKey = (String, Option<IpAddr>);

/// Above this many buckets, expired ones are dropped on the next request.
const MAX_BUCKETS: usize = 10_000;

/// The address a peer is counted under: IPv6 addresses by their /64, which
/// one host usually controls in full.
fn peer_key(peer: IpAddr) -> IpAddr {
    match peer {
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            IpAddr::V6(std::net::Ipv6Addr::from(octets))
        }
        v4 => v4,
    }
}

/// Shared rate limiter state (clone-friendly via Arc).
#[derive(Clone)]
pub struct RateLimiter {
    /// Map from bucket key → (window_start, request_count).
    buckets: Arc<DashMap<BucketKey, (Instant, u32)>>,
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
    fn check(&self, path: &str, peer: Option<IpAddr>) -> bool {
        let peer = peer.map(peer_key);
        if self.buckets.len() > MAX_BUCKETS {
            self.prune();
        }
        for (prefix, config) in self.rules.iter() {
            if path.starts_with(prefix) {
                let mut entry = self
                    .buckets
                    .entry((prefix.clone(), peer))
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

    /// Drop buckets whose window has passed (for every rule, so the longest
    /// window decides).
    fn prune(&self) {
        let longest = self
            .rules
            .iter()
            .map(|(_, config)| config.window)
            .max()
            .unwrap_or_default();
        self.buckets
            .retain(|_, (window_start, _)| window_start.elapsed() < longest);
    }
}

/// Axum middleware function for rate limiting. Install it with
/// `middleware::from_fn_with_state(limiter, rate_limit_middleware)`.
pub async fn rate_limit_middleware(
    State(limiter): State<RateLimiter>,
    request: Request,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    if !limiter.check(request.uri().path(), peer) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Please try again later.",
        )
            .into_response();
    }

    next.run(request).await
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
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(max_requests: u32) -> RateLimiter {
        RateLimiter::new(vec![(
            "/oauth/token".to_string(),
            RateLimitConfig {
                max_requests,
                window: Duration::from_secs(60),
            },
        )])
    }

    #[test]
    fn limits_each_peer_separately() {
        let limiter = limiter(2);
        let a: IpAddr = "192.0.2.1".parse().unwrap();
        let b: IpAddr = "192.0.2.2".parse().unwrap();

        assert!(limiter.check("/oauth/token", Some(a)));
        assert!(limiter.check("/oauth/token", Some(a)));
        assert!(!limiter.check("/oauth/token", Some(a)));
        assert!(limiter.check("/oauth/token", Some(b)));
    }

    #[test]
    fn unlisted_paths_are_not_limited() {
        let limiter = limiter(1);
        for _ in 0..5 {
            assert!(limiter.check("/health", None));
        }
    }
}

#[cfg(test)]
mod pruning_tests {
    use super::*;

    fn limiter() -> RateLimiter {
        RateLimiter::new(vec![(
            "/oauth/token".to_string(),
            RateLimitConfig {
                max_requests: 1,
                window: Duration::from_millis(50),
            },
        )])
    }

    #[test]
    fn ipv6_peers_share_a_bucket_per_64() {
        let limiter = limiter();
        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert!(limiter.check("/oauth/token", Some(a)));
        assert!(!limiter.check("/oauth/token", Some(b)), "same /64");
        assert!(limiter.check("/oauth/token", Some(other)), "another /64");
    }

    #[test]
    fn expired_buckets_are_dropped_once_there_are_many() {
        let limiter = limiter();
        for i in 0..=MAX_BUCKETS as u32 {
            limiter.check("/oauth/token", Some(IpAddr::from(i.to_be_bytes())));
        }
        assert!(limiter.buckets.len() > MAX_BUCKETS);
        std::thread::sleep(Duration::from_millis(60));
        limiter.check("/oauth/token", Some("192.0.2.1".parse().unwrap()));
        assert!(limiter.buckets.len() <= 1, "{}", limiter.buckets.len());
    }
}
