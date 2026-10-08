//! Field-level encryption for sensitive data.
//!
//! Uses AES-256-GCM for authenticated encryption of sensitive fields
//! like credentials and tokens before storing in the database.

use anyhow::{Context, Result};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};

/// Size of the encryption key (32 bytes = 256 bits).
pub const KEY_SIZE: usize = 32;

/// Size of the nonce (12 bytes for AES-GCM).
const NONCE_SIZE: usize = 12;

/// One-way fingerprint of a master key: HMAC-SHA256 keyed with the master
/// key over a fixed label, hex-encoded and truncated to 128 bits. Lets
/// McpMux remember *which* key its data is encrypted with without storing
/// anything that helps recover the key.
pub fn key_fingerprint(master_key: &[u8; KEY_SIZE]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, master_key);
    let tag = ring::hmac::sign(&key, b"mcpmux master key fingerprint v1");
    hex::encode(&tag.as_ref()[..16])
}

/// Prefix of ciphertexts bound to where they are stored. `:` never occurs in
/// the hex of legacy (unbound) ciphertexts.
const BOUND_PREFIX: &str = "v2:";

/// The associated-data context of each encrypted column: what a stored
/// ciphertext is bound to.
pub mod binding {
    /// `credentials.credential_value` (unique per Space, server and type).
    pub fn credential(space_id: &str, server_id: &str, credential_type: &str) -> String {
        format!("credentials|credential_value|{space_id}|{server_id}|{credential_type}")
    }

    /// A settings column of `installed_servers` (`input_values`,
    /// `env_overrides`, `args_append`, `extra_headers`) for row `id`.
    pub fn installed_server(column: &str, id: &str) -> String {
        format!("installed_servers|{column}|{id}")
    }

    /// `outbound_oauth_clients.client_secret_encrypted` for a Space's server.
    pub fn outbound_client_secret(space_id: &str, server_id: &str) -> String {
        format!("outbound_oauth_clients|client_secret|{space_id}|{server_id}")
    }
}

/// Encryptor for sensitive field data.
pub struct FieldEncryptor {
    key: LessSafeKey,
    rng: SystemRandom,
}

impl FieldEncryptor {
    /// Create a new encryptor with the given master key.
    ///
    /// The key must be exactly 32 bytes (256 bits).
    pub fn new(master_key: &[u8; KEY_SIZE]) -> Result<Self> {
        let unbound_key = UnboundKey::new(&AES_256_GCM, master_key)
            .map_err(|_| anyhow::anyhow!("Failed to create encryption key"))?;
        let key = LessSafeKey::new(unbound_key);
        let rng = SystemRandom::new();

        Ok(Self { key, rng })
    }

    /// Encrypt a plaintext string.
    ///
    /// Returns the ciphertext as a hex-encoded string (nonce + ciphertext + tag).
    /// Not bound to where it is stored; stored values use [`Self::encrypt_bound`].
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        self.seal(plaintext, Aad::empty()).map(hex::encode)
    }

    /// Encrypt a value bound to where it is stored (`context`, see
    /// [`binding`]): the context is AES-GCM associated data, so the result
    /// only decrypts with the same context. A ciphertext copied into another
    /// row, column or Space fails to decrypt instead of being accepted.
    /// Format: `v2:` + hex(nonce + ciphertext + tag).
    pub fn encrypt_bound(&self, plaintext: &str, context: &str) -> Result<String> {
        let sealed = self.seal(plaintext, Aad::from(context.as_bytes()))?;
        Ok(format!("{BOUND_PREFIX}{}", hex::encode(sealed)))
    }

    /// Decrypt a stored value: a bound (`v2:`) ciphertext with `context`,
    /// or a legacy unbound one as before.
    pub fn decrypt_bound(&self, stored: &str, context: &str) -> Result<String> {
        match stored.strip_prefix(BOUND_PREFIX) {
            Some(hex_ct) => {
                let ciphertext = hex::decode(hex_ct).context("Invalid hex encoding")?;
                self.open(&ciphertext, Aad::from(context.as_bytes()))
            }
            None => self.decrypt(stored),
        }
    }

    /// Whether a stored value is already bound (`v2:`).
    pub fn is_bound(stored: &str) -> bool {
        stored.starts_with(BOUND_PREFIX)
    }

    fn seal<A: AsRef<[u8]>>(&self, plaintext: &str, aad: Aad<A>) -> Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|_| anyhow::anyhow!("Failed to generate nonce"))?;

        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        // Encrypt in-place
        let mut in_out = plaintext.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(nonce, aad, &mut in_out)
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;

        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&in_out);

        Ok(result)
    }

    /// Decrypt a hex-encoded ciphertext string.
    ///
    /// Expects format: hex(nonce + ciphertext + tag)
    pub fn decrypt(&self, ciphertext_hex: &str) -> Result<String> {
        let ciphertext = hex::decode(ciphertext_hex).context("Invalid hex encoding")?;
        self.open(&ciphertext, Aad::empty())
    }

    fn open<A: AsRef<[u8]>>(&self, ciphertext: &[u8], aad: Aad<A>) -> Result<String> {
        if ciphertext.len() < NONCE_SIZE + AES_256_GCM.tag_len() {
            anyhow::bail!("Ciphertext too short");
        }

        // Extract nonce and ciphertext
        let (nonce_bytes, encrypted) = ciphertext.split_at(NONCE_SIZE);
        let nonce_array: [u8; NONCE_SIZE] = nonce_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid nonce"))?;
        let nonce = Nonce::assume_unique_for_key(nonce_array);

        // Decrypt in-place
        let mut in_out = encrypted.to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce, aad, &mut in_out)
            .map_err(|_| anyhow::anyhow!("Decryption failed - wrong key or corrupted data"))?;

        String::from_utf8(plaintext.to_vec()).context("Decrypted data is not valid UTF-8")
    }
}

