//! ServerManager state machine tests
//!
//! Tests for the core state machine logic including:
//! - State transitions (Disconnected -> Connecting -> Connected)
//! - Event emission
//! - Lock management
//! - OAuth flow states
//! - Error handling

use mcpmux_core::{ConnectionStatus, DomainEvent};
use mcpmux_gateway::pool::ServerKey;
use std::time::Duration;
use tests::ServerManagerTestHarness;
use uuid::Uuid;

// ============================================================================
// Basic State Transitions
// ============================================================================

#[tokio::test]
async fn test_enable_server_starts_connecting() {
    let mut harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    let result = harness.manager.enable_server(key.clone()).await;
    assert!(result.is_ok());

    let events = harness.collect_events().await;
    assert_event_status(&events, "server-1", ConnectionStatus::Connecting);
}

#[tokio::test]
async fn test_enable_server_idempotent_when_connecting() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    // First enable
    harness.manager.enable_server(key.clone()).await.unwrap();

    // Second enable should succeed (idempotent)
    let result = harness.manager.enable_server(key).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_disable_server_transitions_to_disconnected() {
    let mut harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();
    harness.collect_events().await; // Clear enable events

    harness.manager.disable_server(&key).await.unwrap();

    let events = harness.collect_events().await;
    assert_event_status(&events, "server-1", ConnectionStatus::Disconnected);
}

#[tokio::test]
async fn test_disable_nonexistent_server_succeeds() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("nonexistent");

    // Disabling a server that was never enabled should succeed
    let result = harness.manager.disable_server(&key).await;
    assert!(result.is_ok());
}

// ============================================================================
// Status Query
// ============================================================================

#[tokio::test]
async fn test_get_status_returns_none_for_unknown() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("unknown");

    let status = harness.manager.get_status(&key).await;
    assert!(status.is_none());
}

#[tokio::test]
async fn test_get_status_returns_connecting_after_enable() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();

    let status = harness.manager.get_status(&key).await;
    assert!(status.is_some());

    let (conn_status, _flow_id, _has_connected, _error) = status.unwrap();
    // Status is Connecting because we haven't completed the connection
    assert!(matches!(
        conn_status,
        mcpmux_gateway::pool::ConnectionStatus::Connecting
    ));
}

#[tokio::test]
async fn test_get_all_statuses_for_space() {
    let harness = ServerManagerTestHarness::new().await;
    let space_id = Uuid::new_v4();

    let key1 = ServerKey {
        space_id,
        server_id: "server-1".to_string(),
    };
    let key2 = ServerKey {
        space_id,
        server_id: "server-2".to_string(),
    };

    harness.manager.enable_server(key1).await.unwrap();
    harness.manager.enable_server(key2).await.unwrap();

    let all_statuses = harness.manager.get_all_statuses(space_id).await;

    assert_eq!(all_statuses.len(), 2);
    assert!(all_statuses.contains_key("server-1"));
    assert!(all_statuses.contains_key("server-2"));
}

#[tokio::test]
async fn test_connected_count_initially_zero() {
    let harness = ServerManagerTestHarness::new().await;
    assert_eq!(harness.manager.connected_count().await, 0);
}

// ============================================================================
// Flow ID Management
// ============================================================================

#[tokio::test]
async fn test_enable_increments_flow_id() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();

    let status = harness.manager.get_status(&key).await.unwrap();
    let (_conn_status, flow_id, _, _) = status;

    assert!(flow_id >= 1, "Flow ID should be incremented");
}

#[tokio::test]
async fn test_disable_increments_flow_id() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();

    let status_before = harness.manager.get_status(&key).await.unwrap();
    let flow_before = status_before.1;

    harness.manager.disable_server(&key).await.unwrap();

    let status_after = harness.manager.get_status(&key).await.unwrap();
    let flow_after = status_after.1;

    assert!(
        flow_after > flow_before,
        "Flow ID should be incremented on disable"
    );
}

// ============================================================================
// Event Emission
// ============================================================================

#[tokio::test]
async fn test_enable_emits_server_status_changed() {
    let mut harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();

    let events = harness.collect_events().await;

    let status_changed = events.iter().find(|e| {
        matches!(e, DomainEvent::ServerStatusChanged { server_id, .. } if server_id == "server-1")
    });

    assert!(
        status_changed.is_some(),
        "Should emit ServerStatusChanged event"
    );
}

#[tokio::test]
async fn test_disable_emits_disconnected_event() {
    let mut harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();
    harness.collect_events().await;

    harness.manager.disable_server(&key).await.unwrap();

    let events = harness.collect_events().await;
    assert_event_status(&events, "server-1", ConnectionStatus::Disconnected);
}

#[tokio::test]
async fn test_event_contains_correct_space_id() {
    let mut harness = ServerManagerTestHarness::new().await;
    let space_id = Uuid::new_v4();
    let key = ServerKey {
        space_id,
        server_id: "server-1".to_string(),
    };

    harness.manager.enable_server(key).await.unwrap();

    let events = harness.collect_events().await;

    let event = events.iter().find_map(|e| {
        if let DomainEvent::ServerStatusChanged { space_id: sid, .. } = e {
            Some(*sid)
        } else {
            None
        }
    });

    assert_eq!(event, Some(space_id));
}

