//! SQLite implementation of CredentialRepository with typed rows and encryption.
//!
//! Each credential is stored as a separate row per (space, server, type).
//! Only the secret value is encrypted — metadata (type, expiry, scope) is plaintext.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{Credential, CredentialRepository, CredentialType};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::crypto::FieldEncryptor;
use crate::Database;

/// Raw row data extracted from SQLite before decryption.
struct RawCredentialRow {
    space_id: String,
    server_id: String,
    credential_type: String,
    credential_value: String, // Encrypted
    expires_at: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    last_used_at: Option<String>,
    created_at: String,
    updated_at: String,
}

/// SQLite-backed credential repository with field-level encryption.
///
/// Only the secret value (token, key, password) is encrypted using AES-256-GCM.
/// Metadata fields (type, expiry, scope) are stored as plaintext for queryability.
pub struct SqliteCredentialRepository {
    db: Arc<Mutex<Database>>,
    encryptor: Arc<FieldEncryptor>,
}

impl SqliteCredentialRepository {
    /// Create a new credential repository.
    pub fn new(db: Arc<Mutex<Database>>, encryptor: Arc<FieldEncryptor>) -> Self {
        Self { db, encryptor }
    }

    /// Encrypt a credential value for storage.
    fn encrypt_value(&self, value: &str) -> Result<String> {
        self.encryptor
            .encrypt(value)
            .map_err(|e| anyhow::anyhow!("Failed to encrypt credential value: {}", e))
    }

    /// Decrypt a credential value from storage.
    fn decrypt_value(&self, encrypted: &str) -> Result<String> {
        self.encryptor
            .decrypt(encrypted)
            .map_err(|e| anyhow::anyhow!("Failed to decrypt credential value: {}", e))
    }

