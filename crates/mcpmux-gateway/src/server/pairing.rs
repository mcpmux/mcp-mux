//! Device pairing tokens.
//!
//! A pairing token is a short-lived, single-use secret minted on the trusted
//! desktop (via a Tauri command) and carried — typically in a QR code — to a
//! new device. The device exchanges it at `POST /pair/claim` for a freshly
//! minted per-device API key. The token is the proof-of-trust for that
//! exchange: it is never long-lived, never reusable, and can only be minted
//! from the host machine, so a claim endpoint reachable on the LAN cannot be
//! used to self-issue credentials without physical/desktop access.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Default lifetime of a pairing token.
pub const DEFAULT_PAIRING_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// Shared, thread-safe store of outstanding pairing tokens. Cheap to clone.
#[derive(Clone, Default)]
pub struct PairingTokenStore {
    /// token -> expiry instant. Presence == valid; removal == consumed.
    tokens: Arc<DashMap<String, Instant>>,
}

impl PairingTokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a new single-use token valid for `ttl`. Opportunistically drops
    /// any already-expired tokens so the map can't grow unbounded from
    /// abandoned pairings.
    pub fn mint(&self, ttl: Duration) -> String {
        self.sweep();
        let token = format!(
            "mcppair_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        self.tokens.insert(token.clone(), Instant::now() + ttl);
        token
    }

    /// Consume a token: returns `true` exactly once for a valid, unexpired
    /// token (and removes it), `false` for unknown/expired/already-used.
    pub fn consume(&self, token: &str) -> bool {
        match self.tokens.remove(token) {
            Some((_, expiry)) => expiry > Instant::now(),
            None => false,
        }
    }

    /// Number of outstanding (not-yet-swept) tokens — for diagnostics/tests.
    pub fn outstanding(&self) -> usize {
        self.tokens.len()
    }

    fn sweep(&self) {
        let now = Instant::now();
        self.tokens.retain(|_, expiry| *expiry > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_single_use() {
        let store = PairingTokenStore::new();
        let t = store.mint(DEFAULT_PAIRING_TTL);
        assert!(store.consume(&t), "first use accepted");
        assert!(!store.consume(&t), "second use rejected");
    }

    #[test]
    fn unknown_token_rejected() {
        let store = PairingTokenStore::new();
        assert!(!store.consume("mcppair_nope"));
    }

    #[test]
    fn expired_token_rejected_and_swept() {
        let store = PairingTokenStore::new();
        let t = store.mint(Duration::from_millis(0)); // already expired
        assert!(!store.consume(&t), "expired token must be rejected");
        // Minting sweeps expired entries.
        let _ = store.mint(DEFAULT_PAIRING_TTL);
        assert_eq!(store.outstanding(), 1);
    }

    #[test]
    fn tokens_are_unique_and_prefixed() {
        let store = PairingTokenStore::new();
        let a = store.mint(DEFAULT_PAIRING_TTL);
        let b = store.mint(DEFAULT_PAIRING_TTL);
        assert_ne!(a, b);
        assert!(a.starts_with("mcppair_"));
    }
}
