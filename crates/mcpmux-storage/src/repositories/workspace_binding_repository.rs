//! SQLite implementation of [`WorkspaceBindingRepository`].
//!
//! Schema after migration 012 (multi-FS bindings):
//!
//! ```text
//! workspace_bindings
//!   id              TEXT PK
//!   workspace_root  TEXT UNIQUE      — routing key, globally unique
//!   space_id        TEXT NOT NULL    — FK → spaces(id)
//!   created_at      TEXT NOT NULL
//!   updated_at      TEXT NOT NULL
//!
//! workspace_binding_feature_sets   (junction)
//!   binding_id      TEXT NOT NULL    — FK → workspace_bindings(id)
//!   feature_set_id  TEXT NOT NULL    — FK → feature_sets(id)
//!   sort_order      INTEGER          — UI render order; resolver-irrelevant
//!   PK (binding_id, feature_set_id)
//! ```
//!
//! Each binding owns ≥ 1 FeatureSet. The repository surfaces them as
//! `WorkspaceBinding.feature_set_ids` (sorted by `sort_order`) so callers
//! can stop reasoning about the join.
//!
//! Longest-prefix matching (used by the resolver) is done in-memory against
//! `list()` since a mcpmux DB is expected to hold O(tens) of bindings.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{longest_prefix_match, WorkspaceBinding, WorkspaceBindingRepository};
use rusqlite::params;
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

    /// Map a row from `workspace_bindings` (columns in the order of
    /// [`Self::SELECT_COLS`]) to a partially-populated [`WorkspaceBinding`]
    /// — `feature_set_ids` is filled by the caller from the junction.
    fn row_to_binding_no_fs(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceBinding> {
        let id_str: String = row.get(0)?;
        let workspace_root: String = row.get(1)?;
        let label: Option<String> = row.get(2)?;
        let space_id_str: String = row.get(3)?;
        let created_at: String = row.get(4)?;
        let updated_at: String = row.get(5)?;

        Ok(WorkspaceBinding {
            id: id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
            workspace_root,
            label,
            space_id: space_id_str.parse().unwrap_or_else(|_| Uuid::nil()),
            feature_set_ids: Vec::new(), // filled in by caller
            created_at: Self::parse_datetime(&created_at),
            updated_at: Self::parse_datetime(&updated_at),
        })
    }

    /// Bulk-load `(binding_id, feature_set_ids)` from the junction for the
    /// given binding ids, ordered by `sort_order` then `feature_set_id`
    /// (stable, so the UI doesn't shuffle).
    fn load_fs_for_bindings(
        conn: &rusqlite::Connection,
        binding_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, Vec<String>>> {
        if binding_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // Build a `(?, ?, …)` placeholder list — rusqlite has no native
        // IN-array binding, so we expand manually.
        let placeholders = std::iter::repeat_n("?", binding_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT binding_id, feature_set_id
               FROM workspace_binding_feature_sets
              WHERE binding_id IN ({placeholders})
              ORDER BY binding_id, sort_order, feature_set_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params_dyn: Vec<&dyn rusqlite::ToSql> = binding_ids
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params_dyn.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let (binding_id, fs_id) = row?;
            grouped.entry(binding_id).or_default().push(fs_id);
        }
        Ok(grouped)
    }

    /// Replace the junction rows for `binding_id` with the supplied list,
    /// preserving `sort_order` from the slice's index. Used by both
    /// create() and update() so they share the write path.
    fn rewrite_fs_for_binding(
        conn: &rusqlite::Connection,
        binding_id: &str,
        feature_set_ids: &[String],
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM workspace_binding_feature_sets WHERE binding_id = ?1",
            params![binding_id],
        )?;
        for (idx, fs_id) in feature_set_ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO workspace_binding_feature_sets
                    (binding_id, feature_set_id, sort_order)
                 VALUES (?1, ?2, ?3)",
                params![binding_id, fs_id, idx as i64],
            )?;
        }
        Ok(())
    }

    const SELECT_COLS: &'static str =
        "id, workspace_root, label, space_id, created_at, updated_at";

    /// Fetch bindings + their FeatureSet lists in two queries.
    /// `where_clause` is appended to the binding SELECT (use `""` for none);
    /// `string_params` are bound to its placeholders in order.
    ///
    /// Owned `String` params keep this future `Send` — passing borrowed
    /// `&dyn ToSql` slices breaks `async_trait`'s `Send` requirement
    /// because `dyn ToSql` isn't `Sync`.
    async fn fetch_bindings(
        &self,
        where_clause: &str,
        string_params: Vec<String>,
    ) -> Result<Vec<WorkspaceBinding>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let sql = format!(
            "SELECT {} FROM workspace_bindings {} ORDER BY workspace_root",
            Self::SELECT_COLS,
            where_clause,
        );
        let mut stmt = conn.prepare(&sql)?;
        let params_dyn: Vec<&dyn rusqlite::ToSql> = string_params
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();
        let mut bindings: Vec<WorkspaceBinding> = stmt
            .query_map(params_dyn.as_slice(), Self::row_to_binding_no_fs)?
            .collect::<Result<Vec<_>, _>>()?;

        let ids: Vec<String> = bindings.iter().map(|b| b.id.to_string()).collect();
        let mut fs_map = Self::load_fs_for_bindings(conn, &ids)?;
        for binding in &mut bindings {
            if let Some(fs_ids) = fs_map.remove(&binding.id.to_string()) {
                binding.feature_set_ids = fs_ids;
            }
        }
        Ok(bindings)
    }
}

