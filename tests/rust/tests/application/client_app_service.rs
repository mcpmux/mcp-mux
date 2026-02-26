//! Tests for ClientAppService
//!
//! Validates client creation, OAuth registration, updates, deletion,
//! and event emission.

use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::application::ClientAppService;
use mcpmux_core::domain::DomainEvent;
use mcpmux_core::event_bus::EventBus;
use mcpmux_core::repository::InboundMcpClientRepository;
use tests::mocks::*;

fn make_service(
    client_repo: Arc<MockInboundMcpClientRepository>,
) -> (
    ClientAppService,
    tokio::sync::broadcast::Receiver<DomainEvent>,
) {
    let bus = EventBus::new();
    let rx = bus.raw_sender().subscribe();
    let sender = bus.sender();
    let svc = ClientAppService::new(client_repo, sender);
    (svc, rx)
}

#[tokio::test]
async fn create_generates_key_persists_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.clients.clone());

    let client = svc.create("Test Client", "api_key").await.unwrap();

    assert!(
        client.access_key.is_some(),
        "Created client should have an access key"
    );
    assert!(!client.access_key.as_ref().unwrap().is_empty());

    // Verify persisted
    let found = mocks.clients.get(&client.id).await.unwrap();
    assert!(found.is_some());

    // Verify event
    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ClientRegistered {
            client_id,
            client_name,
            registration_type,
        } => {
            assert_eq!(client_id, client.id.to_string());
            assert_eq!(client_name, "Test Client");
            assert_eq!(registration_type, Some("api_key".to_string()));
        }
        other => panic!("Expected ClientRegistered, got {:?}", other),
    }
}

#[tokio::test]
async fn register_oauth_uses_provided_id() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.clients.clone());

    let expected_id = Uuid::new_v4();
    let client = svc
        .register_oauth_client(&expected_id.to_string(), "OAuth Client", "oauth")
        .await
        .unwrap();

    assert_eq!(
        client.id, expected_id,
        "OAuth client should use provided UUID"
    );
}

#[tokio::test]
async fn register_oauth_emits_with_oauth_type() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.clients.clone());

    let id = Uuid::new_v4();
    svc.register_oauth_client(&id.to_string(), "OAuth Client", "oauth")
        .await
        .unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ClientRegistered {
            registration_type, ..
        } => {
            assert_eq!(registration_type, Some("oauth".to_string()));
        }
        other => panic!("Expected ClientRegistered, got {:?}", other),
    }
}

#[tokio::test]
async fn register_oauth_invalid_uuid_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.clients.clone());

    let result = svc
        .register_oauth_client("not-a-uuid", "Bad Client", "oauth")
        .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid client ID"));
}

#[tokio::test]
async fn update_changes_fields_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.clients.clone());

    let client = svc.create("Original", "api_key").await.unwrap();
    let _ = rx.try_recv(); // drain create event

    let updated = svc
        .update(
            client.id,
            Some("Renamed".to_string()),
            Some("custom".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.client_type, "custom");

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ClientUpdated { client_id } => {
            assert_eq!(client_id, client.id.to_string());
        }
        other => panic!("Expected ClientUpdated, got {:?}", other),
    }
}

#[tokio::test]
async fn update_not_found_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(mocks.clients.clone());

    let result = svc
        .update(Uuid::new_v4(), Some("Name".to_string()), None)
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn delete_removes_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.clients.clone());

    let client = svc.create("To Delete", "api_key").await.unwrap();
    let _ = rx.try_recv(); // drain create

    svc.delete(client.id).await.unwrap();

    let found = mocks.clients.get(&client.id).await.unwrap();
    assert!(found.is_none(), "Client should be deleted");

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ClientDeleted { client_id } => {
            assert_eq!(client_id, client.id.to_string());
        }
        other => panic!("Expected ClientDeleted, got {:?}", other),
    }
}

#[tokio::test]
async fn record_token_issued_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(mocks.clients.clone());

    let client_id = Uuid::new_v4().to_string();
    svc.record_token_issued(&client_id);

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ClientTokenIssued { client_id: cid, .. } => {
            assert_eq!(cid, client_id);
        }
        other => panic!("Expected ClientTokenIssued, got {:?}", other),
    }
}
