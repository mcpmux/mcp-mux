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
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|_| anyhow::anyhow!("Failed to generate nonce"))?;

        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        // Encrypt in-place
        let mut in_out = plaintext.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;

        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&in_out);

        Ok(hex::encode(result))
    }

    /// Decrypt a hex-encoded ciphertext string.
    ///
    /// Expects format: hex(nonce + ciphertext + tag)
    pub fn decrypt(&self, ciphertext_hex: &str) -> Result<String> {
        let ciphertext = hex::decode(ciphertext_hex).context("Invalid hex encoding")?;

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
            .open_in_place(nonce, Aad::empty(), &mut in_out)
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
