//! SQLite implementation of InstalledServerRepository.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{
    DefaultParamsStrategy, InstallationSource, InstalledServer, InstalledServerRepository,
    UpdatePolicy,
};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{crypto::FieldEncryptor, Database};

/// Raw row data extracted from SQLite before decryption.
struct RawServerRow {
    id: String,
    space_id: String,
    server_id: String,
    server_name: Option<String>,
    cached_definition: Option<String>,
    input_values: Option<String>,
    enabled: bool,
    env_overrides: Option<String>,
    args_append: Option<String>,
    extra_headers: Option<String>,
    oauth_connected: bool,
    created_at: String,
    updated_at: String,
    source: Option<String>,
    cloned_from: Option<String>,
    display_name_override: Option<String>,
    default_params: Option<String>,
    update_policy: String,
    pinned_version: Option<String>,
    latest_available_version: Option<String>,
    version_checked_at: Option<String>,
    default_params_strategy: Option<String>,
    current_version: Option<String>,
}

/// SQLite-backed implementation of InstalledServerRepository.
pub struct SqliteInstalledServerRepository {
    db: Arc<Mutex<Database>>,
    encryptor: Arc<FieldEncryptor>,
}

impl SqliteInstalledServerRepository {
    /// Create a new SQLite installed server repository.
    pub fn new(db: Arc<Mutex<Database>>, encryptor: Arc<FieldEncryptor>) -> Self {
        Self { db, encryptor }
    }

    /// Encrypt input values for storage.
    fn encrypt_input_values(&self, values: &HashMap<String, String>) -> Result<String> {
        let json = Self::serialize_json_map(values);
        self.encryptor
            .encrypt(&json)
            .map_err(|e| anyhow::anyhow!("Failed to encrypt input values: {}", e))
    }

    /// Decrypt input values from storage.
    ///
    /// Three cases, kept distinct so a real failure can't masquerade as an
    /// empty config (which would silently launch a server with all its
    /// secrets missing):
    ///   * `None` / empty column → no input values (`Ok(empty)`).
    ///   * Decrypts cleanly → parse the plaintext JSON (a parse failure here
    ///     is corruption → error).
    ///   * Decrypt fails → it may be a legacy *unencrypted* row, so try a
    ///     plaintext-JSON parse; if THAT also fails the data is neither
    ///     decryptable nor valid plaintext (wrong master key or tampered
    ///     ciphertext) → propagate a hard error rather than returning empty.
    fn decrypt_input_values(&self, stored: Option<String>) -> Result<HashMap<String, String>> {
        let Some(data) = stored else {
            return Ok(HashMap::new());
        };
        if data.trim().is_empty() {
            return Ok(HashMap::new());
        }
        // Try decrypting first (new encrypted format).
        if let Ok(json) = self.encryptor.decrypt(&data) {
            return serde_json::from_str(&json)
                .map_err(|e| anyhow::anyhow!("Corrupt decrypted input values (not JSON): {}", e));
        }
        // Fallback: legacy unencrypted row stored as plaintext JSON.
        serde_json::from_str(&data).map_err(|e| {
            anyhow::anyhow!(
                "Failed to decrypt input values and data is not valid plaintext JSON \
                 (wrong master key or tampered ciphertext): {}",
                e
            )
        })
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

    /// Parse JSON string to a `HashMap<String, Value>` (for `default_params`).
    fn parse_json_value_map(s: Option<String>) -> HashMap<String, Value> {
        s.and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Serialize a `HashMap<String, Value>` to JSON string (for `default_params`).
    fn serialize_json_value_map(map: &HashMap<String, Value>) -> String {
        serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string())
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
         args_append, extra_headers, oauth_connected, created_at, updated_at, source, cloned_from,
         display_name_override, default_params, update_policy, pinned_version,
         latest_available_version, version_checked_at, default_params_strategy, current_version";

    /// Extract raw row data (used in the closure passed to rusqlite).
    fn extract_row(row: &rusqlite::Row) -> rusqlite::Result<RawServerRow> {
        Ok(RawServerRow {
            id: row.get(0)?,
            space_id: row.get(1)?,
            server_id: row.get(2)?,
            server_name: row.get(3)?,
            cached_definition: row.get(4)?,
            input_values: row.get(5)?,
            enabled: row.get(6)?,
            env_overrides: row.get(7)?,
            args_append: row.get(8)?,
            extra_headers: row.get(9)?,
            oauth_connected: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            source: row.get(13)?,
            cloned_from: row.get(14)?,
            display_name_override: row.get(15)?,
            default_params: row.get(16)?,
            update_policy: row.get(17)?,
            pinned_version: row.get(18)?,
            latest_available_version: row.get(19)?,
            version_checked_at: row.get(20)?,
            default_params_strategy: row.get(21)?,
            current_version: row.get(22)?,
        })
    }

    /// Build InstalledServer from extracted row data (needs &self for decryption).
    fn build_server(&self, row: RawServerRow) -> Result<InstalledServer> {
        let input_values = self
            .decrypt_input_values(row.input_values)
            .map_err(|e| anyhow::anyhow!("server {}: {}", row.server_id, e))?;
        Ok(InstalledServer {
            id: Uuid::parse_str(&row.id).unwrap_or_else(|_| Uuid::new_v4()),
            space_id: row.space_id,
            server_id: row.server_id,
            server_name: row.server_name,
            cached_definition: row.cached_definition,
            input_values,
            enabled: row.enabled,
            env_overrides: Self::parse_json_map(row.env_overrides),
            args_append: Self::parse_json_vec(row.args_append),
            extra_headers: Self::parse_json_map(row.extra_headers),
            default_params: Self::parse_json_value_map(row.default_params),
            default_params_strategy: row
                .default_params_strategy
                .as_deref()
                .map(DefaultParamsStrategy::from_db_str)
                .unwrap_or_default(),
            oauth_connected: row.oauth_connected,
            source: Self::parse_source(row.source),
            cloned_from: row.cloned_from,
            display_name_override: row.display_name_override,
            update_policy: UpdatePolicy::from_db_str(&row.update_policy),
            pinned_version: row.pinned_version,
            latest_available_version: row.latest_available_version,
            current_version: row.current_version,
            version_checked_at: row.version_checked_at.as_deref().map(Self::parse_datetime),
            created_at: Self::parse_datetime(&row.created_at),
            updated_at: Self::parse_datetime(&row.updated_at),
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

        let rows: Vec<_> = stmt
            .query_map([], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_server(r)).collect()
    }

    async fn list_for_space(&self, space_id: &str) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE space_id = ?1 ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let rows: Vec<_> = stmt
            .query_map([space_id], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_server(r)).collect()
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

        let rows: Vec<_> = stmt
            .query_map([&source_prefix], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_server(r)).collect()
    }

    async fn get(&self, id: &Uuid) -> Result<Option<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE id = ?1",
            Self::SELECT_COLUMNS
        ))?;

        let row = stmt
            .query_row([id.to_string()], Self::extract_row)
            .optional()?;

        row.map(|r| self.build_server(r)).transpose()
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

        let row = stmt
            .query_row([space_id, server_id], Self::extract_row)
            .optional()?;

        row.map(|r| self.build_server(r)).transpose()
    }

