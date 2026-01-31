//! OS Keychain integration for master key storage.
//!
//! Uses the platform-native secure storage:
//! - Windows: Credential Manager
//! - macOS: Keychain
//! - Linux: Secret Service (GNOME Keyring, KWallet)

use anyhow::{Context, Result};
use keyring::Entry;
use mcpmux_core::branding;
use tracing::{debug, info, warn};
use zeroize::Zeroizing;

use crate::crypto::{generate_master_key, KEY_SIZE};

/// Key name for the master encryption key.
const MASTER_KEY_NAME: &str = "master-encryption-key";

/// Key name for the JWT signing secret.
const JWT_SIGNING_SECRET_NAME: &str = "jwt-signing-secret";

/// Trait for providing the master encryption key.
///
/// This abstraction allows for different key storage mechanisms:
/// - OS Keychain (Phase 1)
/// - Cloud-based encrypted key (Phase 3+)
pub trait MasterKeyProvider: Send + Sync {
    /// Get the master key, creating one if it doesn't exist.
    fn get_or_create_key(&self) -> Result<Zeroizing<[u8; KEY_SIZE]>>;

    /// Check if a master key exists.
    fn key_exists(&self) -> bool;

    /// Delete the master key (for testing or reset).
    fn delete_key(&self) -> Result<()>;
}

/// OS Keychain-based master key provider.
///
/// Stores the master key in the platform's native secure storage.
pub struct KeychainKeyProvider {
    entry: Entry,
}

impl KeychainKeyProvider {
    /// Create a new keychain key provider.
    pub fn new() -> Result<Self> {
        let entry = Entry::new(branding::KEYCHAIN_SERVICE, MASTER_KEY_NAME)
            .context("Failed to create keychain entry")?;

        Ok(Self { entry })
    }

    /// Create with a custom service and key name (for testing).
    #[cfg(test)]
    pub fn with_names(service: &str, key_name: &str) -> Result<Self> {
        let entry = Entry::new(service, key_name)
            .context("Failed to create keychain entry")?;

        Ok(Self { entry })
    }
}

impl MasterKeyProvider for KeychainKeyProvider {
    fn get_or_create_key(&self) -> Result<Zeroizing<[u8; KEY_SIZE]>> {
        // Try to get existing key
        match self.entry.get_password() {
            Ok(hex_key) => {
                debug!("Retrieved existing master key from keychain");
                let key_bytes = hex::decode(&hex_key)
                    .context("Invalid key format in keychain")?;

                if key_bytes.len() != KEY_SIZE {
                    anyhow::bail!(
                        "Invalid key size in keychain: expected {}, got {}",
                        KEY_SIZE,
                        key_bytes.len()
                    );
                }

                let mut key = Zeroizing::new([0u8; KEY_SIZE]);
                key.copy_from_slice(&key_bytes);
                Ok(key)
            }
            Err(keyring::Error::NoEntry) => {
                // No key exists, generate a new one
                info!("No master key found, generating new key");
                let key = generate_master_key()?;
                let hex_key = hex::encode(key);

                self.entry
                    .set_password(&hex_key)
                    .context("Failed to store master key in keychain")?;

                info!("Master key generated and stored in keychain");
                Ok(Zeroizing::new(key))
            }
            Err(e) => {
                warn!("Keychain error: {:?}", e);
                Err(anyhow::anyhow!("Failed to access keychain: {}", e))
            }
        }
    }

    fn key_exists(&self) -> bool {
        self.entry.get_password().is_ok()
    }

    fn delete_key(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) => {
                info!("Master key deleted from keychain");
                Ok(())
            }
            Err(keyring::Error::NoEntry) => {
                debug!("No key to delete");
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("Failed to delete key from keychain: {}", e)),
        }
    }
}

impl Default for KeychainKeyProvider {
    fn default() -> Self {
        Self::new().expect("Failed to create keychain provider")
    }
}

// ============================================================================
// JWT Signing Secret Provider
// ============================================================================

/// Size of the JWT signing secret (32 bytes = 256 bits for HS256).
pub const JWT_SECRET_SIZE: usize = 32;

