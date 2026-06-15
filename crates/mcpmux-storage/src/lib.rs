//! McpMux Storage Layer
//!
//! SQLite database with field-level encryption for sensitive data.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │                    Application                       │
//! ├──────────────────────────────────────────────────────┤
//! │               Repository Traits                      │
//! │        (SpaceRepository, CredentialRepository, etc.) │
//! ├──────────────────────────────────────────────────────┤
//! │            SQLite Implementations                    │
//! │    (SqliteSpaceRepository, SqliteCredentialRepo)     │
//! ├──────────────────────────────────────────────────────┤
//! │         FieldEncryptor (AES-256-GCM)                 │
//! │        (Encrypts tokens/credentials)                 │
//! ├──────────────────────────────────────────────────────┤
//! │    DpapiKeyProvider (Windows) / KeychainKeyProvider   │
//! │    (DPAPI file storage / OS Keychain)                 │
//! ├──────────────────────────────────────────────────────┤
//! │                   Database                           │
//! │                   (SQLite)                           │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use mcpmux_storage::{
//!     Database, SqliteSpaceRepository, SqliteCredentialRepository,
//!     FieldEncryptor, MasterKeyProvider,
//! };
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//!
//! // Get master key (DPAPI on Windows, OS Keychain elsewhere)
//! let key_provider = mcpmux_storage::create_key_provider(&data_dir)?;
//! let master_key = key_provider.get_or_create_key()?;
//!
//! // Open database
//! let db = Database::open(&path)?;
//! let db = Arc::new(Mutex::new(db));
//!
//! // Create encryptor for sensitive fields
//! let encryptor = Arc::new(FieldEncryptor::new(&master_key)?);
//!
//! // Create repositories
//! let space_repo = SqliteSpaceRepository::new(db.clone());
//! let credential_repo = SqliteCredentialRepository::new(db.clone(), encryptor);
//! ```

pub mod crypto;
mod database;
pub mod keychain;
#[cfg(windows)]
pub mod keychain_dpapi;
#[cfg(not(windows))]
pub mod keychain_file;
mod repositories;

pub use crypto::{generate_master_key, FieldEncryptor, KEY_SIZE};
pub use database::Database;
pub use keychain::{
    generate_jwt_secret, JwtSecretProvider, KeychainJwtSecretProvider, KeychainKeyProvider,
    MasterKeyProvider, JWT_SECRET_SIZE,
};
#[cfg(windows)]
pub use keychain_dpapi::{DpapiJwtSecretProvider, DpapiKeyProvider};
#[cfg(not(windows))]
pub use keychain_file::{FileJwtSecretProvider, FileKeyProvider};
pub use repositories::*;

/// Default database file name.
pub const DATABASE_FILE: &str = "mcpmux.db";

/// Get the default database path for the current platform.
pub fn default_database_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|p| p.join("mcpmux").join(DATABASE_FILE))
}

/// Create the platform-appropriate master key provider.
///
/// - **Windows**: Uses DPAPI file-based storage (key not visible in Credential Manager UI).
///   Also migrates existing keys from Credential Manager on first use.
/// - **macOS/Linux**: Uses the OS keychain (Keychain / Secret Service).
pub fn create_key_provider(
    data_dir: &std::path::Path,
) -> anyhow::Result<Box<dyn MasterKeyProvider>> {
    #[cfg(windows)]
    {
        // Migrate any existing keys from Credential Manager to DPAPI files
        if let Err(e) = keychain_dpapi::migrate_from_credential_manager(data_dir) {
            tracing::warn!("Credential Manager migration encountered an error: {}", e);
        }
        Ok(Box::new(DpapiKeyProvider::new(data_dir)?))
    }

    #[cfg(not(windows))]
    {
        // Try OS keychain first, fall back to file-based storage if unavailable
        match KeychainKeyProvider::new() {
            Ok(provider) => match provider.get_or_create_key() {
                Ok(_) => return Ok(Box::new(provider)),
                Err(e) => tracing::warn!(
                    "OS keychain unavailable ({e}), using file-based key storage. \
                     For better security, install gnome-keyring or another Secret Service provider."
                ),
            },
            Err(e) => {
                tracing::warn!("OS keychain unavailable ({e}), using file-based key storage.")
            }
        }
        Ok(Box::new(FileKeyProvider::new(data_dir)?))
    }
}

/// Create the platform-appropriate JWT secret provider.
///
/// - **Windows**: Uses DPAPI file-based storage.
/// - **macOS/Linux**: Uses the OS keychain, with file-based fallback if unavailable.
pub fn create_jwt_secret_provider(
    data_dir: &std::path::Path,
) -> anyhow::Result<Box<dyn JwtSecretProvider>> {
    #[cfg(windows)]
    {
        Ok(Box::new(DpapiJwtSecretProvider::new(data_dir)?))
    }

    #[cfg(not(windows))]
    {
        match KeychainJwtSecretProvider::new() {
            Ok(provider) => match provider.get_or_create_secret() {
                Ok(_) => return Ok(Box::new(provider)),
                Err(e) => tracing::warn!(
                    "OS keychain unavailable for JWT secret ({e}), using file-based storage."
                ),
            },
            Err(e) => tracing::warn!(
                "OS keychain unavailable for JWT secret ({e}), using file-based storage."
            ),
        }
        Ok(Box::new(FileJwtSecretProvider::new(data_dir)?))
    }
}
