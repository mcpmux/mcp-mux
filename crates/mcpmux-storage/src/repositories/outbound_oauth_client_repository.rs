//! SQLite implementation of OutboundOAuthRepository.
//!
//! Manages OUTBOUND OAuth registrations where McpMux acts as OAuth client
//! connecting TO backend MCP servers (e.g., Cloudflare, Atlassian).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{OutboundOAuthRegistration, OutboundOAuthRepository, StoredOAuthMetadata};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

use crate::crypto::{binding, FieldEncryptor};
use crate::Database;

/// Columns `get` and `list_for_space` read, in `RegistrationRow::from_row` order
const SELECT_COLUMNS: &str = "id, space_id, server_id, server_url, client_id, redirect_uri, \
     metadata_json, created_at, updated_at, client_secret_encrypted, \
     client_secret_expires_at, token_endpoint_auth_method";

/// A registration row as stored, before the client secret is decrypted
struct RegistrationRow {
    id: String,
    space_id: String,
    server_id: String,
    server_url: String,
    client_id: String,
    redirect_uri: Option<String>,
    metadata_json: Option<String>,
    created_at: String,
    updated_at: String,
    client_secret_encrypted: Option<String>,
    client_secret_expires_at: Option<String>,
    token_endpoint_auth_method: Option<String>,
}

impl RegistrationRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            space_id: row.get(1)?,
            server_id: row.get(2)?,
            server_url: row.get(3)?,
            client_id: row.get(4)?,
            redirect_uri: row.get(5)?,
            metadata_json: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            client_secret_encrypted: row.get(9)?,
            client_secret_expires_at: row.get(10)?,
            token_endpoint_auth_method: row.get(11)?,
        })
    }
}

/// SQLite-backed outbound OAuth client repository.
///
/// The DCR `client_secret` is encrypted with AES-256-GCM; everything else is plaintext.
pub struct SqliteOutboundOAuthRepository {
    db: Arc<Mutex<Database>>,
    encryptor: Arc<FieldEncryptor>,
}

impl SqliteOutboundOAuthRepository {
    pub fn new(db: Arc<Mutex<Database>>, encryptor: Arc<FieldEncryptor>) -> Self {
        Self { db, encryptor }
    }

    /// Encrypt a client secret bound to the Space and server it belongs to.
    fn encrypt_secret(
        &self,
        secret: Option<&str>,
        space_id: &str,
        server_id: &str,
    ) -> Result<Option<String>> {
        let context = binding::outbound_client_secret(space_id, server_id);
        secret
            .map(|s| {
                self.encryptor
                    .encrypt_bound(s, &context)
                    .map_err(|e| anyhow::anyhow!("Failed to encrypt client secret: {}", e))
            })
            .transpose()
    }

    /// A secret that can't be decrypted (e.g. after a master key reset) fails the
    /// lookup, like a credential that can't be decrypted. Returning the client without
    /// its secret would make the next sign-in reuse it and fail. The sign-in flow
    /// instead treats a failed lookup as no registration, so it registers a fresh
    /// client, whose save overwrites this row.
    fn decrypt_secret(
        &self,
        encrypted: Option<String>,
        space_id: &str,
        server_id: &str,
    ) -> Result<Option<String>> {
        let context = binding::outbound_client_secret(space_id, server_id);
        encrypted
            .map(|encrypted| {
                self.encryptor
                    .decrypt_bound(&encrypted, &context)
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to decrypt OAuth client secret for {}: {}",
                            server_id,
                            e
                        )
                    })
            })
            .transpose()
    }

    fn to_registration(&self, row: RegistrationRow) -> Result<OutboundOAuthRegistration> {
        let client_secret =
            self.decrypt_secret(row.client_secret_encrypted, &row.space_id, &row.server_id)?;

        let metadata: Option<StoredOAuthMetadata> = row.metadata_json.and_then(|json| {
            serde_json::from_str(&json)
                .map_err(|e| warn!("Failed to parse stored OAuth metadata: {}", e))
                .ok()
        });

        Ok(OutboundOAuthRegistration {
            id: row.id.parse().unwrap_or_else(|_| Uuid::new_v4()),
            space_id: row.space_id.parse().unwrap_or_else(|_| Uuid::new_v4()),
            server_id: row.server_id,
            server_url: row.server_url,
            client_id: row.client_id,
            client_secret,
            client_secret_expires_at: row
                .client_secret_expires_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            token_endpoint_auth_method: row.token_endpoint_auth_method,
            redirect_uri: row.redirect_uri,
            metadata,
            created_at: Self::parse_datetime(&row.created_at),
            updated_at: Self::parse_datetime(&row.updated_at),
        })
    }

    fn parse_datetime(s: &str) -> DateTime<Utc> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        Utc::now()
    }
}