/// Trait for providing the JWT signing secret.
///
/// The JWT signing secret is used for:
/// - Signing access tokens issued by the gateway
/// - Verifying access tokens on protected endpoints
pub trait JwtSecretProvider: Send + Sync {
    /// Get the JWT signing secret, creating one if it doesn't exist.
    fn get_or_create_secret(&self) -> Result<Zeroizing<[u8; JWT_SECRET_SIZE]>>;

    /// Check if a JWT signing secret exists.
    fn secret_exists(&self) -> bool;

    /// Delete the JWT signing secret (for testing or reset).
    fn delete_secret(&self) -> Result<()>;
}

/// OS Keychain-based JWT signing secret provider.
///
/// Stores the JWT signing secret in the platform's native secure storage.
pub struct KeychainJwtSecretProvider {
    entry: Entry,
}

impl KeychainJwtSecretProvider {
    /// Create a new keychain JWT secret provider.
    pub fn new() -> Result<Self> {
        let entry = Entry::new(branding::KEYCHAIN_SERVICE, JWT_SIGNING_SECRET_NAME)
            .context("Failed to create keychain entry for JWT secret")?;

        Ok(Self { entry })
    }

    /// Create with a custom service and key name (for testing).
    #[cfg(test)]
    pub fn with_names(service: &str, key_name: &str) -> Result<Self> {
        let entry = Entry::new(service, key_name)
            .context("Failed to create keychain entry")?;

        Ok(Self { entry })
    }
}

impl JwtSecretProvider for KeychainJwtSecretProvider {
    fn get_or_create_secret(&self) -> Result<Zeroizing<[u8; JWT_SECRET_SIZE]>> {
        debug!("[Keychain] Attempting to retrieve JWT signing secret from keychain");
        
        // Try to get existing secret
        match self.entry.get_password() {
            Ok(hex_secret) => {
                info!("[Keychain] Retrieved existing JWT signing secret (len={})", hex_secret.len());
                let secret_bytes = hex::decode(&hex_secret)
                    .context("Invalid JWT secret format in keychain")?;

                if secret_bytes.len() != JWT_SECRET_SIZE {
                    anyhow::bail!(
                        "Invalid JWT secret size in keychain: expected {}, got {}",
                        JWT_SECRET_SIZE,
                        secret_bytes.len()
                    );
                }

                let mut secret = Zeroizing::new([0u8; JWT_SECRET_SIZE]);
                secret.copy_from_slice(&secret_bytes);
                Ok(secret)
            }
            Err(keyring::Error::NoEntry) => {
                // No secret exists, generate a new one
                info!("[Keychain] No JWT signing secret found, generating new secret");
                let secret = generate_jwt_secret()?;
                let hex_secret = hex::encode(secret);
                
                debug!("[Keychain] Storing JWT secret (hex len={})", hex_secret.len());

                match self.entry.set_password(&hex_secret) {
                    Ok(()) => {
                        info!("[Keychain] JWT signing secret generated and stored in keychain");
                    }
                    Err(e) => {
                        warn!("[Keychain] Failed to store JWT secret: {:?}", e);
                        return Err(anyhow::anyhow!("Failed to store JWT signing secret in keychain: {}", e));
                    }
                }

                Ok(Zeroizing::new(secret))
            }
            Err(e) => {
                warn!("[Keychain] Error accessing JWT secret: {:?}", e);
                Err(anyhow::anyhow!("Failed to access keychain for JWT secret: {}", e))
            }
        }
    }

    fn secret_exists(&self) -> bool {
        self.entry.get_password().is_ok()
    }

    fn delete_secret(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) => {
                info!("[Keychain] JWT signing secret deleted from keychain");
                Ok(())
            }
            Err(keyring::Error::NoEntry) => {
                debug!("[Keychain] No JWT secret to delete");
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("Failed to delete JWT secret from keychain: {}", e)),
        }
    }
}

impl Default for KeychainJwtSecretProvider {
    fn default() -> Self {
        Self::new().expect("Failed to create JWT secret provider")
    }
}

