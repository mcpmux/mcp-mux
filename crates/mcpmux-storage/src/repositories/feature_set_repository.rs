//! SQLite implementation of FeatureSetRepository.
//!
//! Updated for the new schema with feature_set_type, space_id, and composition.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{
    FeatureSet, FeatureSetMember, FeatureSetRepository, FeatureSetType, MemberMode, MemberType,
};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;

use crate::Database;

/// SQLite-backed implementation of FeatureSetRepository.
pub struct SqliteFeatureSetRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteFeatureSetRepository {
    /// Create a new SQLite feature set repository.
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

    /// Parse a row into a FeatureSet (without members).
    fn row_to_feature_set(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeatureSet> {
        Ok(FeatureSet {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            icon: row.get(3)?,
            space_id: row.get(4)?,
            feature_set_type: FeatureSetType::parse(&row.get::<_, String>(5)?)
                .unwrap_or(FeatureSetType::Custom),
            server_id: row.get(6)?,
            is_builtin: row.get::<_, i32>(7)? == 1,
            is_deleted: row.get::<_, i32>(8)? == 1,
            created_at: Self::parse_datetime(&row.get::<_, String>(9)?),
            updated_at: Self::parse_datetime(&row.get::<_, String>(10)?),
            auto_include: row.get::<_, i32>(11)? == 1,
            members: vec![], // Members loaded separately
        })
    }

    /// If `feature_set_id` is in auto mode, turn it into a manual set whose
    /// explicit members are every feature currently in its Space, so a
    /// single-member edit lands on top of what the set already granted
    /// instead of replacing it. `skip_feature_id` is left out of the written
    /// members (the caller is about to add or remove it). Returns whether the
    /// set was in auto mode. Runs on the caller's connection/transaction.
    fn materialize_auto_sync(
        conn: &rusqlite::Connection,
        feature_set_id: &str,
        skip_feature_id: Option<&str>,
    ) -> Result<bool> {
        let row: Option<(i32, Option<String>)> = conn
            .query_row(
                "SELECT auto_include, space_id FROM feature_sets WHERE id = ? AND is_deleted = 0",
                params![feature_set_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((1, space_id)) = row else {
            return Ok(false);
        };

        // Every feature the Space knows about, available or not: a server
        // that happens to be disconnected right now keeps its tools in the
        // set (resolution filters by availability anyway).
        let feature_ids: Vec<String> = match space_id {
            Some(space_id) => {
                let mut stmt = conn.prepare(
                    "SELECT id FROM server_features WHERE space_id = ?
                     ORDER BY server_id, feature_type, feature_name",
                )?;
                let ids = stmt
                    .query_map(params![space_id], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                ids
            }
            None => Vec::new(),
        };

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "DELETE FROM feature_set_members WHERE feature_set_id = ?",
            params![feature_set_id],
        )?;
        for feature_id in feature_ids
            .iter()
            .filter(|id| Some(id.as_str()) != skip_feature_id)
        {
            conn.execute(
                "INSERT INTO feature_set_members (id, feature_set_id, member_type, member_id, mode, created_at)
                 VALUES (?1, ?2, 'feature', ?3, 'include', ?4)",
                params![uuid::Uuid::new_v4().to_string(), feature_set_id, feature_id, now],
            )?;
        }
        conn.execute(
            "UPDATE feature_sets SET auto_include = 0, updated_at = ?2 WHERE id = ?1",
            params![feature_set_id, now],
        )?;
        Ok(true)
    }

    /// Parse a row into a FeatureSetMember.
    fn row_to_member(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeatureSetMember> {
        Ok(FeatureSetMember {
            id: row.get(0)?,
            feature_set_id: row.get(1)?,
            member_type: MemberType::parse(&row.get::<_, String>(2)?)
                .unwrap_or(MemberType::Feature),
            member_id: row.get(3)?,
            mode: MemberMode::parse(&row.get::<_, String>(4)?).unwrap_or(MemberMode::Include),
        })
    }

    /// Load members for a feature set
    async fn load_members(&self, feature_set_id: &str) -> Result<Vec<FeatureSetMember>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, feature_set_id, member_type, member_id, mode
             FROM feature_set_members
             WHERE feature_set_id = ?
             ORDER BY id",
        )?;

        let members = stmt
            .query_map(params![feature_set_id], Self::row_to_member)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(members)
    }

    /// Load members for a feature set (synchronous version for use with locked connection)
    fn get_members_sync(
        conn: &rusqlite::Connection,
        feature_set_id: &str,
    ) -> Result<Vec<FeatureSetMember>> {
        let mut stmt = conn.prepare(
            "SELECT id, feature_set_id, member_type, member_id, mode
             FROM feature_set_members
             WHERE feature_set_id = ?
             ORDER BY id",
        )?;

        let members = stmt
            .query_map(params![feature_set_id], Self::row_to_member)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(members)
    }
}

#[async_trait]
impl FeatureSetRepository for SqliteFeatureSetRepository {
    async fn list(&self) -> Result<Vec<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, space_id, feature_set_type, 
                    server_id, is_builtin, is_deleted, created_at, updated_at, auto_include 
             FROM feature_sets 
             WHERE is_deleted = 0
             ORDER BY is_builtin DESC, name ASC",
        )?;

        let feature_sets = stmt
            .query_map([], Self::row_to_feature_set)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(feature_sets)
    }

