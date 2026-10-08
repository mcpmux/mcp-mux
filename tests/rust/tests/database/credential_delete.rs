//! Deleting a credential removes its ciphertext from the database files.

use mcpmux_core::repository::{CredentialRepository, SpaceRepository};
use mcpmux_core::Credential;
use mcpmux_storage::{
    generate_master_key, Database, FieldEncryptor, SqliteCredentialRepository,
    SqliteSpaceRepository,
};
use std::sync::Arc;
use tests::fixtures;
use tokio::sync::Mutex;

fn file_contains(path: &std::path::Path, needle: &[u8]) -> bool {
    std::fs::read(path)
        .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
        .unwrap_or(false)
}

#[tokio::test]
async fn deleted_credentials_leave_no_ciphertext_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcpmux.db");
    let db = Arc::new(Mutex::new(Database::open(&path).unwrap()));
    let key = generate_master_key().unwrap();
    let repo = SqliteCredentialRepository::new(
        Arc::clone(&db),
        Arc::new(FieldEncryptor::new(&key).unwrap()),
    );
    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&SqliteSpaceRepository::new(Arc::clone(&db)), &space)
        .await
        .unwrap();

    let credential = Credential::api_key(space.id, "srv", "sk-live-0123456789");
    repo.save(&credential).await.unwrap();
    let ciphertext: String = db
        .lock()
        .await
        .connection()
        .query_row("SELECT credential_value FROM credentials", [], |r| r.get(0))
        .unwrap();
    // Make sure the row really reached the main database file first.
    db.lock().await.checkpoint_wal();
    assert!(file_contains(&path, ciphertext.as_bytes()));

    repo.delete_all(&space.id, "srv").await.unwrap();

    let wal = dir.path().join("mcpmux.db-wal");
    for file in [&path, &wal] {
        assert!(
            !file_contains(file, ciphertext.as_bytes()),
            "ciphertext still in {}",
            file.display()
        );
    }
}

#[test]
fn credential_debug_output_hides_the_value() {
    let credential = Credential::api_key(uuid::Uuid::new_v4(), "srv", "sk-live-0123456789");
    let printed = format!("{credential:?}");
    assert!(!printed.contains("sk-live"), "{printed}");
    assert!(printed.contains("<redacted>"));
}
