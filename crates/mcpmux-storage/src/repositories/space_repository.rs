//! SQLite implementation of SpaceRepository.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{Space, SpaceRepository};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// SQLite-backed implementation of SpaceRepository.
pub struct SqliteSpaceRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteSpaceRepository {
    /// Create a new SQLite space repository.
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Parse a datetime string to DateTime<Utc>.
    /// Handles both RFC3339 format and SQLite's `datetime('now')` format.
    fn parse_datetime(s: &str) -> DateTime<Utc> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        Utc::now()
    }

    /// Columns selected for every `Space` read. Order must match `map_row`.
    const COLUMNS: &'static str =
        "id, name, icon, description, is_default, sort_order, created_at, updated_at";

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Space> {
        let id_str: String = row.get(0)?;
        Ok(Space {
            id: id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
            name: row.get(1)?,
            icon: row.get(2)?,
            description: row.get(3)?,
            is_default: row.get::<_, i32>(4)? == 1,
            sort_order: row.get(5)?,
            created_at: Self::parse_datetime(&row.get::<_, String>(6)?),
            updated_at: Self::parse_datetime(&row.get::<_, String>(7)?),
        })
    }
}

#[async_trait]
impl SpaceRepository for SqliteSpaceRepository {
    async fn list(&self) -> Result<Vec<Space>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let sql = format!(
            "SELECT {} FROM spaces ORDER BY sort_order ASC, name ASC",
            Self::COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let spaces = stmt
            .query_map([], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(spaces)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Space>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let sql = format!("SELECT {} FROM spaces WHERE id = ?", Self::COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let space = stmt
            .query_row(params![id.to_string()], Self::map_row)
            .optional()?;

        Ok(space)
    }

    async fn create(&self, space: &Space) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let space_id = space.id.to_string();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        conn.execute(
            "INSERT INTO spaces (id, name, icon, description, is_default, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                space_id,
                space.name,
                space.icon,
                space.description,
                if space.is_default { 1 } else { 0 },
                space.sort_order,
                space.created_at.to_rfc3339(),
                space.updated_at.to_rfc3339(),
            ],
        )?;

        // Auto-seed the builtin "Starter" FeatureSet for this Space — a
        // ready-to-use starting point. The id prefix `fs_default_<space>`
        // is preserved for FK-stability across the rename (migration 013).
        // The Starter is the default fallback for folders that aren't
        // explicitly mapped (and rootless/unknown sessions), so it's
        // load-bearing and builtin (can't be deleted).
        conn.execute(
            "INSERT OR IGNORE INTO feature_sets (id, name, description, icon, space_id, feature_set_type, is_builtin, created_at, updated_at)
             VALUES (?1, 'Starter', 'Auto-created with this Space. Unmapped folders fall back to this set — it''s the default toolset for anything you haven''t explicitly mapped. Edit or rename it to change what they get; it can''t be deleted.', '⭐', ?2, 'starter', 1, ?3, ?3)",
            params![
                format!("fs_default_{}", space_id),
                space_id,
                now,
            ],
        )?;

        Ok(())
    }

    async fn update(&self, space: &Space) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let rows_affected = conn.execute(
            "UPDATE spaces
             SET name = ?2, icon = ?3, description = ?4, is_default = ?5, sort_order = ?6, updated_at = ?7
             WHERE id = ?1",
            params![
                space.id.to_string(),
                space.name,
                space.icon,
                space.description,
                if space.is_default { 1 } else { 0 },
                space.sort_order,
                space.updated_at.to_rfc3339(),
            ],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("Space not found: {}", space.id);
        }

        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute("DELETE FROM spaces WHERE id = ?", params![id.to_string()])?;

        Ok(())
    }

    async fn get_default(&self) -> Result<Option<Space>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let sql = format!(
            "SELECT {} FROM spaces WHERE is_default = 1 LIMIT 1",
            Self::COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let space = stmt.query_row([], Self::map_row).optional()?;

        Ok(space)
    }

    async fn set_default(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let tx = conn.unchecked_transaction()?;
        tx.execute("UPDATE spaces SET is_default = 0", [])?;
        let rows_affected = tx.execute(
            "UPDATE spaces SET is_default = 1 WHERE id = ?",
            params![id.to_string()],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("Space not found: {}", id);
        }

        tx.commit()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default space ID created by migration
    const DEFAULT_SPACE_ID: &str = "00000000-0000-0000-0000-000000000001";

    #[tokio::test]
    async fn test_crud_operations() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteSpaceRepository::new(db);

        let initial = repo.list().await.unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].name, "My Space");

        let space = Space::new("Test Space").with_icon("🧪");
        repo.create(&space).await.unwrap();

        let found = repo.get(&space.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Test Space");

        let all = repo.list().await.unwrap();
        assert_eq!(all.len(), 2);

        let mut updated = space.clone();
        updated.name = "Updated Space".to_string();
        repo.update(&updated).await.unwrap();

        let found = repo.get(&space.id).await.unwrap().unwrap();
        assert_eq!(found.name, "Updated Space");

        repo.delete(&space.id).await.unwrap();
        let found = repo.get(&space.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_default_space() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteSpaceRepository::new(db);

        let default = repo.get_default().await.unwrap();
        assert!(default.is_some());
        assert_eq!(default.unwrap().name, "My Space");

        let space2 = Space::new("Space 2");
        repo.create(&space2).await.unwrap();

        repo.set_default(&space2.id).await.unwrap();
        let default = repo.get_default().await.unwrap();
        assert!(default.is_some());
        assert_eq!(default.unwrap().name, "Space 2");

        let default_uuid = Uuid::parse_str(DEFAULT_SPACE_ID).unwrap();
        repo.set_default(&default_uuid).await.unwrap();
        let default = repo.get_default().await.unwrap();
        assert_eq!(default.unwrap().name, "My Space");
    }
}
