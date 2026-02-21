//! Tests for PermissionAppService
//!
//! Validates feature set CRUD, member management, and grant operations
//! with event emission.

use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::application::PermissionAppService;
use mcpmux_core::domain::{Client, DomainEvent, MemberMode};
use mcpmux_core::event_bus::EventBus;
use mcpmux_core::repository::{FeatureSetRepository, InboundMcpClientRepository};
use tests::mocks::*;

fn make_service(
    fs_repo: Arc<MockFeatureSetRepository>,
    client_repo: Option<Arc<MockInboundMcpClientRepository>>,
) -> (
    PermissionAppService,
    tokio::sync::broadcast::Receiver<DomainEvent>,
) {
    let bus = EventBus::new();
    let rx = bus.raw_sender().subscribe();
    let sender = bus.sender();
    let svc = PermissionAppService::new(
        fs_repo,
        client_repo.map(|r| r as Arc<dyn mcpmux_core::repository::InboundMcpClientRepository>),
        sender,
    );
    (svc, rx)
}

#[tokio::test]
async fn create_fs_persists_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(&space_id.to_string(), "My FS", None, None)
        .await
        .unwrap();

    // Verify persisted
    let found = mocks.feature_sets.get(&fs.id).await.unwrap();
    assert!(found.is_some());

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::FeatureSetCreated {
            space_id: sid,
            feature_set_id,
            name,
            ..
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(feature_set_id, fs.id);
            assert_eq!(name, "My FS");
        }
        other => panic!("Expected FeatureSetCreated, got {:?}", other),
    }
}

#[tokio::test]
async fn create_fs_with_desc_icon() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(
            &space_id.to_string(),
            "Fancy FS",
            Some("A description".to_string()),
            Some("🔧".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(fs.description, Some("A description".to_string()));
    assert_eq!(fs.icon, Some("🔧".to_string()));
}

#[tokio::test]
async fn update_fs_changes_name_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(&space_id.to_string(), "Original", None, None)
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain create

    let updated = svc
        .update_feature_set(&fs.id, Some("Renamed".to_string()), None, None)
        .await
        .unwrap();

    assert_eq!(updated.name, "Renamed");

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::FeatureSetUpdated {
            feature_set_id,
            name,
            ..
        } => {
            assert_eq!(feature_set_id, fs.id);
            assert_eq!(name, "Renamed");
        }
        other => panic!("Expected FeatureSetUpdated, got {:?}", other),
    }
}

#[tokio::test]
async fn update_fs_not_found_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.feature_sets.clone(), None);

    let result = svc
        .update_feature_set("nonexistent", Some("Name".to_string()), None, None)
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn delete_custom_fs_succeeds() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(&space_id.to_string(), "To Delete", None, None)
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain create

    svc.delete_feature_set(&fs.id).await.unwrap();

    let found = mocks.feature_sets.get(&fs.id).await.unwrap();
    assert!(found.is_none());

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::FeatureSetDeleted {
            space_id: sid,
            feature_set_id,
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(feature_set_id, fs.id);
        }
        other => panic!("Expected FeatureSetDeleted, got {:?}", other),
    }
}

#[tokio::test]
async fn delete_builtin_fs_returns_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let all_fs = mcpmux_core::domain::FeatureSet::new_all(&space_id.to_string());
    mocks.feature_sets.create(&all_fs).await.unwrap();

    let result = svc.delete_feature_set(&all_fs.id).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Cannot delete builtin"));
}

#[tokio::test]
async fn add_member_persists_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(&space_id.to_string(), "Test FS", None, None)
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain create

    svc.add_feature_member(&fs.id, "feature-123", MemberMode::Include)
        .await
        .unwrap();

    let members = mocks
        .feature_sets
        .get_feature_members(&fs.id)
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].member_id, "feature-123");

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::FeatureSetMembersChanged {
            feature_set_id,
            added_count,
            removed_count,
            ..
        } => {
            assert_eq!(feature_set_id, fs.id);
            assert_eq!(added_count, 1);
            assert_eq!(removed_count, 0);
        }
        other => panic!("Expected FeatureSetMembersChanged, got {:?}", other),
    }
}

#[tokio::test]
async fn remove_member_persists_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.feature_sets.clone(), None);

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(&space_id.to_string(), "Test FS", None, None)
        .await
        .unwrap();
    svc.add_feature_member(&fs.id, "feature-123", MemberMode::Include)
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain create
    let _ = rx.try_recv(); // drain add

    svc.remove_feature_member(&fs.id, "feature-123")
        .await
        .unwrap();

    let members = mocks
        .feature_sets
        .get_feature_members(&fs.id)
        .await
        .unwrap();
    assert!(members.is_empty());

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::FeatureSetMembersChanged {
            added_count,
            removed_count,
            ..
        } => {
            assert_eq!(added_count, 0);
            assert_eq!(removed_count, 1);
        }
        other => panic!("Expected FeatureSetMembersChanged, got {:?}", other),
    }
}

#[tokio::test]
async fn grant_no_client_repo_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.feature_sets.clone(), None);

    let result = svc
        .grant_feature_set(Uuid::new_v4(), &Uuid::new_v4().to_string(), "fs-123")
        .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Client repository not configured"));
}

#[tokio::test]
async fn grant_client_not_found_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.feature_sets.clone(), Some(mocks.clients.clone()));

    let space_id = Uuid::new_v4();
    let fs = svc
        .create_feature_set(&space_id.to_string(), "FS", None, None)
        .await
        .unwrap();

    let result = svc
        .grant_feature_set(Uuid::new_v4(), &space_id.to_string(), &fs.id)
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Client not found"));
}

#[tokio::test]
async fn grant_fs_not_found_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.feature_sets.clone(), Some(mocks.clients.clone()));

    let client = Client::new("Test", "api_key");
    mocks.clients.create(&client).await.unwrap();

    let result = svc
        .grant_feature_set(client.id, &Uuid::new_v4().to_string(), "nonexistent")
        .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Feature set not found"));
}

#[tokio::test]
async fn grant_succeeds_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.feature_sets.clone(), Some(mocks.clients.clone()));

    let space_id = Uuid::new_v4();
    let space_id_str = space_id.to_string();

    let client = Client::new("Test", "api_key");
    mocks.clients.create(&client).await.unwrap();

    let fs = svc
        .create_feature_set(&space_id_str, "My FS", None, None)
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain FS create

    svc.grant_feature_set(client.id, &space_id_str, &fs.id)
        .await
        .unwrap();

    // Verify grant persisted
    let grants = mocks
        .clients
        .get_grants_for_space(&client.id, &space_id_str)
        .await
        .unwrap();
    assert!(grants.contains(&fs.id));

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::GrantIssued {
            client_id,
            space_id: sid,
            feature_set_id,
        } => {
            assert_eq!(client_id, client.id.to_string());
            assert_eq!(sid, space_id);
            assert_eq!(feature_set_id, fs.id);
        }
        other => panic!("Expected GrantIssued, got {:?}", other),
    }
}
