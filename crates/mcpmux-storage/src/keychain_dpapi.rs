//! DPAPI-based key storage for Windows.
//!
//! Stores encryption keys as DPAPI-protected files instead of Windows Credential Manager.
//! This prevents the master key from being visible in the Credential Manager UI while
//! maintaining the same security guarantees (user-scope DPAPI protection).
//!
//! Key files are stored in `<data_dir>/keys/` as opaque encrypted blobs.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use windows_dpapi::{decrypt_data, encrypt_data, Scope};
use zeroize::Zeroizing;

use crate::crypto::{generate_master_key, KEY_SIZE};
use crate::keychain::{generate_jwt_secret, JwtSecretProvider, MasterKeyProvider, JWT_SECRET_SIZE};

/// File name for the DPAPI-protected master encryption key.
const MASTER_KEY_FILE: &str = "master.dpapi";

/// File name for the DPAPI-protected JWT signing secret.
const JWT_SECRET_FILE: &str = "jwt.dpapi";

/// DPAPI-based master key provider.
///
/// Stores the master key in a DPAPI-protected file within the app's data directory.
/// The key is encrypted with user-scope DPAPI, meaning only the current Windows user
/// on this machine can decrypt it.
pub struct DpapiKeyProvider {
    key_path: PathBuf,
}

impl DpapiKeyProvider {
    /// Create a new DPAPI key provider that stores keys in the given data directory.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let keys_dir = data_dir.join("keys");
        fs::create_dir_all(&keys_dir)
            .with_context(|| format!("Failed to create keys directory: {:?}", keys_dir))?;

        Ok(Self {
            key_path: keys_dir.join(MASTER_KEY_FILE),
        })
    }
}

impl MasterKeyProvider for DpapiKeyProvider {
    fn get_or_create_key(&self) -> Result<Zeroizing<[u8; KEY_SIZE]>> {
        if self.key_path.exists() {
            debug!(
                "Reading DPAPI-protected master key from {:?}",
                self.key_path
            );
            let encrypted = fs::read(&self.key_path)
                .with_context(|| format!("Failed to read key file: {:?}", self.key_path))?;

            let decrypted = decrypt_data(&encrypted, Scope::User)
                .context("Failed to decrypt master key with DPAPI")?;

            if decrypted.len() != KEY_SIZE {
                anyhow::bail!(
                    "Invalid key size in DPAPI file: expected {}, got {}",
                    KEY_SIZE,
                    decrypted.len()
                );
            }

            let mut key = Zeroizing::new([0u8; KEY_SIZE]);
            key.copy_from_slice(&decrypted);
            debug!("Master key loaded from DPAPI-protected file");
            Ok(key)
        } else {
            info!("No master key found, generating new DPAPI-protected key");
            let key = generate_master_key()?;

            let encrypted = encrypt_data(&key, Scope::User)
                .context("Failed to encrypt master key with DPAPI")?;

            fs::write(&self.key_path, &encrypted)
                .with_context(|| format!("Failed to write key file: {:?}", self.key_path))?;

            info!("Master key generated and stored as DPAPI-protected file");
            Ok(Zeroizing::new(key))
        }
    }

    fn key_exists(&self) -> bool {
        self.key_path.exists()
    }

    fn delete_key(&self) -> Result<()> {
        if self.key_path.exists() {
            fs::remove_file(&self.key_path)
                .with_context(|| format!("Failed to delete key file: {:?}", self.key_path))?;
            info!("Master key DPAPI file deleted");
        } else {
            debug!("No DPAPI key file to delete");
        }
        Ok(())
    }
}

/// DPAPI-based JWT signing secret provider.
///
/// Stores the JWT signing secret in a DPAPI-protected file.
pub struct DpapiJwtSecretProvider {
    secret_path: PathBuf,
}

impl DpapiJwtSecretProvider {
    /// Create a new DPAPI JWT secret provider that stores secrets in the given data directory.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let keys_dir = data_dir.join("keys");
        fs::create_dir_all(&keys_dir)
            .with_context(|| format!("Failed to create keys directory: {:?}", keys_dir))?;

        Ok(Self {
            secret_path: keys_dir.join(JWT_SECRET_FILE),
        })
    }
}

impl JwtSecretProvider for DpapiJwtSecretProvider {
    fn get_or_create_secret(&self) -> Result<Zeroizing<[u8; JWT_SECRET_SIZE]>> {
        if self.secret_path.exists() {
            debug!(
                "Reading DPAPI-protected JWT secret from {:?}",
                self.secret_path
            );
            let encrypted = fs::read(&self.secret_path).with_context(|| {
                format!("Failed to read JWT secret file: {:?}", self.secret_path)
            })?;

            let decrypted = decrypt_data(&encrypted, Scope::User)
                .context("Failed to decrypt JWT secret with DPAPI")?;

            if decrypted.len() != JWT_SECRET_SIZE {
                anyhow::bail!(
                    "Invalid JWT secret size in DPAPI file: expected {}, got {}",
                    JWT_SECRET_SIZE,
                    decrypted.len()
                );
            }

            let mut secret = Zeroizing::new([0u8; JWT_SECRET_SIZE]);
            secret.copy_from_slice(&decrypted);
            debug!("JWT secret loaded from DPAPI-protected file");
            Ok(secret)
        } else {
            info!("No JWT secret found, generating new DPAPI-protected secret");
            let secret = generate_jwt_secret()?;

            let encrypted = encrypt_data(&secret, Scope::User)
                .context("Failed to encrypt JWT secret with DPAPI")?;

            fs::write(&self.secret_path, &encrypted).with_context(|| {
                format!("Failed to write JWT secret file: {:?}", self.secret_path)
            })?;

            info!("JWT secret generated and stored as DPAPI-protected file");
            Ok(Zeroizing::new(secret))
        }
    }