#[async_trait]
impl OutboundOAuthRepository for SqliteOutboundOAuthRepository {
    async fn get(
        &self,
        space_id: &Uuid,
        server_id: &str,
    ) -> Result<Option<OutboundOAuthRegistration>> {
        let row = {
            let db = self.db.lock().await;
            db.connection()
                .query_row(
                    &format!(
                        "SELECT {SELECT_COLUMNS} FROM outbound_oauth_clients
                         WHERE space_id = ? AND server_id = ?"
                    ),
                    params![space_id.to_string(), server_id],
                    RegistrationRow::from_row,
                )
                .optional()?
        };

        row.map(|row| self.to_registration(row)).transpose()
    }

    /// Reads only the client_id column, so the per-request credential load never
    /// decrypts the client secret
    async fn get_client_id(&self, space_id: &Uuid, server_id: &str) -> Result<Option<String>> {
        let db = self.db.lock().await;
        let client_id = db
            .connection()
            .query_row(
                "SELECT client_id FROM outbound_oauth_clients
                 WHERE space_id = ? AND server_id = ?",
                params![space_id.to_string(), server_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(client_id)
    }

    async fn save(&self, reg: &OutboundOAuthRegistration) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Serialize metadata to JSON if present
        let metadata_json: Option<String> = reg
            .metadata
            .as_ref()
            .and_then(|m| serde_json::to_string(m).ok());
        let client_secret_encrypted = self.encrypt_secret(
            reg.client_secret.as_deref(),
            &reg.space_id.to_string(),
            &reg.server_id,
        )?;

        conn.execute(
            "INSERT INTO outbound_oauth_clients (
                id, space_id, server_id, server_url, client_id, redirect_uri, metadata_json,
                created_at, updated_at, client_secret_encrypted, client_secret_expires_at,
                token_endpoint_auth_method
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(space_id, server_id) DO UPDATE SET
                server_url = excluded.server_url,
                client_id = excluded.client_id,
                redirect_uri = excluded.redirect_uri,
                metadata_json = excluded.metadata_json,
                updated_at = excluded.updated_at,
                client_secret_encrypted = excluded.client_secret_encrypted,
                client_secret_expires_at = excluded.client_secret_expires_at,
                token_endpoint_auth_method = excluded.token_endpoint_auth_method",
            params![
                reg.id.to_string(),
                reg.space_id.to_string(),
                reg.server_id,
                reg.server_url,
                reg.client_id,
                reg.redirect_uri,
                metadata_json,
                reg.created_at.to_rfc3339(),
                reg.updated_at.to_rfc3339(),
                client_secret_encrypted,
                reg.client_secret_expires_at.map(|dt| dt.to_rfc3339()),
                reg.token_endpoint_auth_method,
            ],
        )?;

        Ok(())
    }

    async fn delete(&self, space_id: &Uuid, server_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM outbound_oauth_clients WHERE space_id = ? AND server_id = ?",
            params![space_id.to_string(), server_id],
        )?;

        Ok(())
    }

    async fn list_for_space(&self, space_id: &Uuid) -> Result<Vec<OutboundOAuthRegistration>> {
        let rows = {
            let db = self.db.lock().await;
            let conn = db.connection();
            let mut stmt = conn.prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM outbound_oauth_clients
                 WHERE space_id = ?
                 ORDER BY server_id"
            ))?;
            let rows = stmt
                .query_map(params![space_id.to_string()], RegistrationRow::from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        rows.into_iter()
            .map(|row| self.to_registration(row))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_space(db: &Arc<Mutex<Database>>, space_id: &Uuid) {
        let db_lock = db.lock().await;
        db_lock.connection().execute(
            "INSERT INTO spaces (id, name, created_at, updated_at) VALUES (?, 'Test', datetime('now'), datetime('now'))",
            params![space_id.to_string()],
        ).unwrap();
    }

    #[tokio::test]
    async fn test_backend_oauth_crud() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let key = crate::crypto::generate_master_key().unwrap();
        let encryptor = Arc::new(FieldEncryptor::new(&key).unwrap());
        let repo = SqliteOutboundOAuthRepository::new(db.clone(), encryptor);

        let space_id = Uuid::new_v4();
        create_test_space(&db, &space_id).await;

        let reg = OutboundOAuthRegistration::new(
            space_id,
            "cloudflare-bindings",
            "https://bindings.mcp.cloudflare.com",
            "client_123",
            "http://127.0.0.1:9876/callback",
        );

        repo.save(&reg).await.unwrap();

        let found = repo.get(&space_id, "cloudflare-bindings").await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.client_id, "client_123");
        assert_eq!(found.server_url, "https://bindings.mcp.cloudflare.com");
        assert_eq!(
            found.redirect_uri,
            Some("http://127.0.0.1:9876/callback".to_string())
        );
        assert!(found.matches_redirect_uri("http://127.0.0.1:9876/callback"));
        assert!(!found.matches_redirect_uri("http://127.0.0.1:9877/callback"));

        repo.delete(&space_id, "cloudflare-bindings").await.unwrap();
        assert!(repo
            .get(&space_id, "cloudflare-bindings")
            .await
            .unwrap()
            .is_none());
    }
}
