//! PKCE (Proof Key for Code Exchange)
//!
//! Implements RFC 7636 for secure authorization code flow.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};

/// PKCE code verifier and challenge pair
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// The code verifier (kept secret, sent in token exchange)
    pub verifier: String,
    /// The code challenge (sent in authorization request)
    pub challenge: String,
    /// Challenge method (always S256)
    pub method: String,
}

impl PkceChallenge {
    /// Generate a new PKCE challenge
    pub fn generate() -> Self {
        // Generate 32 random bytes for the verifier
        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();

        // Base64-URL encode to create verifier (43-128 characters)
        let verifier = URL_SAFE_NO_PAD.encode(&random_bytes);

        // Create challenge: SHA256(verifier) then base64-URL encode
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Self {
            verifier,
            challenge,
            method: "S256".to_string(),
        }
    }

    /// Verify that a verifier matches a challenge
    pub fn verify(verifier: &str, challenge: &str) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let computed_challenge = URL_SAFE_NO_PAD.encode(hash);
        computed_challenge == challenge
    }
}

impl Default for PkceChallenge {
    fn default() -> Self {
        Self::generate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let pkce = PkceChallenge::generate();

        // Verifier should be at least 43 characters (256 bits base64)
        assert!(pkce.verifier.len() >= 43);

        // Challenge should be 43 characters (256 bits / 6 bits per char)
        assert_eq!(pkce.challenge.len(), 43);

        // Method should be S256
        assert_eq!(pkce.method, "S256");
    }

    #[test]
    fn test_pkce_verification() {
        let pkce = PkceChallenge::generate();

        // Should verify correctly
        assert!(PkceChallenge::verify(&pkce.verifier, &pkce.challenge));

        // Should fail with wrong verifier
        assert!(!PkceChallenge::verify("wrong_verifier", &pkce.challenge));
    }

    #[test]
    fn test_pkce_uniqueness() {
        let pkce1 = PkceChallenge::generate();
        let pkce2 = PkceChallenge::generate();

        // Each generation should be unique
        assert_ne!(pkce1.verifier, pkce2.verifier);
        assert_ne!(pkce1.challenge, pkce2.challenge);
    }
}
