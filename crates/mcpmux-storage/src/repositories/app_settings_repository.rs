//! SQLite implementation of AppSettingsRepository.
//!
//! Simple key-value store for application-wide settings.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mcpmux_core::AppSettingsRepository;
use rusqlite::params;
use tokio::sync::Mutex;

use crate::Database;

/// SQLite-backed app settings repository.
///
/// Stores application settings as key-value pairs with dot-notation namespacing.
/// 
/// # Example Keys
/// - `gateway.port` - Gateway server port (u16)
/// - `gateway.auto_start` - Auto-start gateway (bool)
/// - `ui.theme` - UI theme preference (string)
/// - `ui.window_state` - Window position/size (JSON)
pub struct SqliteAppSettingsRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteAppSettingsRepository {
    /// Create a new app settings repository.
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AppSettingsRepository for SqliteAppSettingsRepository {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let result = conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?",
            params![key],
            |row| row.get(0),
        );

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at) 
             VALUES (?, ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value],
        )?;

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();

        conn.execute("DELETE FROM app_settings WHERE key = ?", params![key])?;

        Ok(())
    }

    async fn list(&self) -> Result<Vec<(String, String)>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        let mut stmt = conn.prepare("SELECT key, value FROM app_settings ORDER BY key")?;

        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let db = self.db.lock().await;
        let conn = db.connection();

        // Use LIKE with escaped prefix for prefix matching
        let pattern = format!("{}%", prefix.replace('%', "\\%").replace('_', "\\_"));
        
        let mut stmt = conn.prepare(
            "SELECT key, value FROM app_settings WHERE key LIKE ? ESCAPE '\\' ORDER BY key"
        )?;

        let rows = stmt
            .query_map(params![pattern], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    async fn setup_test_db() -> Arc<Mutex<Database>> {
        let db = Database::open_in_memory().expect("Failed to create test database");
        Arc::new(Mutex::new(db))
    }

    #[tokio::test]
    async fn test_get_set_delete() {
        let db = setup_test_db().await;
        let repo = SqliteAppSettingsRepository::new(db);

        // Initially empty
        assert_eq!(repo.get("test.key").await.unwrap(), None);

        // Set a value
        repo.set("test.key", "test_value").await.unwrap();
        assert_eq!(repo.get("test.key").await.unwrap(), Some("test_value".to_string()));

        // Update the value
        repo.set("test.key", "updated_value").await.unwrap();
        assert_eq!(repo.get("test.key").await.unwrap(), Some("updated_value".to_string()));

        // Delete
        repo.delete("test.key").await.unwrap();
        assert_eq!(repo.get("test.key").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_list() {
        let db = setup_test_db().await;
        let repo = SqliteAppSettingsRepository::new(db);

        repo.set("b.key", "b_value").await.unwrap();
        repo.set("a.key", "a_value").await.unwrap();
        repo.set("c.key", "c_value").await.unwrap();

        let all = repo.list().await.unwrap();
        
        // Should be sorted by key (plus the default auto_start from migration)
        assert!(all.iter().any(|(k, v)| k == "a.key" && v == "a_value"));
        assert!(all.iter().any(|(k, v)| k == "b.key" && v == "b_value"));
        assert!(all.iter().any(|(k, v)| k == "c.key" && v == "c_value"));
    }

    #[tokio::test]
    async fn test_list_by_prefix() {
        let db = setup_test_db().await;
        let repo = SqliteAppSettingsRepository::new(db);

        repo.set("gateway.port", "45818").await.unwrap();
        repo.set("gateway.auto_start", "true").await.unwrap();
        repo.set("ui.theme", "dark").await.unwrap();

        let gateway_settings = repo.list_by_prefix("gateway.").await.unwrap();
        assert_eq!(gateway_settings.len(), 2);
        assert!(gateway_settings.iter().any(|(k, _)| k == "gateway.port"));
        assert!(gateway_settings.iter().any(|(k, _)| k == "gateway.auto_start"));

        let ui_settings = repo.list_by_prefix("ui.").await.unwrap();
        assert_eq!(ui_settings.len(), 1);
        assert!(ui_settings.iter().any(|(k, _)| k == "ui.theme"));
    }
}
