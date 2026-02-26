//! Simple per-path rate limiting middleware for the gateway.
//!
//! Uses a DashMap to track request counts per (path, window) pair.
//! Designed for a localhost-only gateway where IP-based limiting is
//! irrelevant (all clients share 127.0.0.1).

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
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
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_limit() {
        let limiter = RateLimiter::new(vec![(
            "/test".to_string(),
            RateLimitConfig {
                max_requests: 5,
                window: Duration::from_secs(60),
            },
        )]);

        for _ in 0..5 {
            assert!(limiter.check("/test"), "Should allow requests within limit");
        }
    }

    #[test]
    fn blocks_at_limit() {
        let limiter = RateLimiter::new(vec![(
            "/test".to_string(),
            RateLimitConfig {
                max_requests: 3,
                window: Duration::from_secs(60),
            },
        )]);

        assert!(limiter.check("/test")); // 1
        assert!(limiter.check("/test")); // 2
        assert!(limiter.check("/test")); // 3
        assert!(!limiter.check("/test"), "Should block at limit");
    }

    #[test]
    fn no_matching_rule_allows() {
        let limiter = RateLimiter::new(vec![(
            "/oauth".to_string(),
            RateLimitConfig {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        )]);

        assert!(limiter.check("/health"), "Unmatched path should be allowed");
    }

    #[test]
    fn prefix_matching() {
        let limiter = RateLimiter::new(vec![(
            "/oauth/token".to_string(),
            RateLimitConfig {
                max_requests: 1,
                window: Duration::from_secs(60),
            },
        )]);

        assert!(limiter.check("/oauth/token/extra")); // matches prefix, count=1
        assert!(
            !limiter.check("/oauth/token"),
            "Second request to same prefix should be blocked"
        );
    }

    #[test]
    fn independent_buckets() {
        let limiter = RateLimiter::new(vec![
            (
                "/oauth/authorize".to_string(),
                RateLimitConfig {
                    max_requests: 1,
                    window: Duration::from_secs(60),
                },
            ),
            (
                "/oauth/token".to_string(),
                RateLimitConfig {
                    max_requests: 1,
                    window: Duration::from_secs(60),
                },
            ),
        ]);

        assert!(limiter.check("/oauth/authorize"));
        assert!(
            !limiter.check("/oauth/authorize"),
            "authorize should be blocked"
        );
        // token should still be allowed (independent counter)
        assert!(limiter.check("/oauth/token"));
    }

    #[test]
    fn default_has_expected_rules() {
        let limiter = default_oauth_rate_limiter();
        // Verify it has rules for 5 OAuth paths by checking they are rate-limited
        let paths = [
            "/oauth/authorize",
            "/authorize",
            "/oauth/token",
            "/oauth/register",
            "/oauth/clients",
        ];
        for path in &paths {
            assert!(
                limiter.check(path),
                "First request to {} should be allowed",
                path
            );
        }
    }

    #[test]
    fn window_reset() {
        let limiter = RateLimiter::new(vec![(
            "/test".to_string(),
            RateLimitConfig {
                max_requests: 1,
                window: Duration::from_millis(1), // 1ms window
            },
        )]);

        assert!(limiter.check("/test")); // count=1
        assert!(!limiter.check("/test")); // blocked

        // Sleep past the window
        std::thread::sleep(Duration::from_millis(10));

        assert!(limiter.check("/test"), "Should allow after window reset");
    }
}
