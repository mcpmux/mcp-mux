//! SQLite implementation of [`WorkspaceBindingRepository`].
//!
//! Schema after migration 007 (concrete-pointers model):
//!
//! ```text
//! workspace_bindings
//!   id              TEXT PK
//!   workspace_root  TEXT UNIQUE      — routing key, globally unique
//!   space_id        TEXT NOT NULL    — FK → spaces(id)
//!   feature_set_id  TEXT NOT NULL    — FK → feature_sets(id)
//!   created_at      TEXT NOT NULL
//!   updated_at      TEXT NOT NULL
//! ```
//!
//! Longest-prefix matching (used by the resolver) is done in-memory against
//! `list()` since a mcpmux DB is expected to hold O(tens) of bindings.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{longest_prefix_match, WorkspaceBinding, WorkspaceBindingRepository};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

pub struct SqliteWorkspaceBindingRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteWorkspaceBindingRepository {
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

    fn row_to_binding(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceBinding> {
        let id_str: String = row.get(0)?;
        let workspace_root: String = row.get(1)?;
        let space_id_str: String = row.get(2)?;
        let feature_set_id: String = row.get(3)?;
        let created_at: String = row.get(4)?;
        let updated_at: String = row.get(5)?;

        Ok(WorkspaceBinding {
            id: id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
            workspace_root,
            space_id: space_id_str.parse().unwrap_or_else(|_| Uuid::nil()),
            feature_set_id,
            created_at: Self::parse_datetime(&created_at),
            updated_at: Self::parse_datetime(&updated_at),
        })
    }

    const SELECT_COLS: &'static str =
        "id, workspace_root, space_id, feature_set_id, created_at, updated_at";
}

#[async_trait]
impl WorkspaceBindingRepository for SqliteWorkspaceBindingRepository {
    async fn list(&self) -> Result<Vec<WorkspaceBinding>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let sql = format!(
            "SELECT {} FROM workspace_bindings ORDER BY workspace_root",
            Self::SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let bindings = stmt
            .query_map([], Self::row_to_binding)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(bindings)
    }

    async fn list_for_space(&self, space_id: &Uuid) -> Result<Vec<WorkspaceBinding>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let sql = format!(
            "SELECT {} FROM workspace_bindings WHERE space_id = ? ORDER BY workspace_root",
            Self::SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let bindings = stmt
            .query_map(params![space_id.to_string()], Self::row_to_binding)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(bindings)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<WorkspaceBinding>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let sql = format!(
            "SELECT {} FROM workspace_bindings WHERE id = ?",
            Self::SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let binding = stmt
            .query_row(params![id.to_string()], Self::row_to_binding)
            .optional()?;
        Ok(binding)
    }

    async fn create(&self, binding: &WorkspaceBinding) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT INTO workspace_bindings
                (id, workspace_root, space_id, feature_set_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                binding.id.to_string(),
                binding.workspace_root,
                binding.space_id.to_string(),
                binding.feature_set_id,
                binding.created_at.to_rfc3339(),
                binding.updated_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    async fn update(&self, binding: &WorkspaceBinding) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let rows_affected = conn.execute(
            "UPDATE workspace_bindings
             SET workspace_root = ?2, space_id = ?3, feature_set_id = ?4, updated_at = ?5
             WHERE id = ?1",
            params![
                binding.id.to_string(),
                binding.workspace_root,
                binding.space_id.to_string(),
                binding.feature_set_id,
                binding.updated_at.to_rfc3339(),
            ],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("WorkspaceBinding not found: {}", binding.id);
        }

        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "DELETE FROM workspace_bindings WHERE id = ?",
            params![id.to_string()],
        )?;
        Ok(())
    }

