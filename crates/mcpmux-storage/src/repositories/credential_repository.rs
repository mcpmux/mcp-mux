//! SQLite implementation of CredentialRepository with encryption.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{Credential, CredentialRepository, CredentialValue};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::crypto::FieldEncryptor;
use crate::Database;

/// SQLite-backed credential repository with field-level encryption.
///
/// Sensitive fields (tokens, keys) are encrypted using AES-256-GCM
/// before being stored in the database.
pub struct SqliteCredentialRepository {
    db: Arc<Mutex<Database>>,
    encryptor: Arc<FieldEncryptor>,
}

impl SqliteCredentialRepository {
    /// Create a new credential repository.
    pub fn new(db: Arc<Mutex<Database>>, encryptor: Arc<FieldEncryptor>) -> Self {
        Self { db, encryptor }
    }

    /// Encrypt a credential value to JSON.
    fn encrypt_value(&self, value: &CredentialValue) -> Result<String> {
        let json = serde_json::to_string(value)?;
        self.encryptor.encrypt(&json)
    }

    /// Decrypt a credential value from encrypted JSON.
    fn decrypt_value(&self, encrypted: &str) -> Result<CredentialValue> {
        let json = self.encryptor.decrypt(encrypted)?;
        serde_json::from_str(&json).context("Failed to parse credential value")
    }

    /// Parse a datetime string to DateTime<Utc>.
    fn parse_datetime(s: &str) -> DateTime<Utc> {
        // Try RFC3339 first
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        // Try SQLite datetime format
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        Utc::now()
    }

    /// Parse an optional datetime string.
    fn parse_optional_datetime(s: Option<String>) -> Option<DateTime<Utc>> {
        s.map(|dt| Self::parse_datetime(&dt))
    }
}

#[async_trait]
impl CredentialRepository for SqliteCredentialRepository {
    async fn get(&self, space_id: &Uuid, server_id: &str) -> Result<Option<Credential>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT space_id, server_id, credential_value, created_at, updated_at, last_used_at
             FROM credentials
             WHERE space_id = ? AND server_id = ?",
        )?;

        let row = stmt
            .query_row(params![space_id.to_string(), server_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .optional()?;

        match row {
            Some((space_id_str, server_id, encrypted_value, created_at, updated_at, last_used)) => {
                let value = self.decrypt_value(&encrypted_value)?;
                Ok(Some(Credential {
                    space_id: space_id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    server_id,
                    value,
                    created_at: Self::parse_datetime(&created_at),
                    updated_at: Self::parse_datetime(&updated_at),
                    last_used: Self::parse_optional_datetime(last_used),
                }))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, credential: &Credential) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let encrypted_value = self.encrypt_value(&credential.value)?;

        conn.execute(
            "INSERT INTO credentials (id, space_id, server_id, credential_type, credential_value, created_at, updated_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(space_id, server_id) DO UPDATE SET
                credential_type = excluded.credential_type,
                credential_value = excluded.credential_value,
                updated_at = excluded.updated_at,
                last_used_at = excluded.last_used_at",
            params![
                Uuid::new_v4().to_string(),
                credential.space_id.to_string(),
                credential.server_id,
                credential_type_name(&credential.value),
                encrypted_value,
                credential.created_at.to_rfc3339(),
                credential.updated_at.to_rfc3339(),
                credential.last_used.map(|dt| dt.to_rfc3339()),
            ],
        )?;

        Ok(())
    }

    async fn delete(&self, space_id: &Uuid, server_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM credentials WHERE space_id = ? AND server_id = ?",
            params![space_id.to_string(), server_id],
        )?;

        Ok(())
    }

    async fn clear_tokens(&self, space_id: &Uuid, server_id: &str) -> Result<bool> {
        // For OAuth, client registration is in oauth_clients table, so just delete tokens
        let existing = self.get(space_id, server_id).await?;

        match existing {
            Some(credential) if credential.is_oauth() => {
                // Delete OAuth tokens - client registration is preserved in oauth_clients
                self.delete(space_id, server_id).await?;
                Ok(true)
            }
            Some(_) => {
                // Non-OAuth credentials (API keys) - don't clear on logout
                Ok(false)
            }
            None => Ok(false),
        }
    }

    async fn list_for_space(&self, space_id: &Uuid) -> Result<Vec<Credential>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT space_id, server_id, credential_value, created_at, updated_at, last_used_at
             FROM credentials
             WHERE space_id = ?
             ORDER BY server_id",
        )?;

        let rows = stmt.query_map(params![space_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;

        let mut credentials = Vec::new();
        for row in rows {
            let (space_id_str, server_id, encrypted_value, created_at, updated_at, last_used) =
                row?;
            let value = self.decrypt_value(&encrypted_value)?;
            credentials.push(Credential {
                space_id: space_id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
                server_id,
                value,
                created_at: Self::parse_datetime(&created_at),
                updated_at: Self::parse_datetime(&updated_at),
                last_used: Self::parse_optional_datetime(last_used),
            });
        }

        Ok(credentials)
    }
}

/// Get the type name for a credential value.
fn credential_type_name(value: &CredentialValue) -> &'static str {
    match value {
        CredentialValue::ApiKey { .. } => "api_key",
        CredentialValue::OAuth { .. } => "oauth",
        CredentialValue::BasicAuth { .. } => "basic_auth",
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
        // Create in-memory database and encryptor
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
        let found = repo.get(&space_id, "github").await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.server_id, "github");
        match found.value {
            CredentialValue::ApiKey { key } => assert_eq!(key, "ghp_test_token_12345"),
            _ => panic!("Wrong credential type"),
        }

        // Update
        let updated_cred = Credential::api_key(space_id, "github", "ghp_new_token");
        repo.save(&updated_cred).await.unwrap();

        let found = repo.get(&space_id, "github").await.unwrap().unwrap();
        match found.value {
            CredentialValue::ApiKey { key } => assert_eq!(key, "ghp_new_token"),
            _ => panic!("Wrong credential type"),
        }

        // Delete
        repo.delete(&space_id, "github").await.unwrap();
        let found = repo.get(&space_id, "github").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_oauth_credential() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteCredentialRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;
        let expires = Utc::now() + chrono::Duration::hours(1);

        let cred = Credential::oauth(
            space_id,
            "atlassian",
            "access_token_xyz",
            Some("refresh_token_abc".to_string()),
            Some(expires),
        );
        repo.save(&cred).await.unwrap();

        let found = repo.get(&space_id, "atlassian").await.unwrap().unwrap();
        match found.value {
            CredentialValue::OAuth {
                access_token,
                refresh_token,
                ..
            } => {
                assert_eq!(access_token, "access_token_xyz");
                assert_eq!(refresh_token, Some("refresh_token_abc".to_string()));
            }
            _ => panic!("Wrong credential type"),
        }
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

        // List space1
        let creds = repo.list_for_space(&space1).await.unwrap();
        assert_eq!(creds.len(), 2);

        // List space2
        let creds = repo.list_for_space(&space2).await.unwrap();
        assert_eq!(creds.len(), 1);
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
    }
}
