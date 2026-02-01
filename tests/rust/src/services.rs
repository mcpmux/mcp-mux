//! Test service builders and helpers
//!
//! Provides factory functions to create service instances for testing.

use std::sync::Arc;

use mcpmux_core::DomainEvent;
use tokio::sync::broadcast;

use mcpmux_gateway::pool::{FeatureService, ServerManager};
use mcpmux_gateway::services::PrefixCacheService;

use crate::mocks::{
    MockCredentialRepository, MockFeatureSetRepository, MockOutboundOAuthRepository,
    MockServerFeatureRepository,
};

/// Test harness for ServerManager
///
/// Provides all the infrastructure needed to test ServerManager state machine
/// with injectable mock dependencies.
pub struct ServerManagerTestHarness {
    /// The ServerManager under test
    pub manager: Arc<ServerManager>,

    /// Event sender for triggering events
    pub event_tx: broadcast::Sender<DomainEvent>,

    /// Event receiver for asserting emitted events
    pub event_rx: broadcast::Receiver<DomainEvent>,

    /// Feature service for feature operations
    pub feature_service: Arc<FeatureService>,

    /// Prefix cache for prefix operations
    pub prefix_cache: Arc<PrefixCacheService>,

    /// Mock repositories
    pub feature_repo: Arc<MockServerFeatureRepository>,
    pub feature_set_repo: Arc<MockFeatureSetRepository>,
    pub credential_repo: Arc<MockCredentialRepository>,
    pub oauth_repo: Arc<MockOutboundOAuthRepository>,
}

impl ServerManagerTestHarness {
    /// Create a new test harness with default mock dependencies
    pub async fn new() -> Self {
        let (event_tx, event_rx) = broadcast::channel(100);

        // Create mock repositories
        let feature_repo = Arc::new(MockServerFeatureRepository::new());
        let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
        let credential_repo = Arc::new(MockCredentialRepository::new());
        let oauth_repo = Arc::new(MockOutboundOAuthRepository::new());

        // Create services with mock dependencies
        let prefix_cache = Arc::new(PrefixCacheService::new());
        let feature_service = Arc::new(FeatureService::new(
            feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache.clone(),
        ));

        // Create ConnectionService mock
        // Note: For ServerManager unit tests, we test the state machine logic
        // which primarily uses prefix_cache and emits events.
        // Full integration tests would use real ConnectionService.
        let connection_service = create_mock_connection_service(
            credential_repo.clone(),
            oauth_repo.clone(),
            prefix_cache.clone(),
        );

        let manager = Arc::new(ServerManager::new(
            event_tx.clone(),
            feature_service.clone(),
            connection_service,
            prefix_cache.clone(),
        ));

        Self {
            manager,
            event_tx,
            event_rx,
            feature_service,
            prefix_cache,
            feature_repo,
            feature_set_repo,
            credential_repo,
            oauth_repo,
        }
    }

    /// Subscribe to events (creates a new receiver)
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_tx.subscribe()
    }

    /// Collect all pending events from the receiver
    pub async fn collect_events(&mut self) -> Vec<DomainEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// Create a minimal ConnectionService for testing
///
/// This creates a real ConnectionService with mock repositories.
/// For unit tests that don't need actual connections, this provides
/// enough infrastructure for the state machine to work.
fn create_mock_connection_service(
    credential_repo: Arc<MockCredentialRepository>,
    oauth_repo: Arc<MockOutboundOAuthRepository>,
    prefix_cache: Arc<PrefixCacheService>,
) -> Arc<mcpmux_gateway::pool::ConnectionService> {
    use mcpmux_gateway::pool::{ConnectionService, OutboundOAuthManager, TokenService};

    // Create minimal token service
    let token_service = Arc::new(TokenService::new(
        credential_repo.clone(),
        oauth_repo.clone(),
    ));

    // Create minimal OAuth manager (won't actually do OAuth in unit tests)
    let oauth_manager = Arc::new(OutboundOAuthManager::new());

    Arc::new(ConnectionService::new(
        token_service,
        oauth_manager,
        credential_repo,
        oauth_repo,
        prefix_cache,
    ))
}

/// Create a standalone PrefixCacheService for testing
pub fn test_prefix_cache() -> Arc<PrefixCacheService> {
    Arc::new(PrefixCacheService::new())
}

/// Create a standalone FeatureService with mock repos
pub fn test_feature_service() -> (
    Arc<FeatureService>,
    Arc<MockServerFeatureRepository>,
    Arc<MockFeatureSetRepository>,
) {
    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    let service = Arc::new(FeatureService::new(
        feature_repo.clone(),
        feature_set_repo.clone(),
        prefix_cache,
    ));

    (service, feature_repo, feature_set_repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpmux_gateway::pool::ServerKey;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_harness_creation() {
        let harness = ServerManagerTestHarness::new().await;
        assert!(harness.manager.connected_count().await == 0);
    }

    #[tokio::test]
    async fn test_enable_server_emits_connecting_event() {
        let mut harness = ServerManagerTestHarness::new().await;

        let key = ServerKey {
            space_id: Uuid::new_v4(),
            server_id: "test-server".to_string(),
        };

        // Enable server
        let result = harness.manager.enable_server(key.clone()).await;
        assert!(result.is_ok());

        // Check event was emitted
        let events = harness.collect_events().await;
        assert!(!events.is_empty(), "Should emit at least one event");

        // First event should be Connecting
        let first_event = &events[0];
        if let DomainEvent::ServerStatusChanged {
            server_id, status, ..
        } = first_event
        {
            assert_eq!(server_id, "test-server");
            assert_eq!(*status, mcpmux_core::ConnectionStatus::Connecting);
        } else {
            panic!("Expected ServerStatusChanged event, got {:?}", first_event);
        }
    }

    #[tokio::test]
    async fn test_disable_server() {
        let mut harness = ServerManagerTestHarness::new().await;

        let key = ServerKey {
            space_id: Uuid::new_v4(),
            server_id: "test-server".to_string(),
        };

        // Enable then disable
        harness.manager.enable_server(key.clone()).await.unwrap();
        harness.collect_events().await; // Clear enable events

        harness.manager.disable_server(&key).await.unwrap();

        let events = harness.collect_events().await;
        assert!(!events.is_empty());

        // Should have Disconnected event
        let has_disconnected = events.iter().any(|e| {
            matches!(e, DomainEvent::ServerStatusChanged { status, .. }
                if *status == mcpmux_core::ConnectionStatus::Disconnected)
        });
        assert!(has_disconnected, "Should emit Disconnected event");
    }
}