    async fn install(&self, server: &InstalledServer) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let encrypted_inputs = self.encrypt_input_values(&server.input_values)?;

        conn.execute(
            "INSERT INTO installed_servers
             (id, space_id, server_id, server_name, cached_definition, input_values, enabled, env_overrides,
              args_append, extra_headers, oauth_connected, created_at, updated_at, source, cloned_from,
              display_name_override, default_params, update_policy, pinned_version,
              latest_available_version, version_checked_at, default_params_strategy, current_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            params![
                server.id.to_string(),
                server.space_id,
                server.server_id,
                server.server_name,
                server.cached_definition,
                encrypted_inputs,
                server.enabled,
                Self::serialize_json_map(&server.env_overrides),
                Self::serialize_json_vec(&server.args_append),
                Self::serialize_json_map(&server.extra_headers),
                server.oauth_connected,
                server.created_at.to_rfc3339(),
                server.updated_at.to_rfc3339(),
                Self::serialize_source(&server.source),
                server.cloned_from,
                server.display_name_override,
                Self::serialize_json_value_map(&server.default_params),
                server.update_policy.as_db_str(),
                server.pinned_version,
                server.latest_available_version,
                server.version_checked_at.map(|dt| dt.to_rfc3339()),
                server.default_params_strategy.as_db_str(),
                server.current_version,
            ],
        )?;
        Ok(())
    }

    async fn update(&self, server: &InstalledServer) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let encrypted_inputs = self.encrypt_input_values(&server.input_values)?;

        conn.execute(
            "UPDATE installed_servers
             SET server_name = ?2, cached_definition = ?3, input_values = ?4, enabled = ?5,
                 env_overrides = ?6, args_append = ?7, extra_headers = ?8, oauth_connected = ?9,
                 updated_at = ?10, source = ?11, cloned_from = ?12, display_name_override = ?13,
                 default_params = ?14, update_policy = ?15, pinned_version = ?16,
                 latest_available_version = ?17, version_checked_at = ?18,
                 default_params_strategy = ?19, current_version = ?20
             WHERE id = ?1",
            params![
                server.id.to_string(),
                server.server_name,
                server.cached_definition,
                encrypted_inputs,
                server.enabled,
                Self::serialize_json_map(&server.env_overrides),
                Self::serialize_json_vec(&server.args_append),
                Self::serialize_json_map(&server.extra_headers),
                server.oauth_connected,
                Utc::now().to_rfc3339(),
                Self::serialize_source(&server.source),
                server.cloned_from,
                server.display_name_override,
                Self::serialize_json_value_map(&server.default_params),
                server.update_policy.as_db_str(),
                server.pinned_version,
                server.latest_available_version,
                server.version_checked_at.map(|dt| dt.to_rfc3339()),
                server.default_params_strategy.as_db_str(),
                server.current_version,
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

        let rows: Vec<_> = stmt
            .query_map([space_id], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_server(r)).collect()
    }

    async fn list_enabled_all(&self) -> Result<Vec<InstalledServer>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM installed_servers WHERE enabled = 1 ORDER BY created_at DESC",
            Self::SELECT_COLUMNS
        ))?;

        let rows: Vec<_> = stmt
            .query_map([], Self::extract_row)?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter().map(|r| self.build_server(r)).collect()
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

        let encrypted_inputs = self.encrypt_input_values(&input_values)?;

        tracing::debug!(
            "[InstalledServerRepo] Updating inputs for {}: {} values (encrypted)",
            id,
            input_values.len(),
        );

        conn.execute(
            "UPDATE installed_servers SET input_values = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), encrypted_inputs, Utc::now().to_rfc3339()],
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
