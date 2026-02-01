//! FeatureSetRepository integration tests
//!
//! Tests for feature set CRUD, builtin types (All, Default, ServerAll),
//! and feature member composition.

use mcpmux_core::domain::{FeatureSetType, MemberMode};
use mcpmux_core::repository::{FeatureSetRepository, SpaceRepository};
use mcpmux_storage::{SqliteFeatureSetRepository, SqliteSpaceRepository};
use std::sync::Arc;
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;

// =============================================================================
// FeatureSet CRUD Tests
// =============================================================================

#[tokio::test]
async fn test_create_and_get_feature_set() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // Create a space first
    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Create a custom feature set
    let fs = fixtures::test_feature_set("My Tools", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .expect("Failed to create");

    // Get by ID
    let loaded = FeatureSetRepository::get(&feature_repo, &fs.id)
        .await
        .expect("Failed to get");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.name, "My Tools");
    assert_eq!(loaded.feature_set_type, FeatureSetType::Custom);
}

#[tokio::test]
async fn test_list_by_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    SpaceRepository::create(&space_repo, &space1).await.unwrap();
    SpaceRepository::create(&space_repo, &space2).await.unwrap();

    // Create feature sets in different spaces
    let fs1 = fixtures::test_feature_set("FS 1", &space1.id.to_string());
    let fs2 = fixtures::test_feature_set("FS 2", &space1.id.to_string());
    let fs3 = fixtures::test_feature_set("FS 3", &space2.id.to_string());

    FeatureSetRepository::create(&feature_repo, &fs1)
        .await
        .unwrap();
    FeatureSetRepository::create(&feature_repo, &fs2)
        .await
        .unwrap();
    FeatureSetRepository::create(&feature_repo, &fs3)
        .await
        .unwrap();

    // List for space1: 2 custom + 2 builtin (All, Default) = 4
    let space1_sets = FeatureSetRepository::list_by_space(&feature_repo, &space1.id.to_string())
        .await
        .expect("Failed to list");
    assert_eq!(space1_sets.len(), 4);

    // List for space2: 1 custom + 2 builtin = 3
    let space2_sets = FeatureSetRepository::list_by_space(&feature_repo, &space2.id.to_string())
        .await
        .expect("Failed to list");
    assert_eq!(space2_sets.len(), 3);
}

#[tokio::test]
async fn test_update_feature_set() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut fs = fixtures::test_feature_set("Original", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .unwrap();

    // Update
    fs.name = "Updated Name".to_string();
    fs.description = Some("New description".to_string());
    FeatureSetRepository::update(&feature_repo, &fs)
        .await
        .expect("Failed to update");

    // Verify
    let loaded = FeatureSetRepository::get(&feature_repo, &fs.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.name, "Updated Name");
    assert_eq!(loaded.description, Some("New description".to_string()));
}

#[tokio::test]
async fn test_delete_feature_set() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let fs = fixtures::test_feature_set("To Delete", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .unwrap();

    // Delete
    FeatureSetRepository::delete(&feature_repo, &fs.id)
        .await
        .expect("Failed to delete");

    // Verify gone
    let loaded = FeatureSetRepository::get(&feature_repo, &fs.id)
        .await
        .unwrap();
    assert!(loaded.is_none());
}

// =============================================================================
// Builtin Feature Sets Tests
// =============================================================================

#[tokio::test]
async fn test_ensure_builtin_for_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Ensure builtin (All + Default)
    FeatureSetRepository::ensure_builtin_for_space(&feature_repo, &space.id.to_string())
        .await
        .expect("Failed to ensure builtin");

    // Get All feature set
    let all_set = FeatureSetRepository::get_all_for_space(&feature_repo, &space.id.to_string())
        .await
        .expect("Failed to get All");
    assert!(all_set.is_some());
    assert_eq!(all_set.unwrap().feature_set_type, FeatureSetType::All);

    // Get Default feature set
    let default_set =
        FeatureSetRepository::get_default_for_space(&feature_repo, &space.id.to_string())
            .await
            .expect("Failed to get Default");
    assert!(default_set.is_some());
    assert_eq!(
        default_set.unwrap().feature_set_type,
        FeatureSetType::Default
    );
}

