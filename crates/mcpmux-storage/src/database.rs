//! Database manager for SQLite storage.
//!
//! Note: We use standard SQLite (not SQLCipher) for simplicity.
//! Sensitive data (credentials, tokens) is encrypted at the application level
//! using the `crypto` module before being stored.
//!
//! ## Migration System
//!
//! Migrations are numbered sequentially (001, 002, 003, etc.) and stored in
//! the `migrations/` directory. Each migration is run exactly once, tracked
//! via the `schema_migrations` table.
//!
//! To add a new migration:
//! 1. Create a new file: `migrations/NNN_description.sql`
//! 2. Add the migration to the `MIGRATIONS` array below
//! 3. The migration will auto-run on next app startup

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use tracing::{debug, info};

/// A database migration with version number and SQL content.
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

/// All migrations in order. Add new migrations here.
///
/// Note: Migrations have been consolidated into a single clean initial migration.
/// The schema includes cached_definition for offline operation and excludes
/// runtime fields (connection_status, last_connected_at, last_error).
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial",
    sql: include_str!("migrations/001_initial.sql"),
}];

/// SQLite database wrapper.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open a database at the given path.
    ///
    /// If the database doesn't exist, it will be created.
    /// All pending migrations will be automatically applied.
    pub fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create database directory: {:?}", parent))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {:?}", path))?;

        // Enable foreign keys
        conn.pragma_update(None, "foreign_keys", "ON")?;

        // Set journal mode to WAL for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;

        debug!("Opened database at {:?}", path);

        let db = Self { conn };
        db.run_migrations()?;

        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;

        // Enable foreign keys
        conn.pragma_update(None, "foreign_keys", "ON")?;

        debug!("Opened in-memory database");

        let db = Self { conn };
        db.run_migrations()?;

        Ok(db)
    }

    /// Run all pending database migrations.
    fn run_migrations(&self) -> Result<()> {
        // First, ensure the schema_migrations table exists
        self.ensure_migrations_table()?;

        // Get current schema version
        let current_version = self.get_schema_version();

        info!(
            "Current database schema version: {}, latest available: {}",
            current_version,
            MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
        );

        // Run all migrations that haven't been applied yet
        for migration in MIGRATIONS {
            if migration.version > current_version {
                info!(
                    "Running migration {} ({})...",
                    migration.version, migration.name
                );

                // Run migration in a transaction
                let tx = self.conn.unchecked_transaction()?;

                if let Err(e) = self.conn.execute_batch(migration.sql) {
                    tracing::error!(
                        "Migration {} ({}) failed with error: {}",
                        migration.version,
                        migration.name,
                        e
                    );
                    return Err(anyhow::anyhow!(
                        "Failed to run migration {} ({}): {}",
                        migration.version,
                        migration.name,
                        e
                    ));
                }

                // Record that this migration was applied
                self.conn.execute(
                    "INSERT OR REPLACE INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, datetime('now'))",
                    rusqlite::params![migration.version, migration.name],
                )?;

                tx.commit()?;

                info!(
                    "Migration {} ({}) completed successfully",
                    migration.version, migration.name
                );
            }
        }

        Ok(())
    }

    /// Ensure the schema_migrations table exists with correct structure.
    fn ensure_migrations_table(&self) -> Result<()> {
        // Check if table exists
        let table_exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if table_exists {
            // Check if 'name' column exists (old schema didn't have it)
            let has_name_column: bool = self
                .conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('schema_migrations') WHERE name='name'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            if !has_name_column {
                // Upgrade old schema_migrations table to new format
                info!("Upgrading schema_migrations table to new format...");
                self.conn.execute_batch(
                    "ALTER TABLE schema_migrations ADD COLUMN name TEXT DEFAULT 'unknown';",
                )?;
            }
        } else {
            // Create new table
            self.conn.execute(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                )",
                [],
            )?;
        }
        Ok(())
    }

    /// Get the current schema version (highest applied migration).
    fn get_schema_version(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    /// Get a reference to the underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Execute a closure within a transaction.
    pub fn transaction<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let tx = self.conn.unchecked_transaction()?;
        let result = f(&self.conn)?;
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_in_memory_database() {
        let db = Database::open_in_memory().unwrap();

        // Verify tables exist
        let count: i64 = db
            .connection()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(count > 0, "Tables should be created");
    }

    #[test]
    fn test_persistent_database() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Open and create
        let db = Database::open(&db_path).unwrap();

        // Insert a space
        db.connection()
            .execute(
                "INSERT INTO spaces (id, name, created_at, updated_at) VALUES ('test', 'Test', datetime('now'), datetime('now'))",
                [],
            )
            .unwrap();

        drop(db);

        // Reopen
        let db2 = Database::open(&db_path).unwrap();
        let name: String = db2
            .connection()
            .query_row("SELECT name FROM spaces WHERE id = 'test'", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(name, "Test");
    }
}
