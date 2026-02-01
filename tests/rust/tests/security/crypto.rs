//! Crypto integration tests
//!
//! These tests verify the encryption/decryption functionality.
//! Note: Unit tests already exist in mcpmux_storage::crypto::tests

use mcpmux_storage::{generate_master_key, FieldEncryptor, KEY_SIZE};
use pretty_assertions::assert_eq;

#[test]
fn test_generate_master_key() {
    let key1 = generate_master_key().expect("Failed to generate key");
    let key2 = generate_master_key().expect("Failed to generate key");

    // Keys should be different
    assert_ne!(key1, key2);

    // Keys should be correct size
    assert_eq!(key1.len(), KEY_SIZE);
    assert_eq!(key2.len(), KEY_SIZE);
}

#[test]
fn test_encrypt_decrypt_roundtrip() {
    let key = generate_master_key().expect("Failed to generate key");
    let encryptor = FieldEncryptor::new(&key).expect("Failed to create encryptor");

    let plaintext = "Hello, World! This is a secret message.";

    let ciphertext = encryptor.encrypt(plaintext).expect("Failed to encrypt");
    let decrypted = encryptor.decrypt(&ciphertext).expect("Failed to decrypt");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_encrypt_produces_different_ciphertext() {
    let key = generate_master_key().expect("Failed to generate key");
    let encryptor = FieldEncryptor::new(&key).expect("Failed to create encryptor");

    let plaintext = "Same message";

    // Encrypt the same message twice
    let ciphertext1 = encryptor.encrypt(plaintext).expect("Failed to encrypt");
    let ciphertext2 = encryptor.encrypt(plaintext).expect("Failed to encrypt");

    // Ciphertexts should be different (due to random nonce)
    assert_ne!(ciphertext1, ciphertext2);

    // But both should decrypt to the same plaintext
    let decrypted1 = encryptor.decrypt(&ciphertext1).expect("Failed to decrypt");
    let decrypted2 = encryptor.decrypt(&ciphertext2).expect("Failed to decrypt");
    assert_eq!(decrypted1, decrypted2);
    assert_eq!(decrypted1, plaintext);
}

#[test]
fn test_decrypt_with_wrong_key_fails() {
    let key1 = generate_master_key().expect("Failed to generate key");
    let key2 = generate_master_key().expect("Failed to generate key");

    let encryptor1 = FieldEncryptor::new(&key1).expect("Failed to create encryptor");
    let encryptor2 = FieldEncryptor::new(&key2).expect("Failed to create encryptor");

    let plaintext = "Secret data";

    let ciphertext = encryptor1.encrypt(plaintext).expect("Failed to encrypt");

    // Decrypting with wrong key should fail
    let result = encryptor2.decrypt(&ciphertext);
    assert!(result.is_err());
}

#[test]
fn test_encrypt_empty_string() {
    let key = generate_master_key().expect("Failed to generate key");
    let encryptor = FieldEncryptor::new(&key).expect("Failed to create encryptor");

    let plaintext = "";

    let ciphertext = encryptor
        .encrypt(plaintext)
        .expect("Failed to encrypt empty");
    let decrypted = encryptor
        .decrypt(&ciphertext)
        .expect("Failed to decrypt empty");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_encrypt_unicode() {
    let key = generate_master_key().expect("Failed to generate key");
    let encryptor = FieldEncryptor::new(&key).expect("Failed to create encryptor");

    let plaintext = "Hello 🔐 Emoji and 日本語 characters!";

    let ciphertext = encryptor
        .encrypt(plaintext)
        .expect("Failed to encrypt unicode");
    let decrypted = encryptor
        .decrypt(&ciphertext)
        .expect("Failed to decrypt unicode");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_encrypt_large_data() {
    let key = generate_master_key().expect("Failed to generate key");
    let encryptor = FieldEncryptor::new(&key).expect("Failed to create encryptor");

    // Create a large string (10KB)
    let plaintext: String = (0..10000)
        .map(|i| ((i % 26) as u8 + b'a') as char)
        .collect();

    let ciphertext = encryptor
        .encrypt(&plaintext)
        .expect("Failed to encrypt large");
    let decrypted = encryptor
        .decrypt(&ciphertext)
        .expect("Failed to decrypt large");

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_ciphertext_is_hex_encoded() {
    let key = generate_master_key().expect("Failed to generate key");
    let encryptor = FieldEncryptor::new(&key).expect("Failed to create encryptor");

    let plaintext = "test";
    let ciphertext = encryptor.encrypt(plaintext).expect("Failed to encrypt");

    // Should be valid hex
    assert!(hex::decode(&ciphertext).is_ok());

    // Should only contain hex characters
    assert!(ciphertext.chars().all(|c| c.is_ascii_hexdigit()));
}
