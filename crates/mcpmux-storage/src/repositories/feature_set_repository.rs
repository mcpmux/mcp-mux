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
            feature_set_type: FeatureSetType::from_str(&row.get::<_, String>(5)?)
                .unwrap_or(FeatureSetType::Custom),
            server_id: row.get(6)?,
            is_builtin: row.get::<_, i32>(7)? == 1,
            is_deleted: row.get::<_, i32>(8)? == 1,
            created_at: Self::parse_datetime(&row.get::<_, String>(9)?),
            updated_at: Self::parse_datetime(&row.get::<_, String>(10)?),
            members: vec![], // Members loaded separately
        })
    }

    /// Parse a row into a FeatureSetMember.
    fn row_to_member(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeatureSetMember> {
        Ok(FeatureSetMember {
            id: row.get(0)?,
            feature_set_id: row.get(1)?,
            member_type: MemberType::from_str(&row.get::<_, String>(2)?)
                .unwrap_or(MemberType::Feature),
            member_id: row.get(3)?,
            mode: MemberMode::from_str(&row.get::<_, String>(4)?).unwrap_or(MemberMode::Include),
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
    fn get_members_sync(conn: &rusqlite::Connection, feature_set_id: &str) -> Result<Vec<FeatureSetMember>> {
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
                    server_id, is_builtin, is_deleted, created_at, updated_at 
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
                    server_id, is_builtin, is_deleted, created_at, updated_at 
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
                        server_id, is_builtin, is_deleted, created_at, updated_at 
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
                 server_id, is_builtin, is_deleted, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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

        let rows_affected = conn.execute(
            "UPDATE feature_sets 
             SET name = ?2, description = ?3, icon = ?4, updated_at = ?5
             WHERE id = ?1 AND is_deleted = 0",
            params![
                feature_set.id,
                feature_set.name,
                feature_set.description,
                feature_set.icon,
                feature_set.updated_at.to_rfc3339(),
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

        // Soft delete
        conn.execute(
            "UPDATE feature_sets SET is_deleted = 1, updated_at = datetime('now') WHERE id = ?",
            params![id],
        )?;

        Ok(())
    }

    async fn list_builtin(&self, space_id: &str) -> Result<Vec<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, space_id, feature_set_type, 
                    server_id, is_builtin, is_deleted, created_at, updated_at 
             FROM feature_sets 
             WHERE space_id = ? AND is_builtin = 1 AND is_deleted = 0
             ORDER BY feature_set_type, name ASC",
        )?;

        let feature_sets = stmt
            .query_map(params![space_id], Self::row_to_feature_set)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(feature_sets)
    }

    async fn get_server_all(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> Result<Option<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn
            .query_row(
                "SELECT id, name, description, icon, space_id, feature_set_type, 
                        server_id, is_builtin, is_deleted, created_at, updated_at 
                 FROM feature_sets 
                 WHERE space_id = ? AND server_id = ? AND feature_set_type = 'server-all' AND is_deleted = 0",
                params![space_id, server_id],
                Self::row_to_feature_set,
            )
            .optional()?;

        Ok(result)
    }

    async fn ensure_server_all(
        &self,
        space_id: &str,
        server_id: &str,
        server_name: &str,
    ) -> Result<FeatureSet> {
        // Check if it already exists
        if let Some(existing) = self.get_server_all(space_id, server_id).await? {
            return Ok(existing);
        }

        // Create new server-all featureset
        let fs = FeatureSet::new_server_all(space_id, server_id, server_name);
        self.create(&fs).await?;
        Ok(fs)
    }

    async fn get_default_for_space(&self, space_id: &str) -> Result<Option<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn
            .query_row(
                "SELECT id, name, description, icon, space_id, feature_set_type, 
                        server_id, is_builtin, is_deleted, created_at, updated_at 
                 FROM feature_sets 
                 WHERE space_id = ? AND feature_set_type = 'default' AND is_deleted = 0",
                params![space_id],
                Self::row_to_feature_set,
            )
            .optional()?;

        Ok(result)
    }

    async fn get_all_for_space(&self, space_id: &str) -> Result<Option<FeatureSet>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn
            .query_row(
                "SELECT id, name, description, icon, space_id, feature_set_type, 
                        server_id, is_builtin, is_deleted, created_at, updated_at 
                 FROM feature_sets 
                 WHERE space_id = ? AND feature_set_type = 'all' AND is_deleted = 0",
                params![space_id],
                Self::row_to_feature_set,
            )
            .optional()?;

        Ok(result)
    }
    
    async fn delete_server_all(&self, space_id: &str, server_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        
        // Hard delete server-all feature set for this server (used during uninstall)
        // Unlike regular delete(), this allows deleting builtin server-all feature sets
        conn.execute(
            "DELETE FROM feature_sets 
             WHERE space_id = ? AND server_id = ? AND feature_set_type = 'server-all'",
            params![space_id, server_id],
        )?;
        
        Ok(())
    }

    async fn ensure_builtin_for_space(&self, space_id: &str) -> Result<()> {
        // Check if "All" exists
        if self.get_all_for_space(space_id).await?.is_none() {
            let all = FeatureSet::new_all(space_id);
            self.create(&all).await?;
        }

        // Check if "Default" exists
        if self.get_default_for_space(space_id).await?.is_none() {
            let default = FeatureSet::new_default(space_id);
            self.create(&default).await?;
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

        let member = FeatureSetMember {
            id: uuid::Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.to_string(),
            member_type: MemberType::Feature,
            member_id: feature_id.to_string(),
            mode,
        };

        conn.execute(
            "INSERT INTO feature_set_members (id, feature_set_id, member_type, member_id, mode, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                member.id,
                member.feature_set_id,
                member.member_type.as_str(),
                member.member_id,
                member.mode.as_str(),
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Remove an individual feature from a feature set
    async fn remove_feature_member(
        &self,
        feature_set_id: &str,
        feature_id: &str,
    ) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "DELETE FROM feature_set_members 
             WHERE feature_set_id = ?1 AND member_id = ?2 AND member_type = 'feature'",
            params![feature_set_id, feature_id],
        )?;

        Ok(())
    }

    /// Get all feature members (not feature_set members) of a feature set
    async fn get_feature_members(
        &self,
        feature_set_id: &str,
    ) -> Result<Vec<FeatureSetMember>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crud_operations() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);

        // Create
        let fs = FeatureSet::new_custom("My Custom Set", "space_123")
            .with_description("A custom feature set");
        repo.create(&fs).await.unwrap();

        // Read
        let found = repo.get(&fs.id).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.name, "My Custom Set");

        // List by space
        let all = repo.list_by_space("space_123").await.unwrap();
        assert_eq!(all.len(), 1);

        // Delete
        repo.delete(&fs.id).await.unwrap();
        let found = repo.get(&fs.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_builtin_feature_sets() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);

        // Create builtin feature sets
        let all = FeatureSet::new_all("space_123");
        let default = FeatureSet::new_default("space_123");
        repo.create(&all).await.unwrap();
        repo.create(&default).await.unwrap();

        // List builtin
        let builtin = repo.list_builtin("space_123").await.unwrap();
        assert_eq!(builtin.len(), 2);

        // Cannot delete builtin
        let result = repo.delete(&all.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_server_all_featureset() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteFeatureSetRepository::new(db);

        // Ensure creates new
        let fs = repo
            .ensure_server_all("space_123", "github-mcp", "GitHub")
            .await
            .unwrap();
        assert_eq!(fs.feature_set_type, FeatureSetType::ServerAll);
        assert_eq!(fs.server_id, Some("github-mcp".to_string()));

        // Ensure returns existing
        let fs2 = repo
            .ensure_server_all("space_123", "github-mcp", "GitHub")
            .await
            .unwrap();
        assert_eq!(fs.id, fs2.id);
    }
}
