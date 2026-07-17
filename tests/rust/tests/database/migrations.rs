//! Migration tests

use mcpmux_storage::Database;
use tests::db::TestDatabase;

#[test]
fn test_migrations_run_successfully() {
    // Database::open runs migrations automatically
    let test_db = TestDatabase::new();

    // If we get here, migrations succeeded
    assert!(test_db.db_path().exists());
}

#[test]
fn test_migrations_are_idempotent() {
    let test_db = TestDatabase::new();

    // Opening the same database again should not fail
    let db2 = Database::open(test_db.db_path());
    assert!(db2.is_ok());
}

#[test]
fn test_database_creates_file() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("mcpmux.db");

    // Initially no database file
    assert!(!db_path.exists());

    // Open database (creates file)
    let _db = Database::open(&db_path).expect("Failed to open database");

    // Now database file should exist
    assert!(db_path.exists());
}

#[test]
fn test_in_memory_database() {
    // In-memory database should also run migrations
    let db = Database::open_in_memory().expect("Failed to open in-memory database");

    // Verify it's usable (we can't really check much else)
    drop(db);
}

/// Migration 017 must drop feature_set_members that point at a feature or
/// feature set that no longer exists (orphans from the pre-refactor member
/// identity), while keeping members that still resolve.
#[test]
fn test_017_purges_orphaned_feature_set_members() {
    let test_db = TestDatabase::new();
    let conn = test_db.db.connection();

    // Seed a space, a custom FS, one live feature, and three members:
    //   m1 — valid feature member (points to the live feature)        → keep
    //   m2 — orphaned feature member (old "server/tool" identity)     → purge
    //   m3 — composition member pointing at a deleted FS              → purge
    conn.execute_batch(
        "INSERT INTO spaces (id,name,icon,description,is_default,sort_order,created_at,updated_at)
           VALUES ('s1','S','x','',0,0,datetime('now'),datetime('now'));
         INSERT INTO feature_sets
           (id,name,description,icon,space_id,feature_set_type,server_id,is_builtin,is_deleted,created_at,updated_at)
           VALUES ('fs1','FS','','','s1','custom',NULL,0,0,datetime('now'),datetime('now'));
         INSERT INTO server_features
           (id,space_id,server_id,feature_type,feature_name,discovered_at,last_seen_at,is_available)
           VALUES ('feat-live','s1','srv','tool','do_thing',datetime('now'),datetime('now'),1);
         INSERT INTO feature_set_members (id,feature_set_id,member_type,member_id,mode,created_at) VALUES
           ('m1','fs1','feature','feat-live','include',datetime('now')),
           ('m2','fs1','feature','srv/do_thing','include',datetime('now')),
           ('m3','fs1','feature_set','fs-gone','include',datetime('now'));",
    )
    .expect("seed failed");

    // Re-apply migration 017 (idempotent) to exercise the purge on the seeded orphans.
    conn.execute_batch(include_str!(
        "../../../../crates/mcpmux-storage/src/migrations/017_purge_orphaned_feature_set_members.sql"
    ))
    .expect("migration 017 failed");

    let remaining: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT member_id FROM feature_set_members WHERE feature_set_id='fs1' ORDER BY member_id")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };

    assert_eq!(
        remaining,
        vec!["feat-live".to_string()],
        "only the still-resolvable feature member should survive the purge"
    );
}