#[async_trait]
impl WorkspaceBindingRepository for SqliteWorkspaceBindingRepository {
    async fn list(&self) -> Result<Vec<WorkspaceBinding>> {
        self.fetch_bindings("", Vec::new()).await
    }

    async fn list_for_space(&self, space_id: &Uuid) -> Result<Vec<WorkspaceBinding>> {
        self.fetch_bindings("WHERE space_id = ?", vec![space_id.to_string()])
            .await
    }

    async fn get(&self, id: &Uuid) -> Result<Option<WorkspaceBinding>> {
        let mut bindings = self
            .fetch_bindings("WHERE id = ?", vec![id.to_string()])
            .await?;
        Ok(bindings.pop())
    }

    async fn create(&self, binding: &WorkspaceBinding) -> Result<()> {
        if binding.feature_set_ids.is_empty() {
            anyhow::bail!(
                "WorkspaceBinding {} must have at least one feature_set_id",
                binding.id
            );
        }
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT INTO workspace_bindings
                (id, workspace_root, label, space_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                binding.id.to_string(),
                binding.workspace_root,
                binding.label,
                binding.space_id.to_string(),
                binding.created_at.to_rfc3339(),
                binding.updated_at.to_rfc3339(),
            ],
        )?;
        Self::rewrite_fs_for_binding(conn, &binding.id.to_string(), &binding.feature_set_ids)?;

        Ok(())
    }

    async fn update(&self, binding: &WorkspaceBinding) -> Result<()> {
        if binding.feature_set_ids.is_empty() {
            anyhow::bail!(
                "WorkspaceBinding {} must have at least one feature_set_id",
                binding.id
            );
        }
        let db = self.db.lock().await;
        let conn = db.connection();

        let rows_affected = conn.execute(
            "UPDATE workspace_bindings
             SET workspace_root = ?2, label = ?3, space_id = ?4, updated_at = ?5
             WHERE id = ?1",
            params![
                binding.id.to_string(),
                binding.workspace_root,
                binding.label,
                binding.space_id.to_string(),
                binding.updated_at.to_rfc3339(),
            ],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("WorkspaceBinding not found: {}", binding.id);
        }

        // Rewrite the junction. ON DELETE CASCADE on the FK means a binding
        // delete cleans up automatically, but for an update we have to do
        // it manually — the user may have re-ordered or swapped FSes.
        Self::rewrite_fs_for_binding(conn, &binding.id.to_string(), &binding.feature_set_ids)?;

        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        // Junction rows go away via ON DELETE CASCADE.
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

    async fn add_fs(db: &Arc<Mutex<Database>>, space_id: Uuid, name: &str) -> String {
        let fs = FeatureSet::new_custom(name, space_id.to_string());
        let fs_id = fs.id.clone();
        let now = Utc::now().to_rfc3339();
        let guard = db.lock().await;
        guard
            .connection()
            .execute(
                "INSERT INTO feature_sets (id, name, feature_set_type, space_id, is_builtin, created_at, updated_at)
                 VALUES (?1, ?2, 'custom', ?3, 0, ?4, ?4)",
                params![fs.id, name, space_id.to_string(), now],
            )
            .unwrap();
        fs_id
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
        assert_eq!(got.feature_set_ids, vec![fs_id]);
    }

    #[tokio::test]
    async fn test_multi_fs_round_trip() {
        let (repo, space_id, fs_id1) = fixture().await;
        // Need to construct a fresh DB-backed FS pair to satisfy the FK.
        // Reach back into the same DB the repo was built around by going
        // through a second `add_fs`.
        let db = repo.db.clone();
        let fs_id2 = add_fs(&db, space_id, "second").await;

        let root = if cfg!(windows) { "d:\\multi" } else { "/multi" };
        let binding =
            WorkspaceBinding::new_multi(root, space_id, vec![fs_id1.clone(), fs_id2.clone()]);
        repo.create(&binding).await.unwrap();

        let got = repo.get(&binding.id).await.unwrap().unwrap();
        // Insertion order preserved via sort_order.
        assert_eq!(got.feature_set_ids, vec![fs_id1.clone(), fs_id2.clone()]);

        // Update — drop one, reorder.
        let mut updated = got;
        updated.feature_set_ids = vec![fs_id2.clone()];
        repo.update(&updated).await.unwrap();
        let after = repo.get(&binding.id).await.unwrap().unwrap();
        assert_eq!(after.feature_set_ids, vec![fs_id2]);
    }

    #[tokio::test]
    async fn test_create_rejects_empty_fs_list() {
        let (repo, space_id, _) = fixture().await;
        let root = if cfg!(windows) { "d:\\empty" } else { "/empty" };
        let binding = WorkspaceBinding::new_multi(root, space_id, vec![]);
        let err = repo.create(&binding).await.unwrap_err();
        assert!(err.to_string().contains("at least one feature_set_id"));
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
