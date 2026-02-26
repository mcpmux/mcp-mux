//! Authorization service integration tests
//!
//! Tests for AuthorizationService using real SQLite repositories.
//! Note: SqliteSpaceRepository::create auto-creates builtin feature sets
//! (All + Default) for each space, and the DB migration seeds a default space.

use mcpmux_core::FeatureSetRepository;
use mcpmux_gateway::services::AuthorizationService;
use mcpmux_storage::{
    InboundClient, InboundClientRepository, RegistrationType, SqliteFeatureSetRepository,
    SqliteSpaceRepository,
};
use std::sync::Arc;
use tests::db::TestDatabase;
use tests::fixtures;
use tokio::sync::Mutex;
use uuid::Uuid;

fn test_client(client_id: &str) -> InboundClient {
    InboundClient {
        client_id: client_id.to_string(),
        registration_type: RegistrationType::Preregistered,
        client_name: "Test Client".to_string(),
        client_alias: None,
        redirect_uris: vec![],
        grant_types: vec![],
        response_types: vec![],
        token_endpoint_auth_method: "none".to_string(),
        scope: None,
        approved: true,
        logo_uri: None,
        client_uri: None,
        software_id: None,
        software_version: None,
        metadata_url: None,
        metadata_cached_at: None,
        metadata_cache_ttl: None,
        connection_mode: "follow_active".to_string(),
        locked_space_id: None,
        last_seen: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

struct TestSetup {
    svc: AuthorizationService,
    client_repo: Arc<InboundClientRepository>,
    fs_repo: Arc<SqliteFeatureSetRepository>,
    space_id: Uuid,
}

/// Creates a test setup with a real space.
/// SqliteSpaceRepository::create auto-creates Default + All feature sets.
async fn setup_with_space() -> TestSetup {
    let test_db = TestDatabase::in_memory();
    let db = Arc::new(Mutex::new(test_db.db));
    let client_repo = Arc::new(InboundClientRepository::new(db.clone()));
    let fs_repo = Arc::new(SqliteFeatureSetRepository::new(db.clone()));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Auth Test Space");
    use mcpmux_core::SpaceRepository;
    space_repo.create(&space).await.unwrap();

    let svc = AuthorizationService::new(client_repo.clone(), fs_repo.clone());
    TestSetup {
        svc,
        client_repo,
        fs_repo,
        space_id: space.id,
    }
}

#[tokio::test]
async fn explicit_grants_plus_default_fs() {
    let t = setup_with_space().await;
    let space_id_str = t.space_id.to_string();

    // Default FS is auto-created by space_repo.create, so it already exists
    let default_fs_id = format!("fs_default_{}", space_id_str);

    // Create a custom FS in DB for the explicit grant
    let custom_fs = mcpmux_core::domain::FeatureSet {
        id: "custom-fs-1".to_string(),
        name: "Custom".to_string(),
        description: None,
        icon: None,
        space_id: Some(space_id_str.clone()),
        feature_set_type: mcpmux_core::domain::FeatureSetType::Custom,
        server_id: None,
        is_builtin: false,
        is_deleted: false,
        members: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    t.fs_repo.create(&custom_fs).await.unwrap();

    let client = test_client("client-1");
    t.client_repo.save_client(&client).await.unwrap();
    t.client_repo
        .grant_feature_set("client-1", &space_id_str, "custom-fs-1")
        .await
        .unwrap();

    let grants = t
        .svc
        .get_client_grants("client-1", &t.space_id)
        .await
        .unwrap();

    assert!(grants.contains(&"custom-fs-1".to_string()));
    assert!(grants.contains(&default_fs_id));
    assert_eq!(grants.len(), 2);
}

#[tokio::test]
async fn no_explicit_grants_gets_default() {
    let t = setup_with_space().await;
    let default_fs_id = format!("fs_default_{}", t.space_id);

    let client = test_client("client-2");
    t.client_repo.save_client(&client).await.unwrap();

    let grants = t
        .svc
        .get_client_grants("client-2", &t.space_id)
        .await
        .unwrap();

    assert_eq!(grants, vec![default_fs_id]);
}

#[tokio::test]
async fn no_duplicate_default_fs() {
    let t = setup_with_space().await;
    let space_id_str = t.space_id.to_string();
    let default_fs_id = format!("fs_default_{}", space_id_str);

    // Grant the default FS explicitly (it was already auto-created)
    let client = test_client("client-3");
    t.client_repo.save_client(&client).await.unwrap();
    t.client_repo
        .grant_feature_set("client-3", &space_id_str, &default_fs_id)
        .await
        .unwrap();

    let grants = t
        .svc
        .get_client_grants("client-3", &t.space_id)
        .await
        .unwrap();

    assert_eq!(
        grants.iter().filter(|g| *g == &default_fs_id).count(),
        1,
        "Default FS should appear exactly once"
    );
}

#[tokio::test]
async fn no_default_fs_means_no_baseline() {
    // Use a space ID that doesn't exist in the DB as feature_set space
    // to test the path where get_default_for_space returns None.
    let test_db = TestDatabase::in_memory();
    let db = Arc::new(Mutex::new(test_db.db));
    let client_repo = Arc::new(InboundClientRepository::new(db.clone()));
    let fs_repo = Arc::new(SqliteFeatureSetRepository::new(db));

    let svc = AuthorizationService::new(client_repo.clone(), fs_repo);

    // Use the seeded space from migration but query with a random space_id
    let client = test_client("client-4");
    client_repo.save_client(&client).await.unwrap();

    // Query with a space that has no feature sets at all
    let random_space_id = Uuid::new_v4();
    let grants = svc
        .get_client_grants("client-4", &random_space_id)
        .await
        .unwrap();

    // No explicit grants and no Default FS for this space = empty
    assert!(grants.is_empty());
}

#[tokio::test]
async fn has_access_with_default_fs() {
    let t = setup_with_space().await;

    let client = test_client("client-5");
    t.client_repo.save_client(&client).await.unwrap();

    // Should have access via the auto-created default FS baseline
    assert!(t.svc.has_access("client-5", &t.space_id).await.unwrap());
}

#[tokio::test]
async fn has_feature_set_access_granted_vs_not() {
    let t = setup_with_space().await;
    let space_id_str = t.space_id.to_string();

    // Create a custom FS to grant
    let granted_fs = mcpmux_core::domain::FeatureSet {
        id: "granted-fs".to_string(),
        name: "Granted".to_string(),
        description: None,
        icon: None,
        space_id: Some(space_id_str.clone()),
        feature_set_type: mcpmux_core::domain::FeatureSetType::Custom,
        server_id: None,
        is_builtin: false,
        is_deleted: false,
        members: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    t.fs_repo.create(&granted_fs).await.unwrap();

    let client = test_client("client-6");
    t.client_repo.save_client(&client).await.unwrap();
    t.client_repo
        .grant_feature_set("client-6", &space_id_str, "granted-fs")
        .await
        .unwrap();

    assert!(t
        .svc
        .has_feature_set_access("client-6", &t.space_id, "granted-fs")
        .await
        .unwrap());

    assert!(!t
        .svc
        .has_feature_set_access("client-6", &t.space_id, "missing-fs")
        .await
        .unwrap());
}
