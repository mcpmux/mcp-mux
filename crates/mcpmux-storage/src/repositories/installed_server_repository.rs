//! SQLite implementation of InstalledServerRepository.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{InstallationSource, InstalledServer, InstalledServerRepository};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// SQLite-backed implementation of InstalledServerRepository.
pub struct SqliteInstalledServerRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteInstalledServerRepository {
    /// Create a new SQLite installed server repository.
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

    /// Parse JSON string to HashMap.
    fn parse_json_map(s: Option<String>) -> HashMap<String, String> {
        s.and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Parse JSON string to Vec.
    fn parse_json_vec(s: Option<String>) -> Vec<String> {
        s.and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Serialize HashMap to JSON string.
    fn serialize_json_map(map: &HashMap<String, String>) -> String {
        serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string())
    }

    /// Serialize Vec to JSON string.
    fn serialize_json_vec(vec: &[String]) -> String {
        serde_json::to_string(vec).unwrap_or_else(|_| "[]".to_string())
    }

    /// Serialize InstallationSource to database string format.
    /// Format: "registry" | "user_config:/path/to/file.json" | "manual_entry"
    fn serialize_source(source: &InstallationSource) -> String {
        match source {
            InstallationSource::Registry => "registry".to_string(),
            InstallationSource::UserConfig { file_path } => {
                format!("user_config:{}", file_path.display())
            }
            InstallationSource::ManualEntry => "manual_entry".to_string(),
        }
    }

    /// Parse InstallationSource from database string format.
    fn parse_source(s: Option<String>) -> InstallationSource {
        match s.as_deref() {
            Some("registry") | None => InstallationSource::Registry,
            Some("manual_entry") => InstallationSource::ManualEntry,
            Some(s) if s.starts_with("user_config:") => {
                let path = s.strip_prefix("user_config:").unwrap_or("");
                InstallationSource::UserConfig {
                    file_path: PathBuf::from(path),
                }
            }
            _ => InstallationSource::Registry,
        }
    }

    /// Standard column list for SELECT queries
    const SELECT_COLUMNS: &'static str =
        "id, space_id, server_id, server_name, cached_definition, input_values, enabled, env_overrides,
         args_append, extra_headers, oauth_connected, created_at, updated_at, source";

    /// Map a row to InstalledServer
    fn map_row(row: &rusqlite::Row) -> rusqlite::Result<InstalledServer> {
        let id: String = row.get(0)?;
        let space_id: String = row.get(1)?;
        let server_id: String = row.get(2)?;
        let server_name: Option<String> = row.get(3)?;
        let cached_definition: Option<String> = row.get(4)?;
        let input_values: Option<String> = row.get(5)?;
        let enabled: bool = row.get(6)?;
        let env_overrides: Option<String> = row.get(7)?;
        let args_append: Option<String> = row.get(8)?;
        let extra_headers: Option<String> = row.get(9)?;
        let oauth_connected: bool = row.get(10)?;
        let created_at: String = row.get(11)?;
        let updated_at: String = row.get(12)?;
        let source: Option<String> = row.get(13)?;

        Ok(InstalledServer {
            id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
            space_id,
            server_id,
            server_name,
            cached_definition,
            input_values: Self::parse_json_map(input_values),
            enabled,
            env_overrides: Self::parse_json_map(env_overrides),
            args_append: Self::parse_json_vec(args_append),
            extra_headers: Self::parse_json_map(extra_headers),
            oauth_connected,
            source: Self::parse_source(source),
            created_at: Self::parse_datetime(&created_at),
            updated_at: Self::parse_datetime(&updated_at),
        })
    }
}

#[async_trait]
impl InstalledServerRepository for SqliteInstalledServerRepository {
    async fn list(&self) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let servers = stmt
            .query_map([], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(servers)
    }

    async fn list_for_space(&self, space_id: &str) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE space_id = ?1 ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let servers = stmt
            .query_map([space_id], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(servers)
    }