    /// Parse a datetime string to DateTime<Utc>.
    fn parse_datetime(s: &str) -> DateTime<Utc> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        Utc::now()
    }

    /// Parse an optional datetime string.
    fn parse_optional_datetime(s: Option<String>) -> Option<DateTime<Utc>> {
        s.map(|dt| Self::parse_datetime(&dt))
    }

    /// Standard column list for SELECT queries.
    const SELECT_COLUMNS: &'static str =
        "space_id, server_id, credential_type, credential_value, expires_at, token_type, scope, last_used_at, created_at, updated_at";

    /// Extract raw row data from a rusqlite Row.
    fn extract_row(row: &rusqlite::Row) -> rusqlite::Result<RawCredentialRow> {
        Ok(RawCredentialRow {
            space_id: row.get(0)?,
            server_id: row.get(1)?,
            credential_type: row.get(2)?,
            credential_value: row.get(3)?,
            expires_at: row.get(4)?,
            token_type: row.get(5)?,
            scope: row.get(6)?,
            last_used_at: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    }

    /// Build a Credential from extracted row data (needs &self for decryption).
    fn build_credential(&self, row: RawCredentialRow) -> Result<Credential> {
        let value = self.decrypt_value(&row.credential_value)?;
        let credential_type = CredentialType::parse(&row.credential_type)
            .ok_or_else(|| anyhow::anyhow!("Unknown credential type: {}", row.credential_type))?;

        Ok(Credential {
            space_id: row.space_id.parse().unwrap_or_else(|_| Uuid::new_v4()),
            server_id: row.server_id,
            credential_type,
            value,
            expires_at: Self::parse_optional_datetime(row.expires_at),
            token_type: row.token_type,
            scope: row.scope,
            created_at: Self::parse_datetime(&row.created_at),
            updated_at: Self::parse_datetime(&row.updated_at),
            last_used: Self::parse_optional_datetime(row.last_used_at),
        })
    }
}

#[async_trait]
impl CredentialRepository for SqliteCredentialRepository {
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
        credential_type: &CredentialType,
    ) -> Result<Option<Credential>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM credentials WHERE space_id = ?1 AND server_id = ?2 AND credential_type = ?3",
            Self::SELECT_COLUMNS
        ))?;

        let row = stmt
            .query_row(
                params![space_id.to_string(), server_id, credential_type.as_str()],
                Self::extract_row,
            )
            .optional()?;

        match row {
            Some(raw) => Ok(Some(self.build_credential(raw)?)),
            None => Ok(None),
        }
    }

    async fn get_all(&self, space_id: &Uuid, server_id: &str) -> Result<Vec<Credential>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM credentials WHERE space_id = ?1 AND server_id = ?2 ORDER BY credential_type",
            Self::SELECT_COLUMNS
        ))?;

        let rows: Vec<_> = stmt
            .query_map(params![space_id.to_string(), server_id], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_credential(r)).collect()
    }

    async fn save(&self, credential: &Credential) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let encrypted_value = self.encrypt_value(&credential.value)?;

        conn.execute(
            "INSERT INTO credentials (id, space_id, server_id, credential_type, credential_value, expires_at, token_type, scope, last_used_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(space_id, server_id, credential_type) DO UPDATE SET
                credential_value = excluded.credential_value,
                expires_at = excluded.expires_at,
                token_type = excluded.token_type,
                scope = excluded.scope,
                updated_at = excluded.updated_at,
                last_used_at = excluded.last_used_at",
            params![
                Uuid::new_v4().to_string(),
                credential.space_id.to_string(),
                credential.server_id,
                credential.credential_type.as_str(),
                encrypted_value,
                credential.expires_at.map(|dt| dt.to_rfc3339()),
                credential.token_type,
                credential.scope,
                credential.last_used.map(|dt| dt.to_rfc3339()),
                credential.created_at.to_rfc3339(),
                credential.updated_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    async fn delete(
        &self,
        space_id: &Uuid,
        server_id: &str,
        credential_type: &CredentialType,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM credentials WHERE space_id = ?1 AND server_id = ?2 AND credential_type = ?3",
            params![space_id.to_string(), server_id, credential_type.as_str()],
        )?;

        Ok(())
    }

    async fn delete_all(&self, space_id: &Uuid, server_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM credentials WHERE space_id = ?1 AND server_id = ?2",
            params![space_id.to_string(), server_id],
        )?;

        Ok(())
    }

    async fn clear_tokens(&self, space_id: &Uuid, server_id: &str) -> Result<bool> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Delete only OAuth tokens (access_token + refresh_token), preserve API keys etc.
        let deleted = conn.execute(
            "DELETE FROM credentials WHERE space_id = ?1 AND server_id = ?2 AND credential_type IN ('access_token', 'refresh_token')",
            params![space_id.to_string(), server_id],
        )?;

        Ok(deleted > 0)
    }

    async fn list_for_space(&self, space_id: &Uuid) -> Result<Vec<Credential>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM credentials WHERE space_id = ?1 ORDER BY server_id, credential_type",
            Self::SELECT_COLUMNS
        ))?;

        let rows: Vec<_> = stmt
            .query_map(params![space_id.to_string()], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_credential(r)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a space in the database (for foreign key constraints)
    async fn create_test_space(db: &Arc<Mutex<Database>>, space_id: &Uuid) {
        let db_lock = db.lock().await;
        db_lock.connection().execute(
            "INSERT INTO spaces (id, name, created_at, updated_at) VALUES (?, 'Test', datetime('now'), datetime('now'))",
            params![space_id.to_string()],
        ).unwrap();
    }

    #[tokio::test]
    async fn test_credential_crud() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;

        // Create API key credential
        let cred = Credential::api_key(space_id, "github", "ghp_test_token_12345");
        repo.save(&cred).await.unwrap();

        // Retrieve
        let found = repo
            .get(&space_id, "github", &CredentialType::ApiKey)
            .await
            .unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.server_id, "github");
        assert_eq!(found.credential_type, CredentialType::ApiKey);
        assert_eq!(found.value, "ghp_test_token_12345");

        // Update
        let updated_cred = Credential::api_key(space_id, "github", "ghp_new_token");
        repo.save(&updated_cred).await.unwrap();

        let found = repo
            .get(&space_id, "github", &CredentialType::ApiKey)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.value, "ghp_new_token");

        // Delete
        repo.delete(&space_id, "github", &CredentialType::ApiKey)
            .await
            .unwrap();
        let found = repo
            .get(&space_id, "github", &CredentialType::ApiKey)
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_access_token_credential() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        let cred =
            Credential::access_token(space_id, "atlassian", "access_token_xyz", Some(expires));
        repo.save(&cred).await.unwrap();

        let found = repo
            .get(&space_id, "atlassian", &CredentialType::AccessToken)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(found.credential_type, CredentialType::AccessToken);
        assert_eq!(found.value, "access_token_xyz");
        assert_eq!(found.token_type, Some("Bearer".to_string()));
        assert!(!found.is_expired());
    }

    #[tokio::test]
    async fn test_separate_access_and_refresh_tokens() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;

        // Save access token and refresh token as separate rows
        let access = Credential::access_token(
            space_id,
            "atlassian",
            "access_xyz",
            Some(Utc::now() + chrono::Duration::hours(1)),
        );
        let refresh = Credential::refresh_token(space_id, "atlassian", "refresh_abc", None);

        repo.save(&access).await.unwrap();
        repo.save(&refresh).await.unwrap();

        // Get all for server — should return 2 rows
        let all = repo.get_all(&space_id, "atlassian").await.unwrap();
        assert_eq!(all.len(), 2);

        // Get each individually
        let found_access = repo
            .get(&space_id, "atlassian", &CredentialType::AccessToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found_access.value, "access_xyz");

        let found_refresh = repo
            .get(&space_id, "atlassian", &CredentialType::RefreshToken)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found_refresh.value, "refresh_abc");
    }

    #[tokio::test]
    async fn test_clear_tokens_only_removes_oauth() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;

        // Save access_token, refresh_token, and api_key for same server
        let access = Credential::access_token(space_id, "server", "access", None);
        let refresh = Credential::refresh_token(space_id, "server", "refresh", None);
        let api_key = Credential::api_key(space_id, "server", "key123");

        repo.save(&access).await.unwrap();
        repo.save(&refresh).await.unwrap();
        repo.save(&api_key).await.unwrap();

        // clear_tokens should remove access + refresh but keep api_key
        let cleared = repo.clear_tokens(&space_id, "server").await.unwrap();
        assert!(cleared);

        let all = repo.get_all(&space_id, "server").await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].credential_type, CredentialType::ApiKey);
    }

    #[tokio::test]
    async fn test_delete_all() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;

        repo.save(&Credential::access_token(
            space_id, "server", "access", None,
        ))
        .await
        .unwrap();
        repo.save(&Credential::api_key(space_id, "server", "key"))
            .await
            .unwrap();

        repo.delete_all(&space_id, "server").await.unwrap();

        let all = repo.get_all(&space_id, "server").await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn test_list_for_space() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space1 = Uuid::new_v4();
        let space2 = Uuid::new_v4();
        create_test_space(&db, &space1).await;
        create_test_space(&db, &space2).await;

        // Add credentials to space1
        repo.save(&Credential::api_key(space1, "github", "token1"))
            .await
            .unwrap();
        repo.save(&Credential::api_key(space1, "gitlab", "token2"))
            .await
            .unwrap();

        // Add credential to space2
        repo.save(&Credential::api_key(space2, "github", "token3"))
            .await
            .unwrap();

        assert_eq!(repo.list_for_space(&space1).await.unwrap().len(), 2);
        assert_eq!(repo.list_for_space(&space2).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_encryption_is_applied() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;
        let secret_token = "super_secret_token_12345";

        repo.save(&Credential::api_key(space_id, "test", secret_token))
            .await
            .unwrap();

        // Query raw database to verify encryption
        let db_lock = db.lock().await;
        let conn = db_lock.connection();
        let raw_value: String = conn
            .query_row(
                "SELECT credential_value FROM credentials WHERE server_id = 'test'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        // Raw value should NOT contain the plaintext secret
        assert!(!raw_value.contains(secret_token));

        // Raw value should be hex-encoded (encrypted)
        assert!(hex::decode(&raw_value).is_ok());

        // But expires_at and token_type should be plaintext (queryable)
        let (cred_type, expires_at): (String, Option<String>) = conn
            .query_row(
                "SELECT credential_type, expires_at FROM credentials WHERE server_id = 'test'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(cred_type, "api_key");
        assert!(expires_at.is_none()); // API keys don't expire
    }
}
