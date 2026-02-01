//! Repository integration tests

use mcpmux_storage::SqliteSpaceRepository;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;

// Import the trait to use its methods
use mcpmux_core::repository::SpaceRepository;

#[tokio::test]
async fn test_space_repository_create_and_get() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = SqliteSpaceRepository::new(db);

    // Create
    let space = fixtures::test_space("Test Space");
    let space_id = space.id;

    SpaceRepository::create(&repo, &space)
        .await
        .expect("Failed to create space");

    // Read
    let loaded = SpaceRepository::get(&repo, &space_id)
        .await
        .expect("Failed to get space");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.name, "Test Space");
    assert_eq!(loaded.icon, Some("🧪".to_string()));
}

#[tokio::test]
async fn test_space_repository_update() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = SqliteSpaceRepository::new(db);

    // Create
    let space = fixtures::test_space("Original Name");
    let space_id = space.id;
    SpaceRepository::create(&repo, &space)
        .await
        .expect("Failed to create space");

    // Update
    let mut updated_space = SpaceRepository::get(&repo, &space_id)
        .await
        .unwrap()
        .unwrap();
    updated_space.name = "Updated Name".to_string();
    SpaceRepository::update(&repo, &updated_space)
        .await
        .expect("Failed to update space");

    // Verify
    let reloaded = SpaceRepository::get(&repo, &space_id)
        .await
        .expect("Failed to reload space");
    assert_eq!(reloaded.unwrap().name, "Updated Name");
}

#[tokio::test]
async fn test_space_repository_delete() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("To Delete");
    let space_id = space.id;
    SpaceRepository::create(&repo, &space).await.unwrap();

    // Delete
    SpaceRepository::delete(&repo, &space_id)
        .await
        .expect("Failed to delete space");

    // Verify deleted
    let deleted = SpaceRepository::get(&repo, &space_id)
        .await
        .expect("Failed to check deleted");
    assert!(deleted.is_none());
}

#[tokio::test]
async fn test_space_repository_list_all() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = SqliteSpaceRepository::new(db);

    // Get initial count (database may have a default space)
    let initial_spaces = SpaceRepository::list(&repo)
        .await
        .expect("Failed to list initial spaces");
    let initial_count = initial_spaces.len();

    // Create multiple spaces
    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    let space3 = fixtures::test_space("Space 3");

    SpaceRepository::create(&repo, &space1).await.unwrap();
    SpaceRepository::create(&repo, &space2).await.unwrap();
    SpaceRepository::create(&repo, &space3).await.unwrap();

    // List all - should have 3 more than initial
    let all_spaces = SpaceRepository::list(&repo)
        .await
        .expect("Failed to list spaces");
    assert_eq!(all_spaces.len(), initial_count + 3);
}

#[tokio::test]
async fn test_space_repository_default_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = SqliteSpaceRepository::new(db);

    // Create a non-default space first
    let other_space = fixtures::test_space("Other Space");
    SpaceRepository::create(&repo, &other_space).await.unwrap();

    // Create and set a default space using set_default (more reliable)
    let my_default_space = fixtures::test_space("My Default");
    let my_default_id = my_default_space.id;
    SpaceRepository::create(&repo, &my_default_space)
        .await
        .unwrap();
    SpaceRepository::set_default(&repo, &my_default_id)
        .await
        .unwrap();

    // Get default - should be our space
    let found_default = SpaceRepository::get_default(&repo)
        .await
        .expect("Failed to get default");
    assert!(found_default.is_some());
    assert_eq!(found_default.unwrap().id, my_default_id);
}

#[tokio::test]
async fn test_space_repository_set_default() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = SqliteSpaceRepository::new(db);

    // Create two spaces
    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    let space1_id = space1.id;
    let space2_id = space2.id;

    SpaceRepository::create(&repo, &space1).await.unwrap();
    SpaceRepository::create(&repo, &space2).await.unwrap();

    // Set space1 as default
    SpaceRepository::set_default(&repo, &space1_id)
        .await
        .expect("Failed to set default");

    let default = SpaceRepository::get_default(&repo).await.unwrap().unwrap();
    assert_eq!(default.id, space1_id);

    // Change default to space2
    SpaceRepository::set_default(&repo, &space2_id)
        .await
        .expect("Failed to change default");

    let new_default = SpaceRepository::get_default(&repo).await.unwrap().unwrap();
    assert_eq!(new_default.id, space2_id);

    // Verify space1 is no longer default
    let space1_reloaded = SpaceRepository::get(&repo, &space1_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!space1_reloaded.is_default);
}

#[tokio::test]
async fn test_space_repository_concurrent_reads() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = Arc::new(SqliteSpaceRepository::new(db));

    // Create a space
    let space = fixtures::test_space("Concurrent Test");
    let space_id = space.id.clone();
    SpaceRepository::create(repo.as_ref(), &space)
        .await
        .unwrap();

    // Spawn multiple concurrent reads
    let mut handles = vec![];
    for _ in 0..5 {
        let repo_clone = Arc::clone(&repo);
        let id = space_id.clone();
        handles.push(tokio::spawn(async move {
            SpaceRepository::get(repo_clone.as_ref(), &id).await
        }));
    }

    // All reads should succeed
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }
}

#[tokio::test]
async fn test_space_repository_concurrent_writes() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = Arc::new(SqliteSpaceRepository::new(db));

    // Spawn multiple concurrent creates
    let mut handles = vec![];
    for i in 0..5 {
        let repo_clone = Arc::clone(&repo);
        handles.push(tokio::spawn(async move {
            let space = fixtures::test_space(&format!("Concurrent Space {}", i));
            SpaceRepository::create(repo_clone.as_ref(), &space).await
        }));
    }

    // All writes should succeed
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok());
    }

    // Verify all spaces were created
    let all_spaces = SpaceRepository::list(repo.as_ref()).await.unwrap();
    let concurrent_count = all_spaces
        .iter()
        .filter(|s| s.name.starts_with("Concurrent Space"))
        .count();
    assert_eq!(concurrent_count, 5);
}
