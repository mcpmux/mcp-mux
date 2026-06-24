//! SQLite implementation of [`WorkspaceAppearanceRepository`].

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{WorkspaceAppearance, WorkspaceAppearanceRepository};
use rusqlite::params;
use tokio::sync::Mutex;

use crate::Database;

#[allow(dead_code)]
pub struct SqliteWorkspaceAppearanceRepository {
    db: Arc<Mutex<Database>>,
}

#[allow(dead_code)]
impl SqliteWorkspaceAppearanceRepository {
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
impl WorkspaceAppearanceRepository for SqliteWorkspaceAppearanceRepository {
    async fn list(&self) -> Result<Vec<WorkspaceAppearance>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(
            "SELECT workspace_root, icon, updated_at
             FROM workspace_appearances
             ORDER BY workspace_root",
        )?;
        let rows = stmt.query_map([], |row| {
            let updated_at: String = row.get(2)?;
            Ok(WorkspaceAppearance {
                workspace_root: row.get(0)?,
                icon: row.get(1)?,
                updated_at: Self::parse_datetime(&updated_at),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    async fn get(&self, workspace_root: &str) -> Result<Option<WorkspaceAppearance>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let mut stmt = conn.prepare(
            "SELECT workspace_root, icon, updated_at
             FROM workspace_appearances
             WHERE workspace_root = ?1",
        )?;
        let mut rows = stmt.query(params![workspace_root])?;
        if let Some(row) = rows.next()? {
            let updated_at: String = row.get(2)?;
            return Ok(Some(WorkspaceAppearance {
                workspace_root: row.get(0)?,
                icon: row.get(1)?,
                updated_at: Self::parse_datetime(&updated_at),
            }));
        }
        Ok(None)
    }

    async fn upsert(&self, appearance: &WorkspaceAppearance) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO workspace_appearances (workspace_root, icon, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(workspace_root) DO UPDATE SET
                 icon = excluded.icon,
                 updated_at = excluded.updated_at",
            params![
                appearance.workspace_root,
                appearance.icon,
                appearance.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    async fn delete(&self, workspace_root: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "DELETE FROM workspace_appearances WHERE workspace_root = ?1",
            params![workspace_root],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpmux_core::normalize_workspace_root;

    #[tokio::test]
    async fn test_upsert_get_delete_round_trip() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteWorkspaceAppearanceRepository::new(db);
        let normalized_root = normalize_workspace_root("file:///home/user/my%20project");

        let mut created = WorkspaceAppearance::new(normalized_root.clone(), "📁");
        repo.upsert(&created).await.unwrap();

        let fetched = repo.get(&normalized_root).await.unwrap().unwrap();
        assert_eq!(fetched.workspace_root, normalized_root);
        assert_eq!(fetched.icon, "📁");

        created.icon = "🧪".to_string();
        created.updated_at = Utc::now();
        repo.upsert(&created).await.unwrap();
        let updated = repo.get(&created.workspace_root).await.unwrap().unwrap();
        assert_eq!(updated.icon, "🧪");

        let listed = repo.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].workspace_root, created.workspace_root);

        repo.delete(&created.workspace_root).await.unwrap();
        assert!(repo.get(&created.workspace_root).await.unwrap().is_none());
    }
}