    async fn find_longest_prefix_match(
        &self,
        // `space_id` is no longer used for lookup — routing is keyed on root
        // alone and each binding already carries its target space. Kept in
        // the signature for trait compatibility with callers that still hold
        // onto a "caller's space" hint.
        _space_id: &Uuid,
        candidate_roots: &[String],
    ) -> Result<Option<WorkspaceBinding>> {
        if candidate_roots.is_empty() {
            return Ok(None);
        }

        let bindings = self.list().await?;
        if bindings.is_empty() {
            return Ok(None);
        }

        let candidate_strings: Vec<&str> =
            bindings.iter().map(|b| b.workspace_root.as_str()).collect();

        let mut best: Option<&WorkspaceBinding> = None;
        for root in candidate_roots {
            if let Some(winner) = longest_prefix_match(root, candidate_strings.iter().copied()) {
                let winning = bindings
                    .iter()
                    .find(|b| b.workspace_root == winner)
                    .expect("candidate came from bindings");
                if best
                    .map(|b| winning.workspace_root.len() > b.workspace_root.len())
                    .unwrap_or(true)
                {
                    best = Some(winning);
                }
            }
        }

        Ok(best.cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpmux_core::FeatureSet;

    async fn fixture() -> (SqliteWorkspaceBindingRepository, Uuid, String) {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteWorkspaceBindingRepository::new(db.clone());
        let space_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();

        // Seed a real FeatureSet so FK constraints are satisfied.
        let fs = FeatureSet::new_custom("test", space_id.to_string());
        let fs_id = fs.id.clone();
        let now = Utc::now().to_rfc3339();
        {
            let guard = db.lock().await;
            guard
                .connection()
                .execute(
                    "INSERT INTO feature_sets (id, name, feature_set_type, space_id, is_builtin, created_at, updated_at)
                     VALUES (?1, 'test', 'custom', ?2, 0, ?3, ?3)",
                    params![fs.id, space_id.to_string(), now],
                )
                .unwrap();
        }
        (repo, space_id, fs_id)
    }

    #[tokio::test]
    async fn test_crud_round_trip() {
        let (repo, space_id, fs_id) = fixture().await;
        let root = if cfg!(windows) { "d:\\proj" } else { "/proj" };
        let binding = WorkspaceBinding::new(root, space_id, fs_id.clone());
        repo.create(&binding).await.unwrap();

        let got = repo.get(&binding.id).await.unwrap().unwrap();
        assert_eq!(got.workspace_root, root);
        assert_eq!(got.space_id, space_id);
        assert_eq!(got.feature_set_id, fs_id);
    }

    #[tokio::test]
    async fn test_list_for_space_filters_by_pointer() {
        let (repo, space_id, fs_id) = fixture().await;
        let root = if cfg!(windows) { "d:\\proj" } else { "/proj" };
        repo.create(&WorkspaceBinding::new(root, space_id, fs_id))
            .await
            .unwrap();

        let hits = repo.list_for_space(&space_id).await.unwrap();
        assert_eq!(hits.len(), 1);

        let other = Uuid::new_v4();
        let hits_other = repo.list_for_space(&other).await.unwrap();
        assert!(hits_other.is_empty());
    }

    #[tokio::test]
    async fn test_longest_prefix_match_picks_nested_root() {
        let (repo, space_id, fs_id) = fixture().await;
        let (outer, inner) = if cfg!(windows) {
            ("d:\\work", "d:\\work\\proj")
        } else {
            ("/work", "/work/proj")
        };
        repo.create(&WorkspaceBinding::new(outer, space_id, fs_id.clone()))
            .await
            .unwrap();
        let b_inner = WorkspaceBinding::new(inner, space_id, fs_id);
        repo.create(&b_inner).await.unwrap();

        let deep = if cfg!(windows) {
            "d:\\work\\proj\\src"
        } else {
            "/work/proj/src"
        };
        let hit = repo
            .find_longest_prefix_match(&space_id, &[deep.to_string()])
            .await
            .unwrap()
            .expect("match");
        assert_eq!(hit.workspace_root, inner);
    }
}