    async fn list_by_source_file(
        &self,
        file_path: &std::path::Path,
    ) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Source format is "user_config:/path/to/file.json"
        let source_prefix = format!("user_config:{}", file_path.display());

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE source = ?1 ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let servers = stmt
            .query_map([&source_prefix], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(servers)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE id = ?1",
            Self::SELECT_COLUMNS
        ))?;

        let server = stmt.query_row([id.to_string()], Self::map_row).optional()?;

        Ok(server)
    }

    async fn get_by_server_id(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> Result<Option<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE space_id = ?1 AND server_id = ?2",
            Self::SELECT_COLUMNS
        ))?;

        let server = stmt
            .query_row([space_id, server_id], Self::map_row)
            .optional()?;

        Ok(server)
    }

    async fn install(&self, server: &InstalledServer) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT INTO installed_servers
             (id, space_id, server_id, server_name, cached_definition, input_values, enabled, env_overrides,
              args_append, extra_headers, oauth_connected, created_at, updated_at, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                server.id.to_string(),
                server.space_id,
                server.server_id,
                server.server_name,
                server.cached_definition,
                Self::serialize_json_map(&server.input_values),
                server.enabled,
                Self::serialize_json_map(&server.env_overrides),
                Self::serialize_json_vec(&server.args_append),
                Self::serialize_json_map(&server.extra_headers),
                server.oauth_connected,
                server.created_at.to_rfc3339(),
                server.updated_at.to_rfc3339(),
                Self::serialize_source(&server.source),
            ],
        )?;
        Ok(())
    }

    async fn update(&self, server: &InstalledServer) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "UPDATE installed_servers
             SET server_name = ?2, cached_definition = ?3, input_values = ?4, enabled = ?5,
                 env_overrides = ?6, args_append = ?7, extra_headers = ?8, oauth_connected = ?9,
                 updated_at = ?10, source = ?11
             WHERE id = ?1",
            params![
                server.id.to_string(),
                server.server_name,
                server.cached_definition,
                Self::serialize_json_map(&server.input_values),
                server.enabled,
                Self::serialize_json_map(&server.env_overrides),
                Self::serialize_json_vec(&server.args_append),
                Self::serialize_json_map(&server.extra_headers),
                server.oauth_connected,
                Utc::now().to_rfc3339(),
                Self::serialize_source(&server.source),
            ],
        )?;
        Ok(())
    }

    async fn uninstall(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM installed_servers WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    async fn list_enabled(&self, space_id: &str) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE space_id = ?1 AND enabled = 1 ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let servers = stmt
            .query_map([space_id], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(servers)
    }

    async fn list_enabled_all(&self) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE enabled = 1 ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let servers = stmt
            .query_map([], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(servers)
    }

    async fn set_enabled(&self, id: &Uuid, enabled: bool) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "UPDATE installed_servers SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), enabled, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    async fn set_oauth_connected(&self, id: &Uuid, connected: bool) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "UPDATE installed_servers SET oauth_connected = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), connected, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    async fn update_inputs(
        &self,
        id: &Uuid,
        input_values: std::collections::HashMap<String, String>,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let input_json = Self::serialize_json_map(&input_values);

        tracing::debug!(
            "[InstalledServerRepo] Updating inputs for {}: {} values, JSON: {:?}",
            id,
            input_values.len(),
            input_json
        );

        conn.execute(
            "UPDATE installed_servers SET input_values = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), input_json, Utc::now().to_rfc3339()],
        )?;

        tracing::debug!(
            "[InstalledServerRepo] Successfully updated inputs for {}",
            id
        );
        Ok(())
    }

    async fn update_cached_definition(
        &self,
        id: &Uuid,
        server_name: Option<String>,
        cached_definition: Option<String>,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "UPDATE installed_servers SET server_name = ?2, cached_definition = ?3, updated_at = ?4 WHERE id = ?1",
            params![
                id.to_string(),
                server_name,
                cached_definition,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }
}
