//! Space resolver service integration tests
//!
//! Tests for SpaceResolverService using real InboundClientRepository (SQLite)
//! and MockSpaceRepository (for controllable space resolution).

use mcpmux_core::SpaceRepository;
use mcpmux_gateway::services::SpaceResolverService;
use mcpmux_storage::{InboundClient, InboundClientRepository, RegistrationType};
use std::sync::Arc;
use tests::db::TestDatabase;
use tests::fixtures;
use tests::mocks::MockSpaceRepository;
use tokio::sync::Mutex;
use uuid::Uuid;

fn test_client_with_mode(
    client_id: &str,
    mode: &str,
    locked_space_id: Option<String>,
) -> InboundClient {
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
        connection_mode: mode.to_string(),
        locked_space_id,
        last_seen: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

struct TestSetup {
    client_repo: Arc<InboundClientRepository>,
    space_repo: Arc<MockSpaceRepository>,
}

async fn setup() -> TestSetup {
    let test_db = TestDatabase::in_memory();
    let db = Arc::new(Mutex::new(test_db.db));
    let client_repo = Arc::new(InboundClientRepository::new(db));
    let space_repo = Arc::new(MockSpaceRepository::new());
    TestSetup {
        client_repo,
        space_repo,
    }
}

#[tokio::test]
async fn locked_mode_returns_locked_space() {
    let t = setup().await;

    // Use the seeded default space ID to satisfy FK constraint on locked_space_id
    let locked_id: Uuid = "00000000-0000-0000-0000-000000000001".parse().unwrap();

    let client = test_client_with_mode("client-locked", "locked", Some(locked_id.to_string()));
    t.client_repo.save_client(&client).await.unwrap();

    let svc = SpaceResolverService::new(t.client_repo, t.space_repo);
    let result = svc.resolve_space_for_client("client-locked").await.unwrap();
    assert_eq!(result, locked_id);
}

#[tokio::test]
async fn locked_no_space_id_err() {
    let t = setup().await;

    let client = test_client_with_mode("client-locked-no-id", "locked", None);
    t.client_repo.save_client(&client).await.unwrap();

    let svc = SpaceResolverService::new(t.client_repo, t.space_repo);
    let result = svc.resolve_space_for_client("client-locked-no-id").await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("no locked_space_id"),
        "Error should mention missing locked_space_id"
    );
}

#[tokio::test]
async fn follow_active_returns_default() {
    let t = setup().await;

    // Set up a default space in the mock
    let space = fixtures::test_space("Active Space");
    t.space_repo.create(&space).await.unwrap();
    t.space_repo.set_default(&space.id).await.unwrap();

    let client = test_client_with_mode("client-follow", "follow_active", None);
    t.client_repo.save_client(&client).await.unwrap();

    let svc = SpaceResolverService::new(t.client_repo, t.space_repo);
    let result = svc.resolve_space_for_client("client-follow").await.unwrap();
    assert_eq!(result, space.id);
}

#[tokio::test]
async fn follow_active_no_default_err() {
    let t = setup().await;

    // MockSpaceRepository starts empty — no default space
    let client = test_client_with_mode("client-no-default", "follow_active", None);
    t.client_repo.save_client(&client).await.unwrap();

    let svc = SpaceResolverService::new(t.client_repo, t.space_repo);
    let result = svc.resolve_space_for_client("client-no-default").await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("No active space"),
        "Error should mention no active space"
    );
}

#[tokio::test]
async fn unknown_mode_falls_back() {
    let t = setup().await;

    let space = fixtures::test_space("Fallback Space");
    t.space_repo.create(&space).await.unwrap();
    t.space_repo.set_default(&space.id).await.unwrap();

    let client = test_client_with_mode("client-unknown", "some_future_mode", None);
    t.client_repo.save_client(&client).await.unwrap();

    let svc = SpaceResolverService::new(t.client_repo, t.space_repo);
    let result = svc
        .resolve_space_for_client("client-unknown")
        .await
        .unwrap();
    assert_eq!(result, space.id);
}
