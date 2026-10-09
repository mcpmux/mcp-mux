//! Startup only runs what the user installed.

use std::sync::Arc;

use mcpmux_core::{DomainEvent, InstalledServer, ServerDiscoveryService, ServerLogManager};
use mcpmux_gateway::server::{DependenciesBuilder, GatewayState, ServiceContainer};
use tests::db::TestDatabase;
use tests::mocks::*;
use tokio::sync::broadcast;

/// A server installed before definitions were stored is not started from
/// whatever the registry serves today: auto-connect reports it failed, and
/// nothing is spawned.
#[tokio::test(flavor = "multi_thread")]
async fn a_server_without_a_stored_definition_is_not_started() {
    let mut legacy = InstalledServer::new("00000000-0000-0000-0000-000000000001", "legacy");
    legacy.cached_definition = None;
    legacy.enabled = true;

    let database = Arc::new(tokio::sync::Mutex::new(TestDatabase::in_memory().db));
    let deps = DependenciesBuilder::new()
        .with_installed_server_repo(Arc::new(
            MockInstalledServerRepository::new().with_server(legacy),
        ))
        .with_credential_repo(Arc::new(MockCredentialRepository::new()))
        .with_backend_oauth_repo(Arc::new(MockOutboundOAuthRepository::new()))
        .with_feature_repo(Arc::new(MockServerFeatureRepository::new())
            as Arc<dyn mcpmux_core::ServerFeatureRepository>)
        .with_feature_set_repo(
            Arc::new(MockFeatureSetRepository::new()) as Arc<dyn mcpmux_core::FeatureSetRepository>
        )
        .with_server_discovery(Arc::new(ServerDiscoveryService::new(
            std::path::PathBuf::from("test-data"),
            std::path::PathBuf::from("test-spaces"),
        )))
        .with_log_manager(Arc::new(ServerLogManager::new(
            mcpmux_core::LogConfig::default(),
        )))
        .with_database(database)
        .build()
        .expect("build dependencies");
    let (event_tx, _) = broadcast::channel::<DomainEvent>(16);
    let gateway_state = Arc::new(tokio::sync::RwLock::new(GatewayState::new(
        event_tx.clone(),
    )));
    let services = ServiceContainer::initialize(&deps, event_tx, gateway_state);

    let result = services
        .startup_orchestrator
        .auto_connect_enabled_servers()
        .await
        .unwrap();
    assert!(result.connected.is_empty(), "{:?}", result.connected);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].0, "legacy");
    assert!(
        result.failed[0].1.contains("no stored definition"),
        "{}",
        result.failed[0].1
    );
}
