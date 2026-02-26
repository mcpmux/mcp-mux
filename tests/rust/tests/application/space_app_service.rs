//! Tests for SpaceAppService
//!
//! Validates space creation, deletion, activation, and event emission.

use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::application::SpaceAppService;
use mcpmux_core::domain::DomainEvent;
use mcpmux_core::event_bus::EventBus;
use mcpmux_core::repository::{FeatureSetRepository, SpaceRepository};
use tests::mocks::*;

fn make_service(
    space_repo: Arc<MockSpaceRepository>,
    fs_repo: Arc<MockFeatureSetRepository>,
) -> (
    SpaceAppService,
    tokio::sync::broadcast::Receiver<DomainEvent>,
) {
    let bus = EventBus::new();
    let rx = bus.raw_sender().subscribe();
    let sender = bus.sender();
    let svc = SpaceAppService::new(space_repo, Some(fs_repo), sender);
    (svc, rx)
}

#[tokio::test]
async fn create_first_auto_default() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let space = svc.create("My Space", None).await.unwrap();
    assert!(space.is_default, "First space should auto-become default");
}

#[tokio::test]
async fn create_second_not_default() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let first = svc.create("First", None).await.unwrap();
    assert!(first.is_default);

    let second = svc.create("Second", None).await.unwrap();
    assert!(!second.is_default, "Second space should not be default");
}

#[tokio::test]
async fn create_emits_space_created() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let space = svc
        .create("Test Space", Some("🧪".to_string()))
        .await
        .unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::SpaceCreated {
            space_id,
            name,
            icon,
        } => {
            assert_eq!(space_id, space.id);
            assert_eq!(name, "Test Space");
            assert_eq!(icon, Some("🧪".to_string()));
        }
        other => panic!("Expected SpaceCreated, got {:?}", other),
    }
}

#[tokio::test]
async fn create_with_fs_repo_ensures_builtins() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let space = svc.create("Test", None).await.unwrap();

    let space_id_str = space.id.to_string();
    let all_fs = mocks
        .feature_sets
        .get_all_for_space(&space_id_str)
        .await
        .unwrap();
    assert!(all_fs.is_some(), "All feature set should be created");

    let default_fs = mocks
        .feature_sets
        .get_default_for_space(&space_id_str)
        .await
        .unwrap();
    assert!(
        default_fs.is_some(),
        "Default feature set should be created"
    );
}

#[tokio::test]
async fn delete_default_returns_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let space = svc.create("Default Space", None).await.unwrap();
    assert!(space.is_default);

    let result = svc.delete(space.id).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Cannot delete the default space"));
}

#[tokio::test]
async fn delete_non_default_succeeds() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let _first = svc.create("Default", None).await.unwrap();
    let second = svc.create("Second", None).await.unwrap();
    let _ = rx.try_recv(); // drain first create
    let _ = rx.try_recv(); // drain second create

    svc.delete(second.id).await.unwrap();

    let found = mocks.spaces.get(&second.id).await.unwrap();
    assert!(found.is_none(), "Space should be deleted");

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::SpaceDeleted { space_id } => {
            assert_eq!(space_id, second.id);
        }
        other => panic!("Expected SpaceDeleted, got {:?}", other),
    }
}

#[tokio::test]
async fn delete_not_found_returns_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let result = svc.delete(Uuid::new_v4()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn set_active_emits_with_from_to() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let first = svc.create("First", None).await.unwrap();
    let second = svc.create("Second", None).await.unwrap();
    // first is auto-default since it was first
    mocks.spaces.set_default(&first.id).await.unwrap();
    let _ = rx.try_recv(); // drain events
    let _ = rx.try_recv();

    svc.set_active(second.id).await.unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::SpaceActivated {
            from_space_id,
            to_space_id,
            to_space_name,
        } => {
            assert_eq!(from_space_id, Some(first.id));
            assert_eq!(to_space_id, second.id);
            assert_eq!(to_space_name, "Second");
        }
        other => panic!("Expected SpaceActivated, got {:?}", other),
    }
}

#[tokio::test]
async fn set_active_no_previous_default() {
    // Use a space repo with a manually inserted space but no default set
    let space = mcpmux_core::domain::Space::new("Manual");
    let space_repo = Arc::new(MockSpaceRepository::new().with_space(space.clone()));
    let fs_repo = Arc::new(MockFeatureSetRepository::new());
    let (svc, mut rx) = make_service(space_repo, fs_repo);

    // No default is set, so get_default() returns None
    svc.set_active(space.id).await.unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::SpaceActivated {
            from_space_id,
            to_space_id,
            ..
        } => {
            assert_eq!(from_space_id, None, "No previous default");
            assert_eq!(to_space_id, space.id);
        }
        other => panic!("Expected SpaceActivated, got {:?}", other),
    }
}

#[tokio::test]
async fn update_partial_fields() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.spaces.clone(), mocks.feature_sets.clone());

    let space = svc
        .create("Original", Some("🎯".to_string()))
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain create

    let updated = svc
        .update(space.id, Some("Renamed".to_string()), None, None)
        .await
        .unwrap();

    assert_eq!(updated.name, "Renamed");
    // Icon should be preserved from creation
    assert_eq!(updated.icon, Some("🎯".to_string()));

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::SpaceUpdated { space_id, name } => {
            assert_eq!(space_id, space.id);
            assert_eq!(name, "Renamed");
        }
        other => panic!("Expected SpaceUpdated, got {:?}", other),
    }
}
