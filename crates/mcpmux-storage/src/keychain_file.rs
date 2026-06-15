//! File-based key storage fallback for environments without an OS keychain.
//!
//! Used on Linux/macOS when Secret Service or Keychain is unavailable (headless servers,
//! WSL, minimal desktop environments). Keys are stored as raw bytes in files with
//! restrictive permissions (0600 on Unix), similar to how SSH protects `~/.ssh/` keys.
//!
//! This is less secure than OS keychain or DPAPI — any process running as the same user
//! can read the key files. For production deployments, install `gnome-keyring` or another
//! Secret Service provider.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info};
use zeroize::Zeroizing;

use crate::crypto::{generate_master_key, KEY_SIZE};
use crate::keychain::{generate_jwt_secret, JwtSecretProvider, MasterKeyProvider, JWT_SECRET_SIZE};

/// File name for the master encryption key.
const MASTER_KEY_FILE: &str = "master.key";

/// File name for the JWT signing secret.
const JWT_SECRET_FILE: &str = "jwt.key";

/// Set restrictive file permissions (owner read/write only).
fn set_owner_only_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to set permissions on {:?}", path))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Write data to a file with restrictive permissions.
fn write_key_file(path: &Path, data: &[u8]) -> Result<()> {
    fs::write(path, data).with_context(|| format!("Failed to write key file: {:?}", path))?;
    set_owner_only_permissions(path)?;
    Ok(())
}

/// File-based master key provider.
///
/// Stores the master key as a raw byte file protected by filesystem permissions.
pub struct FileKeyProvider {
    key_path: PathBuf,
}

impl FileKeyProvider {
    /// Create a new file key provider that stores keys in `<data_dir>/keys/`.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let keys_dir = data_dir.join("keys");
        fs::create_dir_all(&keys_dir)
            .with_context(|| format!("Failed to create keys directory: {:?}", keys_dir))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&keys_dir, fs::Permissions::from_mode(0o700))?;
        }

        Ok(Self {
            key_path: keys_dir.join(MASTER_KEY_FILE),
        })
    }
}

impl MasterKeyProvider for FileKeyProvider {
    fn get_or_create_key(&self) -> Result<Zeroizing<[u8; KEY_SIZE]>> {
        if self.key_path.exists() {
            debug!("Reading master key from {:?}", self.key_path);
            let data = fs::read(&self.key_path)
                .with_context(|| format!("Failed to read key file: {:?}", self.key_path))?;

            if data.len() != KEY_SIZE {
                anyhow::bail!(
                    "Invalid key size in file: expected {}, got {}",
                    KEY_SIZE,
                    data.len()
                );
            }

            let mut key = Zeroizing::new([0u8; KEY_SIZE]);
            key.copy_from_slice(&data);
            debug!("Master key loaded from file");
            Ok(key)
        } else {
            info!("No master key found, generating new file-based key");
            let key = generate_master_key()?;
            write_key_file(&self.key_path, &key)?;
            info!("Master key generated and stored in {:?}", self.key_path);
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
            info!("Master key file deleted");
        } else {
            debug!("No key file to delete");
        }
        Ok(())
    }
}

/// File-based JWT signing secret provider.
///
/// Stores the JWT signing secret as a raw byte file protected by filesystem permissions.
pub struct FileJwtSecretProvider {
    secret_path: PathBuf,
}

impl FileJwtSecretProvider {
    /// Create a new file JWT secret provider that stores secrets in `<data_dir>/keys/`.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let keys_dir = data_dir.join("keys");
        fs::create_dir_all(&keys_dir)
            .with_context(|| format!("Failed to create keys directory: {:?}", keys_dir))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&keys_dir, fs::Permissions::from_mode(0o700))?;
        }

        Ok(Self {
            secret_path: keys_dir.join(JWT_SECRET_FILE),
        })
    }
}

impl JwtSecretProvider for FileJwtSecretProvider {
    fn get_or_create_secret(&self) -> Result<Zeroizing<[u8; JWT_SECRET_SIZE]>> {
        if self.secret_path.exists() {
            debug!("Reading JWT secret from {:?}", self.secret_path);
            let data = fs::read(&self.secret_path).with_context(|| {
                format!("Failed to read JWT secret file: {:?}", self.secret_path)
            })?;

            if data.len() != JWT_SECRET_SIZE {
                anyhow::bail!(
                    "Invalid JWT secret size in file: expected {}, got {}",
                    JWT_SECRET_SIZE,
                    data.len()
                );
            }

            let mut secret = Zeroizing::new([0u8; JWT_SECRET_SIZE]);
            secret.copy_from_slice(&data);
            debug!("JWT secret loaded from file");
            Ok(secret)
        } else {
            info!("No JWT secret found, generating new file-based secret");
            let secret = generate_jwt_secret()?;
            write_key_file(&self.secret_path, &secret)?;
            info!("JWT secret generated and stored in {:?}", self.secret_path);
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
            info!("JWT secret file deleted");
        } else {
            debug!("No JWT secret file to delete");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_master_key_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = FileKeyProvider::new(tmp.path()).unwrap();

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
    fn test_file_jwt_secret_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = FileJwtSecretProvider::new(tmp.path()).unwrap();

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
    fn test_file_key_is_correct_size() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = FileKeyProvider::new(tmp.path()).unwrap();

        provider.get_or_create_key().unwrap();

        let file_contents = fs::read(tmp.path().join("keys").join(MASTER_KEY_FILE)).unwrap();
        assert_eq!(file_contents.len(), KEY_SIZE);
    }
}
