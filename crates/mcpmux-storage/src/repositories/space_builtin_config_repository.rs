//! SQLite implementation of [`SpaceBuiltinConfigRepository`].
//!
//! Stores only deviations from the default: a missing server row means "use
//! the descriptor's `default_enabled`", and a tool is enabled unless an
//! explicit `enabled = 0` row exists for it.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mcpmux_core::SpaceBuiltinConfigRepository;
use rusqlite::params;
use tokio::sync::Mutex;

use crate::Database;

pub struct SqliteSpaceBuiltinConfigRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteSpaceBuiltinConfigRepository {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SpaceBuiltinConfigRepository for SqliteSpaceBuiltinConfigRepository {
    async fn server_enabled_override(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> Result<Option<bool>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let res = conn.query_row(
            "SELECT enabled FROM space_builtin_servers WHERE space_id = ?1 AND server_id = ?2",
            params![space_id, server_id],
            |row| row.get::<_, i64>(0),
        );
        match res {
            Ok(v) => Ok(Some(v != 0)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn disabled_tools(&self, space_id: &str, server_id: &str) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(
            "SELECT tool_name FROM space_builtin_tools \
             WHERE space_id = ?1 AND server_id = ?2 AND enabled = 0",
        )?;
        let rows = stmt
            .query_map(params![space_id, server_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    async fn set_server_enabled(
        &self,
        space_id: &str,
        server_id: &str,
        enabled: bool,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO space_builtin_servers (space_id, server_id, enabled, updated_at) \
             VALUES (?1, ?2, ?3, datetime('now')) \
             ON CONFLICT(space_id, server_id) \
             DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![space_id, server_id, enabled as i64],
        )?;
        Ok(())
    }

    async fn set_tool_enabled(
        &self,
        space_id: &str,
        server_id: &str,
        tool_name: &str,
        enabled: bool,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO space_builtin_tools (space_id, server_id, tool_name, enabled, updated_at) \
             VALUES (?1, ?2, ?3, ?4, datetime('now')) \
             ON CONFLICT(space_id, server_id, tool_name) \
             DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![space_id, server_id, tool_name, enabled as i64],
        )?;
        Ok(())
    }
}
