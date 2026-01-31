//! SQLite implementation of InboundMcpClientRepository.
//!
//! Manages MCP client entities (apps connecting TO McpMux).
//! Works with the unified `inbound_clients` table.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{Client, InboundMcpClientRepository, ConnectionMode};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// SQLite-backed implementation of InboundMcpClientRepository.
/// 
/// Works with the unified `inbound_clients` table which stores both
/// OAuth registration data and MCP client preferences.
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

    /// Parse optional datetime string.
    fn parse_optional_datetime(s: &Option<String>) -> Option<DateTime<Utc>> {
        s.as_ref().map(|s| Self::parse_datetime(s))
    }

    /// Parse connection mode from string.
    fn parse_connection_mode(mode_str: &str, locked_space_id: &Option<String>) -> ConnectionMode {
        match mode_str {
            "locked" => {
                if let Some(space_id_str) = locked_space_id {
                    if let Ok(space_id) = space_id_str.parse() {
                        return ConnectionMode::Locked { space_id };
                    }
                }
                ConnectionMode::FollowActive
            }
            "ask_on_change" => {
                // Simplified: don't load triggers from DB yet
                ConnectionMode::AskOnChange { triggers: vec![] }
            }
            _ => ConnectionMode::FollowActive,
        }
    }

    /// Convert connection mode to storage strings.
    fn connection_mode_to_strings(mode: &ConnectionMode) -> (&'static str, Option<String>) {
        match mode {
            ConnectionMode::Locked { space_id } => ("locked", Some(space_id.to_string())),
            ConnectionMode::FollowActive => ("follow_active", None),
            ConnectionMode::AskOnChange { .. } => ("ask_on_change", None),
        }
    }

    /// Parse grants JSON to HashMap<Uuid, Vec<Uuid>>.
    fn parse_grants(json: &Option<String>) -> HashMap<Uuid, Vec<Uuid>> {
        json.as_ref()
            .and_then(|s| serde_json::from_str::<HashMap<String, Vec<String>>>(s).ok())
            .map(|m| {
                m.into_iter()
                    .filter_map(|(k, v)| {
                        let key: Uuid = k.parse().ok()?;
                        let vals: Vec<Uuid> = v.into_iter().filter_map(|s| s.parse().ok()).collect();
                        Some((key, vals))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl InboundMcpClientRepository for SqliteInboundMcpClientRepository {
    async fn list(&self) -> Result<Vec<Client>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT client_id, client_name, registration_type, logo_uri, connection_mode, locked_space_id,
                    '{}', last_seen, created_at, updated_at
             FROM inbound_clients 
             ORDER BY client_name ASC",
        )?;

        let clients = stmt
            .query_map([], |row| {
                let grants_json: Option<String> = row.get(6)?; // Empty grants JSON placeholder
                Ok(Client {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    name: row.get(1)?,
                    client_type: row.get(2)?,
                    connection_mode: Self::parse_connection_mode(
                        &row.get::<_, String>(4)?,
                        &row.get(5)?,
                    ),
                    grants: Self::parse_grants(&grants_json),
                    access_key: None, // Never loaded from DB
                    last_seen: Self::parse_optional_datetime(&row.get(7)?),
                    created_at: Self::parse_datetime(&row.get::<_, String>(8)?),
                    updated_at: Self::parse_datetime(&row.get::<_, String>(9)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(clients)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Client>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT client_id, client_name, registration_type, logo_uri, connection_mode, locked_space_id,
                    '{}', last_seen, created_at, updated_at
             FROM inbound_clients 
             WHERE client_id = ?",
        )?;

        let client = stmt
            .query_row(params![id.to_string()], |row| {
                let grants_json: Option<String> = row.get(6)?; // Empty grants JSON placeholder
                Ok(Client {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    name: row.get(1)?,
                    client_type: row.get(2)?,
                    connection_mode: Self::parse_connection_mode(
                        &row.get::<_, String>(4)?,
                        &row.get(5)?,
                    ),
                    grants: Self::parse_grants(&grants_json),
                    access_key: None,
                    last_seen: Self::parse_optional_datetime(&row.get(7)?),
                    created_at: Self::parse_datetime(&row.get::<_, String>(8)?),
                    updated_at: Self::parse_datetime(&row.get::<_, String>(9)?),
                })
            })
            .optional()?;

        Ok(client)
    }

    async fn get_by_access_key(&self, key_hash: &str) -> Result<Option<Client>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, name, client_type, logo_uri, connection_mode, locked_space_id,
                    grants, last_seen, created_at, updated_at
             FROM inbound_clients 
             WHERE access_key_hash = ?",
        )?;

        let client = stmt
            .query_row(params![key_hash], |row| {
                let grants_json: Option<String> = row.get(6)?;
                Ok(Client {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    name: row.get(1)?,
                    client_type: row.get(2)?,
                    connection_mode: Self::parse_connection_mode(
                        &row.get::<_, String>(4)?,
                        &row.get(5)?,
                    ),
                    grants: Self::parse_grants(&grants_json),
                    access_key: None,
                    last_seen: Self::parse_optional_datetime(&row.get(7)?),
                    created_at: Self::parse_datetime(&row.get::<_, String>(8)?),
                    updated_at: Self::parse_datetime(&row.get::<_, String>(9)?),
                })
            })
            .optional()?;

        Ok(client)
    }

    async fn create(&self, client: &Client) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let (mode_str, locked_space_id) = Self::connection_mode_to_strings(&client.connection_mode);

        conn.execute(
            "INSERT INTO inbound_clients (
                client_id, registration_type, client_name, logo_uri, 
                connection_mode, locked_space_id, last_seen, created_at, updated_at,
                redirect_uris, grant_types, response_types, token_endpoint_auth_method, scope
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                client.id.to_string(),
                "preregistered", // Default registration type for MCP clients
                client.name,
                None::<String>, // logo_uri
                mode_str,
                locked_space_id,
                client.last_seen.map(|dt| dt.to_rfc3339()),
                client.created_at.to_rfc3339(),
                client.updated_at.to_rfc3339(),
                "[]", // Empty redirect_uris array
                "[]", // Empty grant_types array
                "[]", // Empty response_types array
                "none", // Default auth method
                None::<String>, // No scope
            ],
        )?;

        Ok(())
    }

    async fn update(&self, client: &Client) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let (mode_str, locked_space_id) = Self::connection_mode_to_strings(&client.connection_mode);

        let rows_affected = conn.execute(
            "UPDATE inbound_clients 
             SET client_name = ?2, connection_mode = ?3, locked_space_id = ?4,
                 last_seen = ?5, updated_at = ?6
             WHERE client_id = ?1",
            params![
                client.id.to_string(),
                client.name,
                mode_str,
                locked_space_id,
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

        conn.execute("DELETE FROM inbound_clients WHERE client_id = ?", params![id.to_string()])?;

        Ok(())
    }

    async fn grant_feature_set(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT OR IGNORE INTO client_grants (client_id, space_id, feature_set_id)
             VALUES (?1, ?2, ?3)",
            params![client_id.to_string(), space_id, feature_set_id],
        )?;

        Ok(())
    }

    async fn revoke_feature_set(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM client_grants 
             WHERE client_id = ?1 AND space_id = ?2 AND feature_set_id = ?3",
            params![client_id.to_string(), space_id, feature_set_id],
        )?;

        Ok(())
    }

    async fn get_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT feature_set_id FROM client_grants 
             WHERE client_id = ?1 AND space_id = ?2",
        )?;

        let grants = stmt
            .query_map(params![client_id.to_string(), space_id], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(grants)
    }

    async fn get_all_grants(
        &self,
        client_id: &Uuid,
    ) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT space_id, feature_set_id FROM client_grants 
             WHERE client_id = ?1
             ORDER BY space_id",
        )?;

        let mut grants: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        let rows = stmt.query_map(params![client_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (space_id, feature_set_id) = row?;
            grants
                .entry(space_id)
                .or_default()
                .push(feature_set_id);
        }

        Ok(grants)
    }

    async fn set_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
        feature_set_ids: &[String],
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Remove existing grants for this space
        conn.execute(
            "DELETE FROM client_grants WHERE client_id = ?1 AND space_id = ?2",
            params![client_id.to_string(), space_id],
        )?;

        // Insert new grants
        for feature_set_id in feature_set_ids {
            conn.execute(
                "INSERT INTO client_grants (client_id, space_id, feature_set_id)
                 VALUES (?1, ?2, ?3)",
                params![client_id.to_string(), space_id, feature_set_id],
            )?;
        }

        Ok(())
    }

    async fn has_grants_for_space(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> Result<bool> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM client_grants 
             WHERE client_id = ?1 AND space_id = ?2",
            params![client_id.to_string(), space_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crud_operations() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteInboundMcpClientRepository::new(db);

        // Create
        let client = Client::cursor();
        repo.create(&client).await.unwrap();

        // Read
        let found = repo.get(&client.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Cursor");

        // List
        let all = repo.list().await.unwrap();
        assert_eq!(all.len(), 1);

        // Update
        let mut updated = client.clone();
        updated.name = "Cursor AI".to_string();
        repo.update(&updated).await.unwrap();

        let found = repo.get(&client.id).await.unwrap().unwrap();
        assert_eq!(found.name, "Cursor AI");

        // Delete
        repo.delete(&client.id).await.unwrap();
        let found = repo.get(&client.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_connection_modes() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteInboundMcpClientRepository::new(db);

        // Create with FollowActive
        let client1 = Client::cursor();
        repo.create(&client1).await.unwrap();

        let found = repo.get(&client1.id).await.unwrap().unwrap();
        assert!(matches!(found.connection_mode, ConnectionMode::FollowActive));

        // Create with Locked
        let mut client2 = Client::vscode();
        let space_id = Uuid::new_v4();
        client2.connection_mode = ConnectionMode::Locked { space_id };
        repo.create(&client2).await.unwrap();

        let found = repo.get(&client2.id).await.unwrap().unwrap();
        if let ConnectionMode::Locked { space_id: found_id } = found.connection_mode {
            assert_eq!(found_id, space_id);
        } else {
            panic!("Expected Locked connection mode");
        }
    }
}
