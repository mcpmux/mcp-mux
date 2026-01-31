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
        // Try RFC3339 first (e.g., "2024-01-01T00:00:00Z")
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        
        // Try SQLite's datetime format (e.g., "2024-01-01 00:00:00")
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        
        // Fallback to current time
        Utc::now()
    }
}

#[async_trait]
impl SpaceRepository for SqliteSpaceRepository {
    async fn list(&self) -> Result<Vec<Space>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        tracing::debug!("[SpaceRepository::list] Querying spaces...");
        
        let mut stmt = conn.prepare(
            "SELECT id, name, icon, description, is_default, sort_order, created_at, updated_at 
             FROM spaces 
             ORDER BY sort_order ASC, name ASC",
        )?;

        let spaces = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let name: String = row.get(1)?;
                tracing::debug!("[SpaceRepository::list] Found space: {} ({})", name, id_str);
                
                Ok(Space {
                    id: id_str.parse().unwrap_or_else(|e| {
                        tracing::warn!("[SpaceRepository::list] Failed to parse UUID '{}': {}", id_str, e);
                        Uuid::new_v4()
                    }),
                    name,
                    icon: row.get(2)?,
                    description: row.get(3)?,
                    is_default: row.get::<_, i32>(4)? == 1,
                    sort_order: row.get(5)?,
                    created_at: Self::parse_datetime(&row.get::<_, String>(6)?),
                    updated_at: Self::parse_datetime(&row.get::<_, String>(7)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        
        tracing::info!("[SpaceRepository::list] Returning {} spaces", spaces.len());

        Ok(spaces)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Space>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare(
            "SELECT id, name, icon, description, is_default, sort_order, created_at, updated_at 
             FROM spaces 
             WHERE id = ?",
        )?;

        let space = stmt
            .query_row(params![id.to_string()], |row| {
                Ok(Space {
                    id: row.get::<_, String>(0)?.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    name: row.get(1)?,
                    icon: row.get(2)?,
                    description: row.get(3)?,
                    is_default: row.get::<_, i32>(4)? == 1,
                    sort_order: row.get(5)?,
                    created_at: Self::parse_datetime(&row.get::<_, String>(6)?),
                    updated_at: Self::parse_datetime(&row.get::<_, String>(7)?),
                })
            })
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

        // Auto-create builtin featuresets for this space
        // "All Features" - contains all features from all servers in this space
        conn.execute(
            "INSERT OR IGNORE INTO feature_sets (id, name, description, icon, space_id, feature_set_type, is_builtin, created_at, updated_at)
             VALUES (?1, 'All Features', 'All features from all connected MCP servers in this space', '🌐', ?2, 'all', 1, ?3, ?3)",
            params![
                format!("fs_all_{}", space_id),
                space_id,
                now,
            ],
        )?;

        // "Default" - auto-granted to all clients in this space
        conn.execute(
            "INSERT OR IGNORE INTO feature_sets (id, name, description, icon, space_id, feature_set_type, is_builtin, created_at, updated_at)
             VALUES (?1, 'Default', 'Features automatically granted to all connected clients in this space', '⭐', ?2, 'default', 1, ?3, ?3)",
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
        
        let mut stmt = conn.prepare(
            "SELECT id, name, icon, description, is_default, sort_order, created_at, updated_at
             FROM spaces
             WHERE is_default = 1
             LIMIT 1",
        )?;

        let space = stmt
            .query_row([], |row| {
                let id_str: String = row.get(0)?;
                let name: String = row.get(1)?;
                
                Ok(Space {
                    id: id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
                    name,
                    icon: row.get(2)?,
                    description: row.get(3)?,
                    is_default: true,
                    sort_order: row.get(5)?,
                    created_at: Self::parse_datetime(&row.get::<_, String>(6)?),
                    updated_at: Self::parse_datetime(&row.get::<_, String>(7)?),
                })
            })
            .optional()?;

        Ok(space)
    }

    async fn set_default(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Use a transaction to ensure atomicity
        let tx = conn.unchecked_transaction()?;

        // Clear all defaults
        tx.execute("UPDATE spaces SET is_default = 0", [])?;

        // Set the new default
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

    #[tokio::test]
    async fn test_crud_operations() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteSpaceRepository::new(db);

        // Create
        let space = Space::new("Test Space").with_icon("🧪");
        repo.create(&space).await.unwrap();

        // Read
        let found = repo.get(&space.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Test Space");

        // List
        let all = repo.list().await.unwrap();
        assert_eq!(all.len(), 1);

        // Update
        let mut updated = space.clone();
        updated.name = "Updated Space".to_string();
        repo.update(&updated).await.unwrap();

        let found = repo.get(&space.id).await.unwrap().unwrap();
        assert_eq!(found.name, "Updated Space");

        // Delete
        repo.delete(&space.id).await.unwrap();
        let found = repo.get(&space.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_default_space() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteSpaceRepository::new(db);

        // Create two spaces
        let space1 = Space::new("Space 1").set_default();
        let space2 = Space::new("Space 2");
        repo.create(&space1).await.unwrap();
        repo.create(&space2).await.unwrap();

        // Check default
        let default = repo.get_default().await.unwrap();
        assert!(default.is_some());
        assert_eq!(default.unwrap().name, "Space 1");

        // Change default
        repo.set_default(&space2.id).await.unwrap();
        let default = repo.get_default().await.unwrap();
        assert!(default.is_some());
        assert_eq!(default.unwrap().name, "Space 2");
    }
}