#[tokio::test]
async fn test_ensure_builtin_idempotent() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Call twice
    FeatureSetRepository::ensure_builtin_for_space(&feature_repo, &space.id.to_string())
        .await
        .unwrap();
    FeatureSetRepository::ensure_builtin_for_space(&feature_repo, &space.id.to_string())
        .await
        .unwrap();

    // Should still have exactly 2 builtin sets
    let builtin = FeatureSetRepository::list_builtin(&feature_repo, &space.id.to_string())
        .await
        .expect("Failed to list builtin");
    assert_eq!(builtin.len(), 2);
}

#[tokio::test]
async fn test_server_all_feature_set() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Create server-all feature set
    let server_all = FeatureSetRepository::ensure_server_all(
        &feature_repo,
        &space.id.to_string(),
        "my-server",
        "My Server",
    )
    .await
    .expect("Failed to ensure server-all");

    assert_eq!(server_all.feature_set_type, FeatureSetType::ServerAll);
    assert_eq!(server_all.server_id, Some("my-server".to_string()));

    // Get by server_id
    let found =
        FeatureSetRepository::get_server_all(&feature_repo, &space.id.to_string(), "my-server")
            .await
            .expect("Failed to get server-all");
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, server_all.id);
}

#[tokio::test]
async fn test_delete_server_all() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Create then delete
    FeatureSetRepository::ensure_server_all(&feature_repo, &space.id.to_string(), "srv", "Srv")
        .await
        .unwrap();

    FeatureSetRepository::delete_server_all(&feature_repo, &space.id.to_string(), "srv")
        .await
        .expect("Failed to delete server-all");

    // Should be gone
    let found = FeatureSetRepository::get_server_all(&feature_repo, &space.id.to_string(), "srv")
        .await
        .unwrap();
    assert!(found.is_none());
}

// =============================================================================
// Feature Members Tests
// =============================================================================

#[tokio::test]
async fn test_add_feature_member() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let fs = fixtures::test_feature_set("Custom Set", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .unwrap();

    // Add feature member
    FeatureSetRepository::add_feature_member(
        &feature_repo,
        &fs.id,
        "feature-uuid-1",
        MemberMode::Include,
    )
    .await
    .expect("Failed to add member");

    // Get members
    let members = FeatureSetRepository::get_feature_members(&feature_repo, &fs.id)
        .await
        .expect("Failed to get members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].member_id, "feature-uuid-1");
    assert_eq!(members[0].mode, MemberMode::Include);
}

#[tokio::test]
async fn test_add_multiple_feature_members() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let fs = fixtures::test_feature_set("Multi Member", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .unwrap();

    // Add multiple
    FeatureSetRepository::add_feature_member(&feature_repo, &fs.id, "tool-1", MemberMode::Include)
        .await
        .unwrap();
    FeatureSetRepository::add_feature_member(&feature_repo, &fs.id, "tool-2", MemberMode::Include)
        .await
        .unwrap();
    FeatureSetRepository::add_feature_member(
        &feature_repo,
        &fs.id,
        "dangerous-tool",
        MemberMode::Exclude,
    )
    .await
    .unwrap();

    let members = FeatureSetRepository::get_feature_members(&feature_repo, &fs.id)
        .await
        .unwrap();
    assert_eq!(members.len(), 3);

    let excluded = members.iter().find(|m| m.mode == MemberMode::Exclude);
    assert!(excluded.is_some());
    assert_eq!(excluded.unwrap().member_id, "dangerous-tool");
}

#[tokio::test]
async fn test_remove_feature_member() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let fs = fixtures::test_feature_set("Remove Test", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .unwrap();

    // Add then remove
    FeatureSetRepository::add_feature_member(
        &feature_repo,
        &fs.id,
        "feature-a",
        MemberMode::Include,
    )
    .await
    .unwrap();
    FeatureSetRepository::add_feature_member(
        &feature_repo,
        &fs.id,
        "feature-b",
        MemberMode::Include,
    )
    .await
    .unwrap();

    FeatureSetRepository::remove_feature_member(&feature_repo, &fs.id, "feature-a")
        .await
        .expect("Failed to remove");

    // Only feature-b remains
    let members = FeatureSetRepository::get_feature_members(&feature_repo, &fs.id)
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].member_id, "feature-b");
}

