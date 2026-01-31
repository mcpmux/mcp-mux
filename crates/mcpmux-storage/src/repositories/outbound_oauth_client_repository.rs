//! SQLite implementation of OutboundOAuthRepository.
//! 
//! Manages OUTBOUND OAuth registrations where MCMux acts as OAuth client
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

use crate::Database;

/// SQLite-backed outbound OAuth client repository.
pub struct SqliteOutboundOAuthRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteOutboundOAuthRepository {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
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
    async fn get(&self, space_id: &Uuid, server_id: &str) -> Result<Option<OutboundOAuthRegistration>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, space_id, server_id, server_url, client_id, redirect_uri, metadata_json, created_at, updated_at
             FROM outbound_oauth_clients
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
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })
            .optional()?;

        match row {
            Some((id, space_id_str, server_id, server_url, client_id, redirect_uri, metadata_json, created_at, updated_at)) => {
                // Parse metadata from JSON if present
                let metadata: Option<StoredOAuthMetadata> = metadata_json
                    .and_then(|json| {
                        serde_json::from_str(&json)
                            .map_err(|e| {
                                warn!("Failed to parse stored OAuth metadata: {}", e);
                                e
                            })
                            .ok()
                    });
                
                Ok(Some(OutboundOAuthRegistration {
                    id: id.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    space_id: space_id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    server_id,
                    server_url,
                    client_id,
                    redirect_uri,
                    metadata,
                    created_at: Self::parse_datetime(&created_at),
                    updated_at: Self::parse_datetime(&updated_at),
                }))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, reg: &OutboundOAuthRegistration) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Serialize metadata to JSON if present
        let metadata_json: Option<String> = reg.metadata
            .as_ref()
            .and_then(|m| serde_json::to_string(m).ok());

        conn.execute(
            "INSERT INTO outbound_oauth_clients (
                id, space_id, server_id, server_url, client_id, redirect_uri, metadata_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(space_id, server_id) DO UPDATE SET
                server_url = excluded.server_url,
                client_id = excluded.client_id,
                redirect_uri = excluded.redirect_uri,
                metadata_json = excluded.metadata_json,
                updated_at = excluded.updated_at",
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
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, space_id, server_id, server_url, client_id, redirect_uri, metadata_json, created_at, updated_at
             FROM outbound_oauth_clients
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
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;

        let mut registrations = Vec::new();
        for row in rows {
            let (id, space_id_str, server_id, server_url, client_id, redirect_uri, metadata_json, created_at, updated_at) = row?;

            // Parse metadata from JSON if present
            let metadata: Option<StoredOAuthMetadata> = metadata_json
                .and_then(|json| serde_json::from_str(&json).ok());

            registrations.push(OutboundOAuthRegistration {
                id: id.parse().unwrap_or_else(|_| Uuid::new_v4()),
                space_id: space_id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
                server_id,
                server_url,
                client_id,
                redirect_uri,
                metadata,
                created_at: Self::parse_datetime(&created_at),
                updated_at: Self::parse_datetime(&updated_at),
            });
        }

        Ok(registrations)
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
        let repo = SqliteOutboundOAuthRepository::new(db.clone());

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
        assert_eq!(found.redirect_uri, Some("http://127.0.0.1:9876/callback".to_string()));
        assert!(found.matches_redirect_uri("http://127.0.0.1:9876/callback"));
        assert!(!found.matches_redirect_uri("http://127.0.0.1:9877/callback"));

        repo.delete(&space_id, "cloudflare-bindings").await.unwrap();
        assert!(repo.get(&space_id, "cloudflare-bindings").await.unwrap().is_none());
    }
}
