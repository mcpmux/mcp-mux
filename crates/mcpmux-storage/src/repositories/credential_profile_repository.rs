//! SQLite implementation of CredentialProfileRepository.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{CredentialProfile, CredentialProfileRepository, RepoResult};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    crypto::{binding, FieldEncryptor},
    Database,
};

/// Raw row data extracted from SQLite before decryption.
struct RawProfileRow {
    id: String,
    name: String,
    server_id: String,
    server_name: Option<String>,
    values_encrypted: String,
    created_at: String,
    updated_at: String,
}

/// SQLite-backed implementation of `CredentialProfileRepository`.
pub struct SqliteCredentialProfileRepository {
    db: Arc<Mutex<Database>>,
    encryptor: Arc<FieldEncryptor>,
}

impl SqliteCredentialProfileRepository {
    /// Create a new SQLite credential profile repository.
    pub fn new(db: Arc<Mutex<Database>>, encryptor: Arc<FieldEncryptor>) -> Self {
        Self { db, encryptor }
    }

    /// Encrypt values as JSON, bound to this row id.
    fn encrypt_values(&self, id: &str, values: &HashMap<String, String>) -> Result<String> {
        let json = serde_json::to_string(values)?;
        let context = binding::credential_profile("values_encrypted", id);
        self.encryptor
            .encrypt_bound(&json, &context)
            .map_err(|e| anyhow::anyhow!("Failed to encrypt credential profile values: {e}"))
    }

    /// Decrypt values from stored ciphertext.
    fn decrypt_values(&self, id: &str, stored: &str) -> Result<HashMap<String, String>> {
        if stored.trim().is_empty() {
            return Ok(HashMap::new());
        }
        let context = binding::credential_profile("values_encrypted", id);
        if let Ok(json) = self.encryptor.decrypt_bound(stored, &context) {
            return serde_json::from_str(&json)
                .map_err(|e| anyhow::anyhow!("Corrupt decrypted credential profile JSON: {e}"));
        }
        if FieldEncryptor::is_bound(stored) {
            anyhow::bail!(
                "Failed to decrypt credential profile values (wrong master key, or value was moved or tampered with)"
            );
        }
        if self.encryptor.requires_bound() {
            anyhow::bail!("Credential profile values are not bound; refusing");
        }
        serde_json::from_str(stored)
            .map_err(|e| anyhow::anyhow!("Failed to parse profile values: {e}"))
    }

    fn parse_row(&self, row: RawProfileRow) -> Result<CredentialProfile> {
        let id = Uuid::parse_str(&row.id)?;
        let values = self.decrypt_values(&row.id, &row.values_encrypted)?;
        let created_at = DateTime::parse_from_rfc3339(&row.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let updated_at = DateTime::parse_from_rfc3339(&row.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(CredentialProfile {
            id,
            name: row.name,
            server_id: row.server_id,
            server_name: row.server_name,
            values,
            created_at,
            updated_at,
        })
    }

    const SELECT_COLS: &'static str =
        "id, name, server_id, server_name, values_encrypted, created_at, updated_at";

    fn extract_raw_row(row: &rusqlite::Row) -> rusqlite::Result<RawProfileRow> {
        Ok(RawProfileRow {
            id: row.get(0)?,
            name: row.get(1)?,
            server_id: row.get(2)?,
            server_name: row.get(3)?,
            values_encrypted: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }
}

#[async_trait]
impl CredentialProfileRepository for SqliteCredentialProfileRepository {
    async fn list(&self) -> RepoResult<Vec<CredentialProfile>> {
        let db = self.db.lock().await;
        let mut stmt = db.connection().prepare(&format!(
            "SELECT {} FROM credential_profiles ORDER BY name COLLATE NOCASE ASC",
            Self::SELECT_COLS
        ))?;

        let raw_rows = stmt
            .query_map([], Self::extract_raw_row)?
            .collect::<Result<Vec<_>, _>>()?;

        raw_rows.into_iter().map(|r| self.parse_row(r)).collect()
    }

    async fn list_for_server(&self, server_id: &str) -> RepoResult<Vec<CredentialProfile>> {
        let db = self.db.lock().await;
        let mut stmt = db.connection().prepare(&format!(
            "SELECT {} FROM credential_profiles WHERE server_id = ?1 ORDER BY name COLLATE NOCASE ASC",
            Self::SELECT_COLS
        ))?;

        let raw_rows = stmt
            .query_map(params![server_id], Self::extract_raw_row)?
            .collect::<Result<Vec<_>, _>>()?;

        raw_rows.into_iter().map(|r| self.parse_row(r)).collect()
    }

    async fn get(&self, id: &Uuid) -> RepoResult<Option<CredentialProfile>> {
        let db = self.db.lock().await;
        let mut stmt = db.connection().prepare(&format!(
            "SELECT {} FROM credential_profiles WHERE id = ?1",
            Self::SELECT_COLS
        ))?;

        let row_id = id.to_string();
        let raw = stmt
            .query_row(params![row_id], Self::extract_raw_row)
            .optional()?;

        raw.map(|r| self.parse_row(r)).transpose()
    }

    async fn save(&self, profile: &CredentialProfile) -> RepoResult<()> {
        let row_id = profile.id.to_string();
        let encrypted_values = self.encrypt_values(&row_id, &profile.values)?;
        let created_at = profile.created_at.to_rfc3339();
        let updated_at = profile.updated_at.to_rfc3339();

        let db = self.db.lock().await;
        db.connection().execute(
            "INSERT INTO credential_profiles
                (id, name, server_id, server_name, values_encrypted, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                name = ?2,
                server_id = ?3,
                server_name = ?4,
                values_encrypted = ?5,
                updated_at = ?7",
            params![
                row_id,
                profile.name,
                profile.server_id,
                profile.server_name,
                encrypted_values,
                created_at,
                updated_at,
            ],
        )?;

        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> RepoResult<()> {
        let row_id = id.to_string();
        let db = self.db.lock().await;
        db.connection().execute(
            "DELETE FROM credential_profiles WHERE id = ?1",
            params![row_id],
        )?;
        Ok(())
    }

    async fn count_usages(&self, id: &Uuid) -> RepoResult<usize> {
        let row_id = id.to_string();
        let db = self.db.lock().await;
        let count: i64 = db.connection().query_row(
            "SELECT COUNT(*) FROM installed_servers WHERE credential_profile_id = ?1",
            params![row_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}
