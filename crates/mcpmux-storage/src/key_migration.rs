//! Re-encrypt data that was written under the file-based fallback key.
//!
//! On macOS/Linux, `create_key_provider` can briefly fall back to `keys/master.key`
//! when the OS keychain prompt is dismissed. Credentials encrypted during that
//! window cannot be read once the keychain key is used again. This module detects
//! those rows and re-encrypts them with the active key.

use std::path::Path;

#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;
#[cfg(not(windows))]
use tracing::info;

use crate::crypto::FieldEncryptor;
#[cfg(not(windows))]
use crate::keychain::MasterKeyProvider;
#[cfg(not(windows))]
use crate::keychain_file::FileKeyProvider;
use crate::Database;

/// Re-encrypt credential and installed-server fields that were encrypted with the
/// legacy file fallback key so they work under the active keychain key.
#[cfg(not(windows))]
pub fn migrate_file_key_encrypted_fields(
    db: &Database,
    data_dir: &Path,
    active_encryptor: &FieldEncryptor,
) -> Result<u32> {
    let file_provider = FileKeyProvider::new(data_dir)?;
    if !file_provider.key_exists() {
        return Ok(0);
    }

    let file_key = file_provider.get_or_create_key()?;
    let file_encryptor = FieldEncryptor::new(&file_key)?;

    let mut migrated = 0u32;
    migrated += migrate_credentials(db, active_encryptor, &file_encryptor)?;
    migrated += migrate_installed_server_inputs(db, active_encryptor, &file_encryptor)?;

    if migrated > 0 {
        info!(
            "Re-encrypted {} credential/input field(s) from file fallback key to active master key",
            migrated
        );
    }

    Ok(migrated)
}

/// Windows uses DPAPI file storage only; file-keychain fallback migration is Unix-only.
#[cfg(windows)]
pub fn migrate_file_key_encrypted_fields(
    _db: &Database,
    _data_dir: &Path,
    _active_encryptor: &FieldEncryptor,
) -> Result<u32> {
    Ok(0)
}

/// Migrate encrypted credential values.
#[cfg(not(windows))]
fn migrate_credentials(
    db: &Database,
    active: &FieldEncryptor,
    file: &FieldEncryptor,
) -> Result<u32> {
    let conn = db.connection();
    let mut stmt = conn.prepare(
        "SELECT rowid, credential_value FROM credentials WHERE credential_value IS NOT NULL",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut migrated = 0u32;
    for (rowid, value) in rows {
        if active.decrypt(&value).is_ok() {
            continue;
        }
        let plaintext = file
            .decrypt(&value)
            .with_context(|| format!("credential rowid={rowid} is not readable with either key"))?;
        let reencrypted = active
            .encrypt(&plaintext)
            .context("failed to re-encrypt credential with active key")?;
        conn.execute(
            "UPDATE credentials SET credential_value = ?1 WHERE rowid = ?2",
            rusqlite::params![reencrypted, rowid],
        )?;
        migrated += 1;
    }
    Ok(migrated)
}

/// Migrate encrypted installed-server input_values blobs.
#[cfg(not(windows))]
fn migrate_installed_server_inputs(
    db: &Database,
    active: &FieldEncryptor,
    file: &FieldEncryptor,
) -> Result<u32> {
    let conn = db.connection();
    let mut stmt = conn.prepare(
        "SELECT rowid, input_values FROM installed_servers WHERE input_values IS NOT NULL AND input_values != ''",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut migrated = 0u32;
    for (rowid, value) in rows {
        if active.decrypt(&value).is_ok() {
            continue;
        }
        let plaintext = match file.decrypt(&value) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let reencrypted = active
            .encrypt(&plaintext)
            .context("failed to re-encrypt input_values with active key")?;
        conn.execute(
            "UPDATE installed_servers SET input_values = ?1 WHERE rowid = ?2",
            rusqlite::params![reencrypted, rowid],
        )?;
        migrated += 1;
    }
    Ok(migrated)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use crate::crypto::generate_master_key;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn migrate_file_key_credentials_to_active_key() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();

        let file_provider = FileKeyProvider::new(data_dir).unwrap();
        let file_key = file_provider.get_or_create_key().unwrap();
        let file_encryptor = FieldEncryptor::new(&file_key).unwrap();

        let active_key = generate_master_key().unwrap();
        let active_encryptor = FieldEncryptor::new(&active_key).unwrap();

        let db_path = data_dir.join("mcpmux.db");
        let db = Database::open(&db_path).unwrap();

        let space_id = Uuid::new_v4();
        let conn = db.connection();
        conn.execute(
            "INSERT INTO spaces (id, name, created_at, updated_at) VALUES (?1, 'test', ?2, ?2)",
            rusqlite::params![space_id.to_string(), Utc::now().to_rfc3339()],
        )
        .unwrap();

        let token = "test-oauth-token-value";
        let encrypted = file_encryptor.encrypt(token).unwrap();
        conn.execute(
            "INSERT INTO credentials (space_id, server_id, credential_type, credential_value, created_at, updated_at)
             VALUES (?1, 'demo', 'access_token', ?2, ?3, ?3)",
            rusqlite::params![space_id.to_string(), encrypted, Utc::now().to_rfc3339()],
        )
        .unwrap();

        let stored_before: String = conn
            .query_row(
                "SELECT credential_value FROM credentials WHERE server_id = 'demo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(active_encryptor.decrypt(&stored_before).is_err());

        let migrated = migrate_file_key_encrypted_fields(&db, data_dir, &active_encryptor).unwrap();
        assert_eq!(migrated, 1);

        let stored_after: String = conn
            .query_row(
                "SELECT credential_value FROM credentials WHERE server_id = 'demo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active_encryptor.decrypt(&stored_after).unwrap(), token);
        assert!(file_encryptor.decrypt(&stored_after).is_err());
    }
}
