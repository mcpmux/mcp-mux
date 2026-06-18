//! SQLite implementation of SpaceBaseDirRepository.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{path_is_within, SpaceBaseDir, SpaceBaseDirRepository};
use rusqlite::params;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// SQLite-backed implementation of [`SpaceBaseDirRepository`].
pub struct SqliteSpaceBaseDirRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteSpaceBaseDirRepository {
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

    /// Columns selected for every read. Order must match `map_row`.
    const COLUMNS: &'static str = "id, space_id, path, created_at";

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpaceBaseDir> {
        Ok(SpaceBaseDir {
            id: row.get(0)?,
            space_id: row.get(1)?,
            path: row.get(2)?,
            created_at: Self::parse_datetime(&row.get::<_, String>(3)?),
        })
    }
}

#[async_trait]
impl SpaceBaseDirRepository for SqliteSpaceBaseDirRepository {
    async fn list_all(&self) -> Result<Vec<SpaceBaseDir>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM space_base_dirs ORDER BY path ASC",
            Self::COLUMNS
        ))?;
        let rows = stmt
            .query_map([], Self::map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    async fn list_by_space(&self, space_id: &Uuid) -> Result<Vec<SpaceBaseDir>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM space_base_dirs WHERE space_id = ? ORDER BY path ASC",
            Self::COLUMNS
        ))?;
        let rows = stmt
            .query_map(params![space_id.to_string()], Self::map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    async fn add(&self, space_id: &Uuid, path: &str) -> Result<SpaceBaseDir> {
        if path.trim().is_empty() {
            anyhow::bail!("Base directory path is empty");
        }
        let row = SpaceBaseDir {
            id: Uuid::new_v4().to_string(),
            space_id: space_id.to_string(),
            path: path.to_string(),
            created_at: Utc::now(),
        };
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO space_base_dirs (id, space_id, path, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![row.id, row.space_id, row.path, row.created_at.to_rfc3339()],
        )
        .map_err(|e| {
            // Turn the UNIQUE(path) collision into an actionable message — the
            // folder already belongs to some Space (possibly this one).
            if e.to_string().to_lowercase().contains("unique") {
                anyhow::anyhow!("That folder is already a base directory of a space: {path}")
            } else {
                anyhow::Error::from(e)
            }
        })?;
        Ok(row)
    }

    async fn remove(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute("DELETE FROM space_base_dirs WHERE id = ?", params![id])?;
        Ok(())
    }

    async fn find_space_for_root(&self, root: &str) -> Result<Option<Uuid>> {
        if root.trim().is_empty() {
            return Ok(None);
        }
        // Few base dirs in practice — load them and pick the longest prefix
        // match in Rust (reuses the byte-proven `path_is_within`). The db lock
        // is taken and released inside `list_all`, so there's no double-lock.
        let all = self.list_all().await?;
        let best = all
            .iter()
            .filter(|bd| path_is_within(root, &bd.path))
            .max_by_key(|bd| bd.path.len());
        Ok(best.and_then(|bd| bd.space_id.parse::<Uuid>().ok()))
    }
}