/// Generate a random JWT signing secret.
pub fn generate_jwt_secret() -> Result<[u8; JWT_SECRET_SIZE]> {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut secret = [0u8; JWT_SECRET_SIZE];
    rng.fill(&mut secret)
        .map_err(|_| anyhow::anyhow!("Failed to generate random JWT secret"))?;
    Ok(secret)
}

/// In-memory JWT secret provider for testing.
#[cfg(test)]
pub struct MemoryJwtSecretProvider {
    secret: std::sync::Mutex<Option<[u8; JWT_SECRET_SIZE]>>,
}

#[cfg(test)]
impl Default for MemoryJwtSecretProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MemoryJwtSecretProvider {
    pub fn new() -> Self {
        Self {
            secret: std::sync::Mutex::new(None),
        }
    }

    pub fn with_secret(secret: [u8; JWT_SECRET_SIZE]) -> Self {
        Self {
            secret: std::sync::Mutex::new(Some(secret)),
        }
    }
}

#[cfg(test)]
impl JwtSecretProvider for MemoryJwtSecretProvider {
    fn get_or_create_secret(&self) -> Result<Zeroizing<[u8; JWT_SECRET_SIZE]>> {
        let mut guard = self.secret.lock().unwrap();
        if let Some(secret) = *guard {
            Ok(Zeroizing::new(secret))
        } else {
            let secret = generate_jwt_secret()?;
            *guard = Some(secret);
            Ok(Zeroizing::new(secret))
        }
    }

    fn secret_exists(&self) -> bool {
        self.secret.lock().unwrap().is_some()
    }

    fn delete_secret(&self) -> Result<()> {
        *self.secret.lock().unwrap() = None;
        Ok(())
    }
}

/// In-memory key provider for testing.
#[cfg(test)]
pub struct MemoryKeyProvider {
    key: std::sync::Mutex<Option<[u8; KEY_SIZE]>>,
}

#[cfg(test)]
impl Default for MemoryKeyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MemoryKeyProvider {
    pub fn new() -> Self {
        Self {
            key: std::sync::Mutex::new(None),
        }
    }

    pub fn with_key(key: [u8; KEY_SIZE]) -> Self {
        Self {
            key: std::sync::Mutex::new(Some(key)),
        }
    }
}

#[cfg(test)]
impl MasterKeyProvider for MemoryKeyProvider {
    fn get_or_create_key(&self) -> Result<Zeroizing<[u8; KEY_SIZE]>> {
        let mut guard = self.key.lock().unwrap();
        if let Some(key) = *guard {
            Ok(Zeroizing::new(key))
        } else {
            let key = generate_master_key()?;
            *guard = Some(key);
            Ok(Zeroizing::new(key))
        }
    }

    fn key_exists(&self) -> bool {
        self.key.lock().unwrap().is_some()
    }

    fn delete_key(&self) -> Result<()> {
        *self.key.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_provider() {
        let provider = MemoryKeyProvider::new();

        // Initially no key
        assert!(!provider.key_exists());

        // Get or create generates a key
        let key1 = provider.get_or_create_key().unwrap();
        assert!(provider.key_exists());

        // Getting again returns the same key
        let key2 = provider.get_or_create_key().unwrap();
        assert_eq!(&*key1, &*key2);

        // Delete removes the key
        provider.delete_key().unwrap();
        assert!(!provider.key_exists());

        // New key is generated after delete
        let key3 = provider.get_or_create_key().unwrap();
        assert_ne!(&*key1, &*key3);
    }

    // Note: Keychain tests are integration tests that require the OS keychain
    // They should be run manually or in CI with proper setup
    #[test]
    #[ignore] // Run with: cargo test -- --ignored
    fn test_keychain_provider() {
        let provider = KeychainKeyProvider::with_names("com.mcpmux.test", "test-key").unwrap();

        // Clean up any previous test runs
        let _ = provider.delete_key();

        // Get or create generates a key
        let key1 = provider.get_or_create_key().unwrap();
        assert!(provider.key_exists());

        // Getting again returns the same key
        let key2 = provider.get_or_create_key().unwrap();
        assert_eq!(&*key1, &*key2);

        // Clean up
        provider.delete_key().unwrap();
    }
}

