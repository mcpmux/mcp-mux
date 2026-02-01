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
