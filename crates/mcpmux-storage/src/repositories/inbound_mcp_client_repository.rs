//! SQLite implementation of InboundMcpClientRepository.
//!
//! Identity-only persistence for approved MCP clients. Per-client grants and
//! connection modes have been removed — routing is driven by WorkspaceBinding
//! + each Space's Default feature set (see FeatureSetResolverService).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{Client, InboundMcpClientRepository};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// SQLite-backed implementation of InboundMcpClientRepository.
///
/// Reads identity columns from the unified `inbound_clients` table. OAuth
/// fields (registrations, tokens, etc.) live alongside but are managed
/// through `InboundClientRepository` (the OAuth-oriented helper).
pub struct SqliteInboundMcpClientRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteInboundMcpClientRepository {
    /// Create a new SQLite client repository.
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
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

    /// Parse optional datetime string.
    fn parse_optional_datetime(s: &Option<String>) -> Option<DateTime<Utc>> {
        s.as_ref().map(|s| Self::parse_datetime(s))
    }

    /// Columns selected for every `Client` read. Order must match `map_row`.
    const COLUMNS: &'static str =
        "client_id, client_name, registration_type, last_seen, created_at, updated_at";

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Client> {
        Ok(Client {
            id: row
                .get::<_, String>(0)?
                .parse()
                .unwrap_or_else(|_| Uuid::new_v4()),
            name: row.get(1)?,
            client_type: row.get(2)?,
            access_key: None,
            last_seen: Self::parse_optional_datetime(&row.get(3)?),
            created_at: Self::parse_datetime(&row.get::<_, String>(4)?),
            updated_at: Self::parse_datetime(&row.get::<_, String>(5)?),
        })
    }
}

#[async_trait]
impl InboundMcpClientRepository for SqliteInboundMcpClientRepository {
    async fn list(&self) -> Result<Vec<Client>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let sql = format!(
            "SELECT {} FROM inbound_clients ORDER BY client_name ASC",
            Self::COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let clients = stmt
            .query_map([], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(clients)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Client>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let sql = format!(
            "SELECT {} FROM inbound_clients WHERE client_id = ?",
            Self::COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let client = stmt
            .query_row(params![id.to_string()], Self::map_row)
            .optional()?;
        Ok(client)
    }

    async fn get_by_access_key(&self, key_hash: &str) -> Result<Option<Client>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let sql = format!(
            "SELECT {} FROM inbound_clients WHERE access_key_hash = ?",
            Self::COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let client = stmt
            .query_row(params![key_hash], Self::map_row)
            .optional()?;
        Ok(client)
    }

    async fn create(&self, client: &Client) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT INTO inbound_clients (
                client_id, registration_type, client_name, last_seen, created_at, updated_at,
                redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                client.id.to_string(),
                "preregistered", // Default registration type for MCP clients
                client.name,
                client.last_seen.map(|dt| dt.to_rfc3339()),
                client.created_at.to_rfc3339(),
                client.updated_at.to_rfc3339(),
                "[]",           // redirect_uris
                "[]",           // grant_types
                "[]",           // response_types
                "none",         // token_endpoint_auth_method
                None::<String>, // scope
            ],
        )?;

        Ok(())
    }

    async fn update(&self, client: &Client) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let rows_affected = conn.execute(
            "UPDATE inbound_clients
             SET client_name = ?2, last_seen = ?3, updated_at = ?4
             WHERE client_id = ?1",
            params![
                client.id.to_string(),
                client.name,
                client.last_seen.map(|dt| dt.to_rfc3339()),
                client.updated_at.to_rfc3339(),
            ],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("Client not found: {}", client.id);
        }

        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM inbound_clients WHERE client_id = ?",
            params![id.to_string()],
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crud_operations() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteInboundMcpClientRepository::new(db);

        let client = Client::cursor();
        repo.create(&client).await.unwrap();

        let found = repo.get(&client.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Cursor");

        let all = repo.list().await.unwrap();
        assert_eq!(all.len(), 1);

        let mut updated = client.clone();
        updated.name = "Cursor AI".to_string();
        repo.update(&updated).await.unwrap();

        let found = repo.get(&client.id).await.unwrap().unwrap();
        assert_eq!(found.name, "Cursor AI");

        repo.delete(&client.id).await.unwrap();
        let found = repo.get(&client.id).await.unwrap();
        assert!(found.is_none());
    }
}