/// Generate a random master key.
pub fn generate_master_key() -> Result<[u8; KEY_SIZE]> {
    let rng = SystemRandom::new();
    let mut key = [0u8; KEY_SIZE];
    rng.fill(&mut key)
        .map_err(|_| anyhow::anyhow!("Failed to generate random key"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_ciphertexts_only_decrypt_in_their_own_place() {
        let enc = FieldEncryptor::new(&generate_master_key().unwrap()).unwrap();
        let here = binding::credential("space-a", "github", "access_token");
        let there = binding::credential("space-b", "github", "access_token");
        let stored = enc.encrypt_bound("ghp_token", &here).unwrap();
        assert!(FieldEncryptor::is_bound(&stored));
        assert_eq!(enc.decrypt_bound(&stored, &here).unwrap(), "ghp_token");
        assert!(enc.decrypt_bound(&stored, &there).is_err());
        // Legacy (unbound) values still read through the same call.
        let legacy = enc.encrypt("old").unwrap();
        assert!(!FieldEncryptor::is_bound(&legacy));
        assert_eq!(enc.decrypt_bound(&legacy, &there).unwrap(), "old");
    }

    #[test]
    fn test_encrypt_decrypt() {
        let key = generate_master_key().unwrap();
        let encryptor = FieldEncryptor::new(&key).unwrap();

        let plaintext = "my-secret-token-12345";
        let ciphertext = encryptor.encrypt(plaintext).unwrap();

        // Ciphertext should be hex-encoded
        assert!(hex::decode(&ciphertext).is_ok());

        // Ciphertext should be different from plaintext
        assert_ne!(ciphertext, plaintext);

        // Decrypt should return original
        let decrypted = encryptor.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = generate_master_key().unwrap();
        let key2 = generate_master_key().unwrap();

        let encryptor1 = FieldEncryptor::new(&key1).unwrap();
        let encryptor2 = FieldEncryptor::new(&key2).unwrap();

        let plaintext = "secret";
        let ciphertext = encryptor1.encrypt(plaintext).unwrap();

        // Should fail with wrong key
        let result = encryptor2.decrypt(&ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_different_nonces() {
        let key = generate_master_key().unwrap();
        let encryptor = FieldEncryptor::new(&key).unwrap();

        let plaintext = "same-data";
        let ciphertext1 = encryptor.encrypt(plaintext).unwrap();
        let ciphertext2 = encryptor.encrypt(plaintext).unwrap();

        // Same plaintext should produce different ciphertexts (due to random nonce)
        assert_ne!(ciphertext1, ciphertext2);

        // Both should decrypt to the same value
        assert_eq!(encryptor.decrypt(&ciphertext1).unwrap(), plaintext);
        assert_eq!(encryptor.decrypt(&ciphertext2).unwrap(), plaintext);
    }
}