    fn secret_exists(&self) -> bool {
        self.secret_path.exists()
    }

    fn delete_secret(&self) -> Result<()> {
        if self.secret_path.exists() {
            fs::remove_file(&self.secret_path).with_context(|| {
                format!("Failed to delete JWT secret file: {:?}", self.secret_path)
            })?;
            info!("JWT secret DPAPI file deleted");
        } else {
            debug!("No DPAPI JWT secret file to delete");
        }
        Ok(())
    }
}

/// Migrate existing keys from Windows Credential Manager to DPAPI files.
///
/// If keys exist in Credential Manager but not as DPAPI files, this copies them
/// over and removes the Credential Manager entries. This is a one-time migration.
pub fn migrate_from_credential_manager(data_dir: &Path) -> Result<()> {
    use crate::keychain::{KeychainJwtSecretProvider, KeychainKeyProvider};

    let keys_dir = data_dir.join("keys");
    let master_dpapi_path = keys_dir.join(MASTER_KEY_FILE);
    let jwt_dpapi_path = keys_dir.join(JWT_SECRET_FILE);

    // Migrate master key if DPAPI file doesn't exist yet
    if !master_dpapi_path.exists() {
        if let Ok(keychain_provider) = KeychainKeyProvider::new() {
            if keychain_provider.key_exists() {
                info!("Migrating master key from Credential Manager to DPAPI");
                match keychain_provider.get_or_create_key() {
                    Ok(key) => {
                        let dpapi_provider = DpapiKeyProvider::new(data_dir)?;
                        // Write through DPAPI provider to ensure proper encryption
                        let encrypted = encrypt_data(&*key, Scope::User)
                            .context("Failed to encrypt master key with DPAPI during migration")?;
                        fs::create_dir_all(&keys_dir)?;
                        fs::write(&master_dpapi_path, &encrypted)?;

                        // Remove from Credential Manager
                        if let Err(e) = keychain_provider.delete_key() {
                            warn!("Failed to remove master key from Credential Manager after migration: {}", e);
                        } else {
                            info!("Master key migrated and removed from Credential Manager");
                        }
                        drop(dpapi_provider);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to read master key from Credential Manager for migration: {}",
                            e
                        );
                    }
                }
            }
        }
    }

    // Migrate JWT secret if DPAPI file doesn't exist yet
    if !jwt_dpapi_path.exists() {
        if let Ok(keychain_provider) = KeychainJwtSecretProvider::new() {
            if keychain_provider.secret_exists() {
                info!("Migrating JWT secret from Credential Manager to DPAPI");
                match keychain_provider.get_or_create_secret() {
                    Ok(secret) => {
                        let encrypted = encrypt_data(&*secret, Scope::User)
                            .context("Failed to encrypt JWT secret with DPAPI during migration")?;
                        fs::create_dir_all(&keys_dir)?;
                        fs::write(&jwt_dpapi_path, &encrypted)?;

                        // Remove from Credential Manager
                        if let Err(e) = keychain_provider.delete_secret() {
                            warn!("Failed to remove JWT secret from Credential Manager after migration: {}", e);
                        } else {
                            info!("JWT secret migrated and removed from Credential Manager");
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to read JWT secret from Credential Manager for migration: {}",
                            e
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpapi_master_key_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = DpapiKeyProvider::new(tmp.path()).unwrap();

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

    #[test]
    fn test_dpapi_jwt_secret_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = DpapiJwtSecretProvider::new(tmp.path()).unwrap();

        // Initially no secret
        assert!(!provider.secret_exists());

        // Get or create generates a secret
        let secret1 = provider.get_or_create_secret().unwrap();
        assert!(provider.secret_exists());

        // Getting again returns the same secret
        let secret2 = provider.get_or_create_secret().unwrap();
        assert_eq!(&*secret1, &*secret2);

        // Delete removes the secret
        provider.delete_secret().unwrap();
        assert!(!provider.secret_exists());
    }

    #[test]
    fn test_dpapi_file_is_encrypted() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = DpapiKeyProvider::new(tmp.path()).unwrap();

        let key = provider.get_or_create_key().unwrap();

        // Read the raw file - it should NOT contain the key in plaintext
        let file_contents = fs::read(tmp.path().join("keys").join(MASTER_KEY_FILE)).unwrap();

        // DPAPI output is always larger than the input (has header + IV + tag)
        assert!(file_contents.len() > KEY_SIZE);

        // The raw key bytes should not appear in the file
        assert!(!file_contents.windows(KEY_SIZE).any(|w| w == &*key));
    }
}