#[tokio::test]
async fn test_event_contains_flow_id() {
    let mut harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key).await.unwrap();

    let events = harness.collect_events().await;

    let has_flow_id = events.iter().any(|e| {
        if let DomainEvent::ServerStatusChanged { flow_id, .. } = e {
            *flow_id >= 1
        } else {
            false
        }
    });

    assert!(has_flow_id, "Event should contain non-zero flow_id");
}

// ============================================================================
// Prefix Cache Integration
// ============================================================================

#[tokio::test]
async fn test_enable_assigns_prefix() {
    let harness = ServerManagerTestHarness::new().await;
    let space_id = Uuid::new_v4();
    let key = ServerKey {
        space_id,
        server_id: "test-server".to_string(),
    };

    harness.manager.enable_server(key).await.unwrap();

    // Check prefix was assigned
    let prefix = harness
        .prefix_cache
        .get_prefix_for_server(&space_id.to_string(), "test-server")
        .await;

    // Without alias, prefix defaults to normalized server_id
    assert_eq!(prefix, "test-server");
}

#[tokio::test]
async fn test_disable_releases_prefix() {
    let harness = ServerManagerTestHarness::new().await;
    let space_id = Uuid::new_v4();
    let key = ServerKey {
        space_id,
        server_id: "test-server".to_string(),
    };

    harness.manager.enable_server(key.clone()).await.unwrap();
    harness.manager.disable_server(&key).await.unwrap();

    // Prefix should be released (available again)
    let available = harness
        .prefix_cache
        .is_prefix_available(&space_id.to_string(), "test-server")
        .await;

    assert!(available, "Prefix should be available after disable");
}

// ============================================================================
// Multiple Servers
// ============================================================================

#[tokio::test]
async fn test_multiple_servers_independent() {
    let harness = ServerManagerTestHarness::new().await;
    let key1 = test_key("server-1");
    let key2 = test_key("server-2");

    harness.manager.enable_server(key1.clone()).await.unwrap();
    harness.manager.enable_server(key2.clone()).await.unwrap();

    // Both should be in Connecting state
    let status1 = harness.manager.get_status(&key1).await.unwrap();
    let status2 = harness.manager.get_status(&key2).await.unwrap();

    assert!(matches!(
        status1.0,
        mcpmux_gateway::pool::ConnectionStatus::Connecting
    ));
    assert!(matches!(
        status2.0,
        mcpmux_gateway::pool::ConnectionStatus::Connecting
    ));
}

#[tokio::test]
async fn test_disable_one_doesnt_affect_other() {
    let harness = ServerManagerTestHarness::new().await;
    let key1 = test_key("server-1");
    let key2 = test_key("server-2");

    harness.manager.enable_server(key1.clone()).await.unwrap();
    harness.manager.enable_server(key2.clone()).await.unwrap();

    // Disable only server-1
    harness.manager.disable_server(&key1).await.unwrap();

    // server-2 should still be connecting
    let status2 = harness.manager.get_status(&key2).await.unwrap();
    assert!(matches!(
        status2.0,
        mcpmux_gateway::pool::ConnectionStatus::Connecting
    ));
}

// ============================================================================
// Subscribe
// ============================================================================

#[tokio::test]
async fn test_subscribe_receives_events() {
    let harness = ServerManagerTestHarness::new().await;
    let mut rx = harness.manager.subscribe();
    let key = test_key("server-1");

    harness.manager.enable_server(key).await.unwrap();

    // Should receive the event
    let event = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(event.is_ok(), "Should receive event");

    let event = event.unwrap().unwrap();
    assert!(matches!(event, DomainEvent::ServerStatusChanged { .. }));
}

#[tokio::test]
async fn test_multiple_subscribers() {
    let harness = ServerManagerTestHarness::new().await;
    let mut rx1 = harness.manager.subscribe();
    let mut rx2 = harness.manager.subscribe();
    let key = test_key("server-1");

    harness.manager.enable_server(key).await.unwrap();

    // Both subscribers should receive the event
    let event1 = tokio::time::timeout(Duration::from_millis(100), rx1.recv()).await;
    let event2 = tokio::time::timeout(Duration::from_millis(100), rx2.recv()).await;

    assert!(event1.is_ok());
    assert!(event2.is_ok());
}

// ============================================================================
// OAuth State Transitions
// ============================================================================

#[tokio::test]
async fn test_start_auth_requires_auth_required_state() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    // Server not enabled - start_auth should fail
    let result = harness.manager.start_auth(&key).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_cancel_auth_requires_authenticating_state() {
    let harness = ServerManagerTestHarness::new().await;
    let key = test_key("server-1");

    harness.manager.enable_server(key.clone()).await.unwrap();

    // In Connecting state, cancel_auth should fail
    let result = harness.manager.cancel_auth(&key).await;
    assert!(result.is_err());
}

// ============================================================================
// Helper Functions
// ============================================================================

fn test_key(server_id: &str) -> ServerKey {
    ServerKey {
        space_id: Uuid::new_v4(),
        server_id: server_id.to_string(),
    }
}

fn assert_event_status(
    events: &[DomainEvent],
    expected_server_id: &str,
    expected_status: ConnectionStatus,
) {
    let found = events.iter().any(|e| {
        if let DomainEvent::ServerStatusChanged {
            server_id, status, ..
        } = e
        {
            server_id == expected_server_id && *status == expected_status
        } else {
            false
        }
    });

    assert!(
        found,
        "Expected event with server_id={} and status={:?}",
        expected_server_id, expected_status
    );
}
