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
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "featureset_resolver",
        sql: include_str!("migrations/002_featureset_resolver.sql"),
    },
    Migration {
        version: 3,
        name: "drop_legacy_grants",
        sql: include_str!("migrations/003_drop_legacy_grants.sql"),
    },
    Migration {
        version: 4,
        name: "workspace_modes",
        sql: include_str!("migrations/004_workspace_modes.sql"),
    },
    Migration {
        version: 5,
        name: "drop_client_pin",
        sql: include_str!("migrations/005_drop_client_pin.sql"),
    },
    Migration {
        version: 6,
        name: "collapse_feature_sets",
        sql: include_str!("migrations/006_collapse_feature_sets.sql"),
    },
    Migration {
        version: 7,
        name: "concrete_binding",
        sql: include_str!("migrations/007_concrete_binding.sql"),
    },
    Migration {
        version: 8,
        name: "canonical_default_space",
        sql: include_str!("migrations/008_canonical_default_space.sql"),
    },
    Migration {
        version: 9,
        name: "restore_client_grants",
        sql: include_str!("migrations/009_restore_client_grants.sql"),
    },
    Migration {
        version: 10,
        name: "inbound_client_reports_roots",
        sql: include_str!("migrations/010_inbound_client_reports_roots.sql"),
    },
    Migration {
        version: 11,
        name: "inbound_client_roots_capability_known",
        sql: include_str!("migrations/011_inbound_client_roots_capability_known.sql"),
    },
    Migration {
        version: 12,
        name: "workspace_binding_feature_sets",
        sql: include_str!("migrations/012_workspace_binding_feature_sets.sql"),
    },
    Migration {
        version: 13,
        name: "rename_default_to_starter",
        sql: include_str!("migrations/013_rename_default_to_starter.sql"),
    },
    Migration {
        version: 14,
        name: "rewrite_starter_seed_copy",
        sql: include_str!("migrations/014_rewrite_starter_seed_copy.sql"),
    },
    Migration {
        version: 15,
        name: "rewrite_starter_seed_copy_v2",
        sql: include_str!("migrations/015_rewrite_starter_seed_copy_v2.sql"),
    },
    Migration {
        version: 16,
        name: "space_builtin_servers",
        sql: include_str!("migrations/016_space_builtin_servers.sql"),
    },
];

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

        // Disable foreign-key enforcement for the duration of the migration
        // run, following SQLite's documented "other kinds of table schema
        // changes" procedure (https://sqlite.org/lang_altertable.html): the
        // table-rebuild migrations (005/006 rebuild inbound_clients, 012
        // rebuilds workspace_bindings) `DROP TABLE` the parent, and SQLite's
        // implicit pre-DROP DELETE FIRES `ON DELETE CASCADE` into child rows
        // (oauth_tokens, oauth_authorization_codes, the FS junction) — wiping
        // them. The `PRAGMA foreign_keys=OFF` inside those migration files is
        // a NO-OP because it runs inside the per-migration transaction below;
        // the pragma only takes effect OUTSIDE a transaction, so it must be
        // set here, before any `BEGIN`. The connection-level setting persists
        // across the per-migration transactions until we restore it.
        let fk_was_on = self.foreign_keys_enabled();
        if fk_was_on {
            self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        }

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
                    // Best-effort restore of FK enforcement before bailing.
                    if fk_was_on {
                        let _ = self.conn.pragma_update(None, "foreign_keys", "ON");
                    }
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

        // Re-enable FK enforcement and verify the migrated schema is still
        // referentially consistent (catches any genuine orphan a migration
        // might have introduced while enforcement was off).
        if fk_was_on {
            self.foreign_key_check()?;
            self.conn.pragma_update(None, "foreign_keys", "ON")?;
        }

        Ok(())
    }

    /// Whether foreign-key enforcement is currently enabled on the connection.
    fn foreign_keys_enabled(&self) -> bool {
        self.conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .map(|v| v != 0)
            .unwrap_or(false)
    }

    /// Run `PRAGMA foreign_key_check` and fail if any violation is reported.
    /// Called after a migration run completes with FK enforcement temporarily
    /// disabled, so a buggy migration can't silently leave dangling rows.
    fn foreign_key_check(&self) -> Result<()> {
        let mut stmt = self.conn.prepare("PRAGMA foreign_key_check")?;
        let mut rows = stmt.query([])?;
        let mut violations: Vec<String> = Vec::new();
        while let Some(row) = rows.next()? {
            // Columns: table, rowid, referred_table, fk_index
            let table: String = row.get(0).unwrap_or_default();
            let referred: String = row.get(2).unwrap_or_default();
            violations.push(format!("{table} -> {referred}"));
        }
        if !violations.is_empty() {
            return Err(anyhow::anyhow!(
                "Post-migration foreign_key_check found {} violation(s): {}",
                violations.len(),
                violations.join(", ")
            ));
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

    /// After a full migration run on a fresh DB, FK enforcement must be back
    /// ON and the schema must be referentially consistent — the contract of
    /// the runner's disable-during-migration / re-enable-after logic.
    #[test]
    fn fresh_db_has_fk_enabled_and_no_orphans() {
        let db = Database::open_in_memory().unwrap();
        let fk: i64 = db
            .connection()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            fk, 1,
            "runner must re-enable FK enforcement after migrations"
        );
        let violations: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0, "migrated schema must have no FK violations");
    }

    /// The exact mechanic the table-rebuild migrations (005/006/012) depend
    /// on: DROPping a parent referenced by a child via `ON DELETE CASCADE`
    /// must NOT wipe the child — which only holds when FK enforcement is OFF
    /// during the rebuild (the runner disables it outside the transaction;
    /// the `PRAGMA foreign_keys=OFF` *inside* a migration file is a no-op).
    #[test]
    fn dropping_parent_with_fk_off_preserves_cascade_child() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "CREATE TABLE parent(id TEXT PRIMARY KEY);
             CREATE TABLE child(id TEXT PRIMARY KEY,
                 pid TEXT REFERENCES parent(id) ON DELETE CASCADE);
             INSERT INTO parent VALUES('p1');
             INSERT INTO child VALUES('c1','p1');",
        )
        .unwrap();

        // Disable FK enforcement the way the runner does, then rebuild parent.
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.execute_batch(
            "CREATE TABLE parent_new(id TEXT PRIMARY KEY);
             INSERT INTO parent_new SELECT * FROM parent;
             DROP TABLE parent;
             ALTER TABLE parent_new RENAME TO parent;",
        )
        .unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();

        let n: i64 = conn
            .query_row("SELECT count(*) FROM child", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            n, 1,
            "child must survive a parent table-rebuild when FK enforcement is off"
        );
    }

    /// End-to-end regression for the 005/006 data-loss bug: an
    /// `oauth_tokens` row seeded BEFORE the inbound_clients rebuild
    /// migrations must survive the upgrade. With FK enforcement left on
    /// (pre-fix), `DROP TABLE inbound_clients` cascade-deleted every token.
    #[test]
    fn migration_upgrade_preserves_oauth_tokens() {
        use rusqlite::{params, Connection};

        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let db = Database { conn };
        db.ensure_migrations_table().unwrap();

        // Apply migrations up to v4 (last version before the first
        // inbound_clients rebuild at 005), recording each as applied.
        const PRE_REBUILD: i64 = 4;
        for m in MIGRATIONS.iter().filter(|m| m.version <= PRE_REBUILD) {
            db.conn.execute_batch(m.sql).unwrap();
            db.conn
                .execute(
                    "INSERT OR REPLACE INTO schema_migrations (version, name, applied_at) \
                     VALUES (?1, ?2, datetime('now'))",
                    params![m.version, m.name],
                )
                .unwrap();
        }

        // Seed an inbound client + an issued OAuth token (child via CASCADE).
        db.conn
            .execute(
                "INSERT INTO inbound_clients
                   (client_id, registration_type, client_name, redirect_uris,
                    grant_types, response_types, token_endpoint_auth_method,
                    created_at, updated_at)
                 VALUES ('client-1','dcr','Test','[\"http://127.0.0.1/cb\"]',
                    '[\"authorization_code\"]','[\"code\"]','none',
                    datetime('now'), datetime('now'))",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO oauth_tokens
                   (id, client_id, token_type, token_hash, created_at)
                 VALUES ('tok-1','client-1','access','deadbeef', datetime('now'))",
                [],
            )
            .unwrap();

        // Run the remaining migrations (005..=latest) through the real runner.
        db.run_migrations().unwrap();

        let tokens: i64 = db
            .conn
            .query_row("SELECT count(*) FROM oauth_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            tokens, 1,
            "oauth token must survive the inbound_clients rebuild migrations"
        );
        // And the client row itself was preserved through the rebuilds.
        let clients: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM inbound_clients WHERE client_id='client-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(clients, 1, "inbound client must survive the rebuilds");
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
