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
    const FRESH: &str = "Auto-created with this Space. Unmapped folders fall back to this set — it's the default toolset for anything you haven't explicitly mapped. Edit or rename it to change what they get; it can't be deleted.";

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