/// Migration 018 rewrites the auto-seeded Starter FS's now-stale "no special
/// routing role; delete freely" description (the Starter is the default
/// fallback again and can't be deleted), but only on rows that STILL match the
/// exact 014/015 seed text — a Starter an operator re-described keeps its copy.
#[test]
fn test_018_rewrites_stale_starter_description_only() {
    const STALE: &str = "Auto-created with this Space. Edit, rename, or delete freely — bindings and per-client grants pick FeatureSets explicitly, so this one has no special routing role.";
    const FRESH: &str = "Auto-created with this Space — the default set for folders you haven't explicitly mapped. Edit which tools it includes to change what they get. Its name is fixed and it can't be deleted.";

    let test_db = TestDatabase::new();
    let conn = test_db.db.connection();

    // Two builtin Starter rows: one still carrying the stale 014/015 copy
    // (should be rewritten), one an operator customized (must be left alone).
    conn.execute_batch(
        "INSERT INTO spaces (id,name,icon,description,is_default,sort_order,created_at,updated_at)
           VALUES ('s-stale','Stale','x','',0,0,datetime('now'),datetime('now')),
                  ('s-cust','Custom','x','',0,0,datetime('now'),datetime('now'));
         INSERT INTO feature_sets
           (id,name,description,icon,space_id,feature_set_type,server_id,is_builtin,is_deleted,created_at,updated_at)
           VALUES
           ('fs_default_s-stale','Starter',
            'Auto-created with this Space. Edit, rename, or delete freely — bindings and per-client grants pick FeatureSets explicitly, so this one has no special routing role.',
            '⭐','s-stale','starter',NULL,1,0,datetime('now'),datetime('now')),
           ('fs_default_s-cust','Starter','My own words','⭐','s-cust','starter',NULL,1,0,datetime('now'),datetime('now'));",
    )
    .expect("seed failed");

    // Re-apply migration 018 (idempotent) to exercise the rewrite.
    conn.execute_batch(include_str!(
        "../../../../crates/mcpmux-storage/src/migrations/018_starter_is_default_fallback_copy.sql"
    ))
    .expect("migration 018 failed");

    let desc = |id: &str| -> String {
        conn.query_row(
            "SELECT description FROM feature_sets WHERE id = ?",
            [id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };

    assert_eq!(
        desc("fs_default_s-stale"),
        FRESH,
        "stale copy should be rewritten"
    );
    assert_ne!(STALE, FRESH); // guard against the strings drifting equal
    assert_eq!(
        desc("fs_default_s-cust"),
        "My own words",
        "operator-customized copy must be preserved"
    );
}

// ---------------------------------------------------------------------------
// Upgrade path — applying NEW migrations to an EXISTING (older) on-disk DB.
//
// Every other test here uses a FRESH in-memory DB, where all migrations run at
// once — so they never catch a migration that fails to apply when a real user
// opens a database created by a previous release. These two do.
// ---------------------------------------------------------------------------

fn table_exists(db: &Database, name: &str) -> bool {
    db.connection()
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false)
}

fn column_exists(db: &Database, table: &str, column: &str) -> bool {
    let sql = format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name=?1");
    db.connection()
        .query_row(&sql, [column], |r| r.get::<_, bool>(0))
        .unwrap_or(false)
}

#[test]
fn test_new_schema_objects_exist_after_migration() {
    // A fresh migrate must produce every object the API-key + mapping features
    // depend on — a regression guard against a migration being dropped or broken.
    let db = Database::open_in_memory().expect("open");
    assert!(
        table_exists(&db, "inbound_client_api_keys"),
        "migration 036 must create inbound_client_api_keys"
    );
    assert!(
        column_exists(&db, "workspace_bindings", "binding_type"),
        "migration 037 must add workspace_bindings.binding_type"
    );
    assert!(
        column_exists(&db, "inbound_clients", "locked_space_id"),
        "migration 038 must add inbound_clients.locked_space_id"
    );
}

#[test]
fn test_pending_migrations_apply_to_an_existing_older_database() {
    // Reproduce the real upgrade that surfaced "no such table:
    // inbound_client_api_keys" in the field: a DB created before 036/037/038
    // existed, reopened by a newer build. The pending migrations MUST apply.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mcpmux.db");

    // 1. Fully migrate, then roll the schema back to a pre-036 state.
    {
        let db = Database::open(&path).expect("open");
        db.connection()
            .execute_batch(
                "DELETE FROM schema_migrations WHERE version >= 36;
                 DROP TABLE IF EXISTS inbound_client_api_keys;
                 DROP INDEX IF EXISTS idx_wb_root_global;
                 DROP INDEX IF EXISTS idx_wb_root_machine;
                 DROP INDEX IF EXISTS idx_wb_root_scoped;
                 DROP INDEX IF EXISTS idx_wb_id_global;
                 DROP INDEX IF EXISTS idx_wb_id_machine;
                 DROP INDEX IF EXISTS idx_workspace_bindings_binding_type;
                 DROP INDEX IF EXISTS idx_inbound_clients_locked_space_id;
                 ALTER TABLE workspace_bindings DROP COLUMN binding_type;
                 ALTER TABLE inbound_clients DROP COLUMN locked_space_id;",
            )
            .expect("roll schema back to pre-036");
        assert!(
            !table_exists(&db, "inbound_client_api_keys"),
            "precondition: the rolled-back DB is missing the table"
        );
    }

    // 2. Reopen — run_migrations() must re-apply 036/037/038.
    let db = Database::open(&path).expect("reopen older DB");
    assert!(
        table_exists(&db, "inbound_client_api_keys"),
        "reopening an older DB must re-create inbound_client_api_keys"
    );
    assert!(column_exists(&db, "workspace_bindings", "binding_type"));
    assert!(column_exists(&db, "inbound_clients", "locked_space_id"));
}