#[tokio::test]
async fn test_get_with_members() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let fs = fixtures::test_feature_set("With Members", &space.id.to_string());
    FeatureSetRepository::create(&feature_repo, &fs)
        .await
        .unwrap();

    FeatureSetRepository::add_feature_member(&feature_repo, &fs.id, "tool-1", MemberMode::Include)
        .await
        .unwrap();
    FeatureSetRepository::add_feature_member(&feature_repo, &fs.id, "tool-2", MemberMode::Include)
        .await
        .unwrap();

    // Get with members
    let loaded = FeatureSetRepository::get_with_members(&feature_repo, &fs.id)
        .await
        .expect("Failed to get with members");

    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.members.len(), 2);
}

// =============================================================================
// Feature Set Type Tests
// =============================================================================

#[tokio::test]
async fn test_feature_set_types() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Note: SpaceRepository::create auto-creates All and Default feature sets
    // So we only need to create Custom and ServerAll here
    let custom = fixtures::test_feature_set("Custom", &space.id.to_string());
    let server_all = fixtures::server_all_feature_set(&space.id.to_string(), "srv", "Server");

    FeatureSetRepository::create(&feature_repo, &custom)
        .await
        .unwrap();
    FeatureSetRepository::create(&feature_repo, &server_all)
        .await
        .unwrap();

    // Verify types - use the auto-created IDs for All and Default
    let all_id = format!("fs_all_{}", space.id);
    let default_id = format!("fs_default_{}", space.id);

    let all_loaded = FeatureSetRepository::get(&feature_repo, &all_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(all_loaded.feature_set_type, FeatureSetType::All);

    let default_loaded = FeatureSetRepository::get(&feature_repo, &default_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(default_loaded.feature_set_type, FeatureSetType::Default);

    let custom_loaded = FeatureSetRepository::get(&feature_repo, &custom.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(custom_loaded.feature_set_type, FeatureSetType::Custom);

    let server_all_loaded = FeatureSetRepository::get(&feature_repo, &server_all.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        server_all_loaded.feature_set_type,
        FeatureSetType::ServerAll
    );
}

// =============================================================================
// Space Isolation Tests
// =============================================================================

#[tokio::test]
async fn test_feature_set_space_isolation() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let feature_repo = SqliteFeatureSetRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let work = fixtures::test_space("Work");
    let personal = fixtures::test_space("Personal");
    SpaceRepository::create(&space_repo, &work).await.unwrap();
    SpaceRepository::create(&space_repo, &personal)
        .await
        .unwrap();

    // Create same-named feature sets in different spaces
    let work_tools = fixtures::test_feature_set("Development", &work.id.to_string());
    let personal_tools = fixtures::test_feature_set("Development", &personal.id.to_string());

    FeatureSetRepository::create(&feature_repo, &work_tools)
        .await
        .unwrap();
    FeatureSetRepository::create(&feature_repo, &personal_tools)
        .await
        .unwrap();

    // They should be independent
    // Each space has 2 builtin (All, Default) + 1 custom = 3
    let work_sets = FeatureSetRepository::list_by_space(&feature_repo, &work.id.to_string())
        .await
        .unwrap();
    let personal_sets =
        FeatureSetRepository::list_by_space(&feature_repo, &personal.id.to_string())
            .await
            .unwrap();

    assert_eq!(work_sets.len(), 3);
    assert_eq!(personal_sets.len(), 3);

    // Verify the custom sets are different
    let work_custom: Vec<_> = work_sets
        .iter()
        .filter(|s| s.name == "Development")
        .collect();
    let personal_custom: Vec<_> = personal_sets
        .iter()
        .filter(|s| s.name == "Development")
        .collect();
    assert_eq!(work_custom.len(), 1);
    assert_eq!(personal_custom.len(), 1);
    assert_ne!(work_custom[0].id, personal_custom[0].id);
}
