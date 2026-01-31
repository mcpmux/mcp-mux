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
//! │          KeychainKeyProvider                         │
//! │     (OS Keychain: Windows/macOS/Linux)               │
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
//!     FieldEncryptor, KeychainKeyProvider, MasterKeyProvider,
//! };
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//!
//! // Get master key from OS keychain
//! let key_provider = KeychainKeyProvider::new()?;
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
mod repositories;

pub use crypto::{generate_master_key, FieldEncryptor, KEY_SIZE};
pub use database::Database;
pub use keychain::{
    KeychainKeyProvider, MasterKeyProvider,
    KeychainJwtSecretProvider, JwtSecretProvider, JWT_SECRET_SIZE, generate_jwt_secret,
};
pub use repositories::*;

/// Default database file name.
pub const DATABASE_FILE: &str = "mcpmux.db";

/// Get the default database path for the current platform.
pub fn default_database_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|p| p.join("mcpmux").join(DATABASE_FILE))
}
