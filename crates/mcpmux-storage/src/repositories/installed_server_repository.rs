//! SQLite implementation of InstalledServerRepository.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{InstallationSource, InstalledServer, InstalledServerRepository};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    crypto::{binding, FieldEncryptor},
    Database,
};

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

    /// Encrypt one settings column (input values, env overrides, appended
    /// args, extra headers) as JSON, bound to its column and row. They
    /// routinely hold secrets (API keys, `Authorization` headers,
    /// `--api-key=…` arguments, tokens in env).
    fn encrypt_json<T: serde::Serialize>(
        &self,
        column: &str,
        id: &str,
        value: &T,
    ) -> Result<String> {
        let json = serde_json::to_string(value)?;
        self.encryptor
            .encrypt_bound(&json, &binding::installed_server(column, id))
            .map_err(|e| anyhow::anyhow!("Failed to encrypt {column}: {e}"))
    }

    /// Decrypt one settings column.
    ///
    /// Three cases, kept distinct so a real failure can't masquerade as an
    /// empty config (which would silently launch a server with all its
    /// secrets missing):
    ///   * `None` / empty column → the default (empty).
    ///   * Decrypts cleanly (bound, or legacy unbound) → parse the JSON (a
    ///     parse failure here is corruption → error).
    ///   * Decrypt fails → it may be a legacy *unencrypted* row, so try a
    ///     plaintext-JSON parse; if THAT also fails the data is neither
    ///     decryptable nor valid plaintext (wrong master key, tampered or
    ///     moved ciphertext) → a hard error rather than an empty value.
    fn decrypt_json<T: serde::de::DeserializeOwned + Default>(
        &self,
        column: &str,
        id: &str,
        stored: Option<String>,
    ) -> Result<T> {
        let Some(data) = stored.filter(|d| !d.trim().is_empty()) else {
            return Ok(T::default());
        };
        let context = binding::installed_server(column, id);
        if let Ok(json) = self.encryptor.decrypt_bound(&data, &context) {
            return serde_json::from_str(&json)
                .map_err(|e| anyhow::anyhow!("Corrupt decrypted {column} (not JSON): {e}"));
        }
        if FieldEncryptor::is_bound(&data) {
            anyhow::bail!(
                "Failed to decrypt {column} (wrong master key, or the value was moved or tampered with)"
            );
        }
        serde_json::from_str(&data).map_err(|e| {
            anyhow::anyhow!(
                "Failed to decrypt {column} and data is not valid plaintext JSON \
                 (wrong master key or tampered ciphertext): {e}"
            )
        })
    }

    /// Bring settings columns written by earlier versions up to date: encrypt
    /// plaintext JSON, and re-encrypt unbound ciphertexts bound to their row.
    /// Values that can't be read are left untouched. Run at startup; returns
    /// how many rows changed.
    pub async fn encrypt_plaintext_rows(&self) -> Result<usize> {
        const COLUMNS: [&str; 4] = [
            "input_values",
            "env_overrides",
            "args_append",
            "extra_headers",
        ];
        let db = self.db.lock().await;
        let conn = db.connection();
        let rows: Vec<(String, [Option<String>; 4])> = {
            let mut stmt = conn.prepare(
                "SELECT id, input_values, env_overrides, args_append, extra_headers
                 FROM installed_servers",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    [row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?],
                ))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };

        let mut changed = 0;
        for (id, columns) in rows {
            let mut updated = columns.clone();
            for (column, value) in COLUMNS.iter().zip(updated.iter_mut()) {
                let Some(value) = value.as_mut() else {
                    continue;
                };
                if value.trim().is_empty() || FieldEncryptor::is_bound(value) {
                    continue;
                }
                let plaintext = match self.encryptor.decrypt(value) {
                    Ok(plaintext) => plaintext,
                    Err(_) if serde_json::from_str::<serde_json::Value>(value).is_ok() => {
                        value.clone()
                    }
                    Err(_) => continue,
                };
                *value = self
                    .encryptor
                    .encrypt_bound(&plaintext, &binding::installed_server(column, &id))
                    .map_err(|e| anyhow::anyhow!("Failed to encrypt server {id}: {e}"))?;
            }
            if updated != columns {
                conn.execute(
                    "UPDATE installed_servers
                     SET input_values = ?2, env_overrides = ?3, args_append = ?4, extra_headers = ?5
                     WHERE id = ?1",
                    params![id, updated[0], updated[1], updated[2], updated[3]],
                )?;
                changed += 1;
            }
        }
        Ok(changed)
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
        })
    }

    /// Build InstalledServer from extracted row data (needs &self for decryption).
    fn build_server(&self, row: RawServerRow) -> Result<InstalledServer> {
        let input_values = self
            .decrypt_json("input_values", &row.id, row.input_values)
            .map_err(|e| anyhow::anyhow!("server {}: {}", row.server_id, e))?;
        let with_server = |e: anyhow::Error| anyhow::anyhow!("server {}: {}", row.server_id, e);
        let env_overrides = self
            .decrypt_json("env_overrides", &row.id, row.env_overrides)
            .map_err(with_server)?;
        let args_append = self
            .decrypt_json("args_append", &row.id, row.args_append)
            .map_err(with_server)?;
        let extra_headers = self
            .decrypt_json("extra_headers", &row.id, row.extra_headers)
            .map_err(with_server)?;
        Ok(InstalledServer {
            id: Uuid::parse_str(&row.id).unwrap_or_else(|_| Uuid::new_v4()),
            space_id: row.space_id,
            server_id: row.server_id,
            server_name: row.server_name,
            cached_definition: row.cached_definition,
            input_values,
            enabled: row.enabled,
            env_overrides,
            args_append,
            extra_headers,
            oauth_connected: row.oauth_connected,
            source: Self::parse_source(row.source),
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

        let server_row_id = server.id.to_string();
        let encrypted_inputs =
            self.encrypt_json("input_values", &server_row_id, &server.input_values)?;

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
                encrypted_inputs,
                server.enabled,
                self.encrypt_json("env_overrides", &server_row_id, &server.env_overrides)?,
                self.encrypt_json("args_append", &server_row_id, &server.args_append)?,
                self.encrypt_json("extra_headers", &server_row_id, &server.extra_headers)?,
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

        let server_row_id = server.id.to_string();
        let encrypted_inputs =
            self.encrypt_json("input_values", &server_row_id, &server.input_values)?;

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
                encrypted_inputs,
                server.enabled,
                self.encrypt_json("env_overrides", &server_row_id, &server.env_overrides)?,
                self.encrypt_json("args_append", &server_row_id, &server.args_append)?,
                self.encrypt_json("extra_headers", &server_row_id, &server.extra_headers)?,
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

        let encrypted_inputs = self.encrypt_json("input_values", &id.to_string(), &input_values)?;

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