    async fn list_by_space(&self, space_id: &str) -> Result<Vec<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, space_id, feature_set_type, 
                    server_id, is_builtin, is_deleted, created_at, updated_at, auto_include 
             FROM feature_sets 
             WHERE space_id = ? AND is_deleted = 0
             ORDER BY is_builtin DESC, feature_set_type, name ASC",
        )?;

        let mut feature_sets = stmt
            .query_map(params![space_id], Self::row_to_feature_set)?
            .collect::<Result<Vec<_>, _>>()?;

        // Load members for each feature set
        for fs in &mut feature_sets {
            fs.members = Self::get_members_sync(conn, &fs.id)?;
        }

        Ok(feature_sets)
    }

    async fn get(&self, id: &str) -> Result<Option<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn
            .query_row(
                "SELECT id, name, description, icon, space_id, feature_set_type, 
                        server_id, is_builtin, is_deleted, created_at, updated_at, auto_include 
                 FROM feature_sets 
                 WHERE id = ? AND is_deleted = 0",
                params![id],
                Self::row_to_feature_set,
            )
            .optional()?;

        Ok(result)
    }

    async fn get_with_members(&self, id: &str) -> Result<Option<FeatureSet>> {
        let feature_set = self.get(id).await?;
        if let Some(mut fs) = feature_set {
            fs.members = self.load_members(id).await?;
            Ok(Some(fs))
        } else {
            Ok(None)
        }
    }

    async fn create(&self, feature_set: &FeatureSet) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT INTO feature_sets 
                (id, name, description, icon, space_id, feature_set_type, 
                 server_id, is_builtin, is_deleted, created_at, updated_at, auto_include)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                feature_set.id,
                feature_set.name,
                feature_set.description,
                feature_set.icon,
                feature_set.space_id,
                feature_set.feature_set_type.as_str(),
                feature_set.server_id,
                if feature_set.is_builtin { 1 } else { 0 },
                if feature_set.is_deleted { 1 } else { 0 },
                feature_set.created_at.to_rfc3339(),
                feature_set.updated_at.to_rfc3339(),
                // An auto set never carries explicit members.
                if feature_set.auto_include && feature_set.members.is_empty() {
                    1
                } else {
                    0
                },
            ],
        )?;

        // Insert members if any
        let now = chrono::Utc::now().to_rfc3339();
        for member in &feature_set.members {
            conn.execute(
                "INSERT INTO feature_set_members (id, feature_set_id, member_type, member_id, mode, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    member.id,
                    member.feature_set_id,
                    member.member_type.as_str(),
                    member.member_id,
                    member.mode.as_str(),
                    now,
                ],
            )?;
        }

        Ok(())
    }

    async fn update(&self, feature_set: &FeatureSet) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Builtin FeatureSets (the auto-seeded Starter) are the default
        // fallback for unmapped folders, so their identity is fixed: name,
        // description, and icon are preserved here regardless of the incoming
        // values — only the MEMBERS (replaced below) and `updated_at` are
        // editable. The DB's own `is_builtin` flag governs (not the caller's
        // struct), so the lock holds for every caller, including the
        // member-set command that routes through update(). Custom sets update
        // normally.
        //
        // `auto_include` follows the incoming struct, except that writing any
        // explicit member turns auto mode off: an auto set has no members, so
        // a caller that loaded an auto set and replaced its members is making
        // a manual selection.
        let rows_affected = conn.execute(
            "UPDATE feature_sets
             SET name = CASE WHEN is_builtin = 1 THEN name ELSE ?2 END,
                 description = CASE WHEN is_builtin = 1 THEN description ELSE ?3 END,
                 icon = CASE WHEN is_builtin = 1 THEN icon ELSE ?4 END,
                 updated_at = ?5,
                 auto_include = ?6
             WHERE id = ?1 AND is_deleted = 0",
            params![
                feature_set.id,
                feature_set.name,
                feature_set.description,
                feature_set.icon,
                feature_set.updated_at.to_rfc3339(),
                if feature_set.auto_include && feature_set.members.is_empty() {
                    1
                } else {
                    0
                },
            ],
        )?;

        if rows_affected == 0 {
            anyhow::bail!("FeatureSet not found: {}", feature_set.id);
        }

        // Update members: delete old, insert new
        conn.execute(
            "DELETE FROM feature_set_members WHERE feature_set_id = ?",
            params![feature_set.id],
        )?;

        let now = chrono::Utc::now().to_rfc3339();
        for member in &feature_set.members {
            conn.execute(
                "INSERT INTO feature_set_members (id, feature_set_id, member_type, member_id, mode, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    member.id,
                    member.feature_set_id,
                    member.member_type.as_str(),
                    member.member_id,
                    member.mode.as_str(),
                    now,
                ],
            )?;
        }

        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Don't allow deleting builtin feature sets
        let is_builtin: i32 = conn
            .query_row(
                "SELECT is_builtin FROM feature_sets WHERE id = ?",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if is_builtin == 1 {
            anyhow::bail!("Cannot delete builtin FeatureSet: {}", id);
        }

        // Soft delete, and drop every reference to it in the same transaction.
        // FeatureSets are soft-deleted (`is_deleted = 1`), so the FK
        // `ON DELETE CASCADE` on the junction / grants never fires — a
        // workspace binding or client grant would keep pointing at a
        // FeatureSet that `get()` now reports as missing ("Feature set not
        // found"). Prune those references explicitly so the deletion is fully
        // reflected.
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE feature_sets SET is_deleted = 1, updated_at = datetime('now') WHERE id = ?",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM workspace_binding_feature_sets WHERE feature_set_id = ?",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM client_grants WHERE feature_set_id = ?",
            params![id],
        )?;
        tx.commit()?;

        Ok(())
    }

    async fn get_starter_for_space(&self, space_id: &str) -> Result<Option<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Match on `'starter' OR 'default'` so a freshly-migrated DB and a
        // pre-013 read both resolve correctly; migration 013 itself
        // rewrites stored rows so the legacy alias is dead weight quickly.
        let result = conn
            .query_row(
                "SELECT id, name, description, icon, space_id, feature_set_type,
                        server_id, is_builtin, is_deleted, created_at, updated_at, auto_include
                 FROM feature_sets
                 WHERE space_id = ?
                   AND feature_set_type IN ('starter', 'default')
                   AND is_deleted = 0",
                params![space_id],
                Self::row_to_feature_set,
            )
            .optional()?;

        Ok(result)
    }

    async fn ensure_builtin_for_space(&self, space_id: &str) -> Result<()> {
        if self.get_starter_for_space(space_id).await?.is_none() {
            let mut starter = FeatureSet::new_starter(space_id);
            // Same default as Space creation: auto unless turned off in Settings.
            starter.auto_include = {
                let db = self.db.lock().await;
                let value: Option<String> = db
                    .connection()
                    .query_row(
                        "SELECT value FROM app_settings WHERE key = ?",
                        params![mcpmux_core::STARTER_AUTO_INCLUDE_SETTING_KEY],
                        |row| row.get(0),
                    )
                    .optional()?;
                value.as_deref() != Some("false")
            };
            self.create(&starter).await?;
        }
        Ok(())
    }

    /// Add an individual feature to a feature set (SRP: manage members)
    async fn add_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
        mode: MemberMode,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let tx = conn.unchecked_transaction()?;

        // An edit on an auto set switches it to manual, keeping what it
        // already granted.
        Self::materialize_auto_sync(&tx, feature_set_id, Some(feature_id))?;

        let member = FeatureSetMember {
            id: uuid::Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.to_string(),
            member_type: MemberType::Feature,
            member_id: feature_id.to_string(),
            mode,
        };

        // Idempotent: adding a feature that's already a member (e.g. one the
        // auto-mode materialization above just wrote, or a repeated add from a
        // bulk caller) updates its mode instead of tripping the UNIQUE index.
        tx.execute(
            "INSERT INTO feature_set_members (id, feature_set_id, member_type, member_id, mode, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(feature_set_id, member_type, member_id) DO UPDATE SET mode = excluded.mode",
            params![
                member.id,
                member.feature_set_id,
                member.member_type.as_str(),
                member.member_id,
                member.mode.as_str(),
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        tx.commit()?;

        Ok(())
    }

    /// Remove an individual feature from a feature set
    async fn remove_feature_member(&self, feature_set_id: &str, feature_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let tx = conn.unchecked_transaction()?;

        // On an auto set, materializing without `feature_id` is the removal.
        Self::materialize_auto_sync(&tx, feature_set_id, Some(feature_id))?;
        tx.execute(
            "DELETE FROM feature_set_members 
             WHERE feature_set_id = ?1 AND member_id = ?2 AND member_type = 'feature'",
            params![feature_set_id, feature_id],
        )?;
        tx.commit()?;

        Ok(())
    }

    /// Get all feature members (not feature_set members) of a feature set
    async fn get_feature_members(&self, feature_set_id: &str) -> Result<Vec<FeatureSetMember>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, feature_set_id, member_type, member_id, mode
             FROM feature_set_members
             WHERE feature_set_id = ?1 AND member_type = 'feature'
             ORDER BY id",
        )?;

        let members = stmt
            .query_map(params![feature_set_id], Self::row_to_member)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(members)
    }

    async fn set_auto_include(&self, feature_set_id: &str, enabled: bool) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let tx = conn.unchecked_transaction()?;

        let exists: Option<i32> = tx
            .query_row(
                "SELECT auto_include FROM feature_sets WHERE id = ? AND is_deleted = 0",
                params![feature_set_id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            anyhow::bail!("FeatureSet not found: {}", feature_set_id);
        }

        if enabled {
            tx.execute(
                "DELETE FROM feature_set_members WHERE feature_set_id = ?",
                params![feature_set_id],
            )?;
            tx.execute(
                "UPDATE feature_sets SET auto_include = 1, updated_at = ?2 WHERE id = ?1",
                params![feature_set_id, chrono::Utc::now().to_rfc3339()],
            )?;
        } else {
            // No-op on a set that's already manual.
            Self::materialize_auto_sync(&tx, feature_set_id, None)?;
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
        let repo = SqliteFeatureSetRepository::new(db);

        // Create (use default space from migration)
        let fs = FeatureSet::new_custom("My Custom Set", DEFAULT_SPACE_ID)
            .with_description("A custom feature set");
        repo.create(&fs).await.unwrap();

        // Read
        let found = repo.get(&fs.id).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.name, "My Custom Set");

        // List by space: migration seeds 1 builtin (Default) + our 1 custom = 2.
        let all = repo.list_by_space(DEFAULT_SPACE_ID).await.unwrap();
        assert_eq!(all.len(), 2);

        // Delete
        repo.delete(&fs.id).await.unwrap();
        let found = repo.get(&fs.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_starter_feature_set_seeded_for_default_space() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);

        // Migration 001 seeds the auto-Starter FS for the migration-
        // created default Space; migration 013 renames its type from
        // 'default' to 'starter'. Confirm it's present and blocked from
        // deletion (builtins aren't user-deletable).
        let starter = repo
            .get_starter_for_space(DEFAULT_SPACE_ID)
            .await
            .unwrap()
            .expect("Starter FS should exist for the default space");
        assert_eq!(starter.feature_set_type, FeatureSetType::Starter);

        let result = repo.delete(&starter.id).await;
        assert!(result.is_err(), "builtin Starter FS must not be deletable");
    }

    #[tokio::test]
    async fn test_ensure_builtin_is_idempotent() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);

        repo.ensure_builtin_for_space(DEFAULT_SPACE_ID)
            .await
            .unwrap();
        repo.ensure_builtin_for_space(DEFAULT_SPACE_ID)
            .await
            .unwrap();

        let by_space = repo.list_by_space(DEFAULT_SPACE_ID).await.unwrap();
        let starters = by_space
            .iter()
            .filter(|f| matches!(f.feature_set_type, FeatureSetType::Starter))
            .count();
        assert_eq!(starters, 1);
    }

    /// Seed two features in the default Space and return their ids.
    async fn seed_features(db: &Arc<Mutex<Database>>) -> (String, String) {
        let db = db.lock().await;
        for (id, name) in [("feat-a", "alpha"), ("feat-b", "beta")] {
            db.connection()
                .execute(
                    "INSERT INTO server_features
                        (id, space_id, server_id, feature_type, feature_name, discovered_at, last_seen_at)
                     VALUES (?1, ?2, 'srv', 'tool', ?3, datetime('now'), datetime('now'))",
                    params![id, DEFAULT_SPACE_ID, name],
                )
                .unwrap();
        }
        ("feat-a".to_string(), "feat-b".to_string())
    }

    fn member_ids(fs: &FeatureSet) -> Vec<String> {
        let mut ids: Vec<String> = fs.members.iter().map(|m| m.member_id.clone()).collect();
        ids.sort();
        ids
    }

    async fn starter(repo: &SqliteFeatureSetRepository) -> FeatureSet {
        let id = repo
            .get_starter_for_space(DEFAULT_SPACE_ID)
            .await
            .unwrap()
            .unwrap()
            .id;
        repo.get_with_members(&id).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn fresh_starter_is_auto_with_no_members() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);
        let fs = starter(&repo).await;
        assert!(fs.auto_include);
        assert!(fs.members.is_empty());
    }

    #[tokio::test]
    async fn writing_members_turns_auto_off() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let (a, _) = seed_features(&db).await;
        let repo = SqliteFeatureSetRepository::new(db);

        // A caller that loaded the auto Starter and replaced its members is
        // making a manual selection, even though its struct still says auto.
        let mut fs = starter(&repo).await;
        fs.members = vec![FeatureSetMember::include_feature(&fs.id, &a)];
        repo.update(&fs).await.unwrap();

        let fs = starter(&repo).await;
        assert!(!fs.auto_include);
        assert_eq!(member_ids(&fs), vec![a]);
    }

    #[tokio::test]
    async fn update_without_members_keeps_auto() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);
        let fs = starter(&repo).await;
        repo.update(&fs).await.unwrap();
        assert!(starter(&repo).await.auto_include);
    }

    #[tokio::test]
    async fn single_member_edits_on_an_auto_set_keep_what_it_granted() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let (a, b) = seed_features(&db).await;
        let repo = SqliteFeatureSetRepository::new(db);
        let id = starter(&repo).await.id;

        // Adding a feature that's already granted, twice (bulk callers do
        // this), must neither fail on the UNIQUE index nor drop the others.
        repo.add_feature_member(&id, &a, MemberMode::Include)
            .await
            .unwrap();
        repo.add_feature_member(&id, &b, MemberMode::Include)
            .await
            .unwrap();
        let fs = starter(&repo).await;
        assert!(!fs.auto_include);
        assert_eq!(member_ids(&fs), vec![a.clone(), b.clone()]);

        // Back to auto, then remove one: everything else stays.
        repo.set_auto_include(&id, true).await.unwrap();
        repo.remove_feature_member(&id, &a).await.unwrap();
        let fs = starter(&repo).await;
        assert!(!fs.auto_include);
        assert_eq!(member_ids(&fs), vec![b]);
    }

    #[tokio::test]
    async fn set_auto_include_round_trip() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let (a, b) = seed_features(&db).await;
        let repo = SqliteFeatureSetRepository::new(db);
        let id = starter(&repo).await.id;

        // Off: the user starts editing from what the set granted.
        repo.set_auto_include(&id, false).await.unwrap();
        let fs = starter(&repo).await;
        assert!(!fs.auto_include);
        assert_eq!(member_ids(&fs), vec![a, b]);

        // Off again is a no-op on a manual set.
        repo.remove_feature_member(&id, "feat-a").await.unwrap();
        repo.set_auto_include(&id, false).await.unwrap();
        assert_eq!(
            member_ids(&starter(&repo).await),
            vec!["feat-b".to_string()]
        );

        // On: explicit members are dropped.
        repo.set_auto_include(&id, true).await.unwrap();
        let fs = starter(&repo).await;
        assert!(fs.auto_include);
        assert!(fs.members.is_empty());

        assert!(repo.set_auto_include("missing", true).await.is_err());
    }

    #[tokio::test]
    async fn ensure_builtin_honors_the_settings_default() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        {
            let db = db.lock().await;
            db.connection()
                .execute_batch(&format!(
                    "DELETE FROM feature_sets WHERE feature_set_type = 'starter';
                     INSERT INTO app_settings (key, value, updated_at)
                     VALUES ('{}', 'false', datetime('now'));",
                    mcpmux_core::STARTER_AUTO_INCLUDE_SETTING_KEY
                ))
                .unwrap();
        }
        let repo = SqliteFeatureSetRepository::new(db);
        repo.ensure_builtin_for_space(DEFAULT_SPACE_ID)
            .await
            .unwrap();
        assert!(!starter(&repo).await.auto_include);
    }
}
