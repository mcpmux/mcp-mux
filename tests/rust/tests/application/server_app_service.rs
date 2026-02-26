//! Tests for ServerAppService
//!
//! Validates server installation, uninstallation, enable/disable,
//! config updates, and event emission.

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::application::ServerAppService;
use mcpmux_core::domain::{DomainEvent, InstallationSource, InstalledServer, ServerDefinition};
use mcpmux_core::event_bus::EventBus;
use mcpmux_core::repository::{FeatureSetRepository, InstalledServerRepository};
use tests::mocks::*;

/// Create a test ServerDefinition via JSON deserialization (fills defaults for all fields)
fn test_definition(id: &str, name: &str) -> ServerDefinition {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "description": "Test server",
        "transport": {
            "type": "stdio",
            "command": "echo"
        }
    }))
    .unwrap()
}

/// Create a ServerAppService wired up with mock repos and an event bus
fn make_service(
    server_repo: Arc<MockInstalledServerRepository>,
    fs_repo: Arc<MockFeatureSetRepository>,
    feature_repo: Arc<MockServerFeatureRepository>,
    cred_repo: Arc<MockCredentialRepository>,
) -> (
    ServerAppService,
    tokio::sync::broadcast::Receiver<DomainEvent>,
) {
    let bus = EventBus::new();
    let rx = bus.raw_sender().subscribe();
    let sender = bus.sender();
    let svc = ServerAppService::new(
        server_repo,
        Some(fs_repo),
        Some(feature_repo),
        Some(cred_repo),
        sender,
    );
    (svc, rx)
}

#[tokio::test]
async fn install_persists_server_disabled() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    let server = svc
        .install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();

    assert!(
        !server.enabled,
        "Installed server should be disabled by default"
    );
    assert_eq!(server.server_id, "test-server");

    // Verify persisted in repo
    let found = mocks
        .installed_servers
        .get_by_server_id(&space_id.to_string(), "test-server")
        .await
        .unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn install_emits_server_installed() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ServerInstalled {
            space_id: sid,
            server_id,
            server_name,
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(server_id, "test-server");
            assert_eq!(server_name, "Test Server");
        }
        other => panic!("Expected ServerInstalled, got {:?}", other),
    }
}

#[tokio::test]
async fn install_creates_server_all_fs() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();

    let server_all = mocks
        .feature_sets
        .get_server_all(&space_id.to_string(), "test-server")
        .await
        .unwrap();
    assert!(
        server_all.is_some(),
        "ServerAll feature set should be created on install"
    );
}

#[tokio::test]
async fn install_duplicate_returns_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();

    let result = svc
        .install(space_id, "test-server", &def, HashMap::new())
        .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("already installed"));
}

#[tokio::test]
async fn uninstall_removes_and_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain install event

    svc.uninstall(space_id, "test-server").await.unwrap();

    // Verify removed from repo
    let found = mocks
        .installed_servers
        .get_by_server_id(&space_id.to_string(), "test-server")
        .await
        .unwrap();
    assert!(found.is_none(), "Server should be removed after uninstall");

    // Verify event
    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ServerUninstalled {
            space_id: sid,
            server_id,
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(server_id, "test-server");
        }
        other => panic!("Expected ServerUninstalled, got {:?}", other),
    }
}

#[tokio::test]
async fn uninstall_deletes_fs_features_creds() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let space_id_str = space_id.to_string();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();

    // Pre-populate features and credentials
    use mcpmux_core::repository::{CredentialRepository, ServerFeatureRepository};
    let feature = mcpmux_core::ServerFeature::tool(&space_id_str, "test-server", "my_tool");
    mocks.features.upsert(&feature).await.unwrap();
    let cred = mcpmux_core::domain::Credential::api_key(space_id, "test-server", "secret-key");
    mocks.credentials.save(&cred).await.unwrap();

    svc.uninstall(space_id, "test-server").await.unwrap();

    // Verify ServerAll FS deleted
    let server_all = mocks
        .feature_sets
        .get_server_all(&space_id_str, "test-server")
        .await
        .unwrap();
    assert!(server_all.is_none(), "ServerAll FS should be deleted");

    // Verify features deleted
    let features = mocks
        .features
        .list_for_server(&space_id_str, "test-server")
        .await
        .unwrap();
    assert!(features.is_empty(), "Features should be deleted");

    // Verify credentials deleted
    let creds = mocks
        .credentials
        .get_all(&space_id, "test-server")
        .await
        .unwrap();
    assert!(creds.is_empty(), "Credentials should be deleted");
}

#[tokio::test]
async fn uninstall_not_found_returns_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();

    let result = svc.uninstall(space_id, "nonexistent").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not installed"));
}

#[tokio::test]
async fn uninstall_userconfig_modifies_json() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();

    // Create a temp config file
    let temp_dir = tempfile::TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.json");
    let config_content = serde_json::json!({
        "mcpServers": {
            "test-server": {
                "command": "echo",
                "args": ["hello"]
            },
            "other-server": {
                "command": "echo"
            }
        }
    });
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&config_content).unwrap(),
    )
    .unwrap();

    // Manually insert a server with UserConfig source
    let server = InstalledServer::new(&space_id.to_string(), "test-server")
        .with_source(InstallationSource::UserConfig {
            file_path: config_path.clone(),
        })
        .with_enabled(false);
    mocks.installed_servers.install(&server).await.unwrap();

    svc.uninstall(space_id, "test-server").await.unwrap();

    // Verify the JSON file was modified
    let content = std::fs::read_to_string(&config_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let servers = parsed["mcpServers"].as_object().unwrap();
    assert!(
        !servers.contains_key("test-server"),
        "test-server should be removed from config"
    );
    assert!(
        servers.contains_key("other-server"),
        "other-server should remain"
    );
}

#[tokio::test]
async fn enable_sets_true_emits_event() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain install event

    svc.enable(space_id, "test-server").await.unwrap();

    // Verify enabled in repo
    let server = mocks
        .installed_servers
        .get_by_server_id(&space_id.to_string(), "test-server")
        .await
        .unwrap()
        .unwrap();
    assert!(server.enabled);

    // Verify event
    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ServerEnabled {
            space_id: sid,
            server_id,
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(server_id, "test-server");
        }
        other => panic!("Expected ServerEnabled, got {:?}", other),
    }
}

#[tokio::test]
async fn disable_sets_false_emits_event() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();
    svc.enable(space_id, "test-server").await.unwrap();
    let _ = rx.try_recv(); // drain install
    let _ = rx.try_recv(); // drain enable

    svc.disable(space_id, "test-server").await.unwrap();

    let server = mocks
        .installed_servers
        .get_by_server_id(&space_id.to_string(), "test-server")
        .await
        .unwrap()
        .unwrap();
    assert!(!server.enabled);

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ServerDisabled {
            space_id: sid,
            server_id,
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(server_id, "test-server");
        }
        other => panic!("Expected ServerDisabled, got {:?}", other),
    }
}

#[tokio::test]
async fn enable_not_installed_returns_err() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();

    let result = svc.enable(space_id, "nonexistent").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not installed"));
}

#[tokio::test]
async fn update_config_updates_fields_emits() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain install

    let mut inputs = HashMap::new();
    inputs.insert("key".to_string(), "value".to_string());
    let mut env = HashMap::new();
    env.insert("MY_VAR".to_string(), "my_val".to_string());
    let args = vec!["--flag".to_string()];
    let mut headers = HashMap::new();
    headers.insert("X-Custom".to_string(), "val".to_string());

    let updated = svc
        .update_config(
            space_id,
            "test-server",
            inputs.clone(),
            Some(env.clone()),
            Some(args.clone()),
            Some(headers.clone()),
        )
        .await
        .unwrap();

    assert_eq!(updated.input_values, inputs);
    assert_eq!(updated.env_overrides, env);
    assert_eq!(updated.args_append, args);
    assert_eq!(updated.extra_headers, headers);

    let event = rx.try_recv().unwrap();
    match event {
        DomainEvent::ServerConfigUpdated {
            space_id: sid,
            server_id,
        } => {
            assert_eq!(sid, space_id);
            assert_eq!(server_id, "test-server");
        }
        other => panic!("Expected ServerConfigUpdated, got {:?}", other),
    }
}

#[tokio::test]
async fn update_config_partial_fields() {
    let mocks = MockRepositories::new();
    let (svc, _rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();

    let mut env = HashMap::new();
    env.insert("MY_VAR".to_string(), "my_val".to_string());

    // Only set env_overrides, args/headers left as None
    let updated = svc
        .update_config(
            space_id,
            "test-server",
            HashMap::new(),
            Some(env.clone()),
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(updated.env_overrides, env);
    assert!(updated.args_append.is_empty(), "args should remain empty");
    assert!(
        updated.extra_headers.is_empty(),
        "headers should remain empty"
    );
}

#[tokio::test]
async fn set_oauth_connected_no_event() {
    let mocks = MockRepositories::new();
    let (svc, mut rx) = make_service(
        mocks.installed_servers.clone(),
        mocks.feature_sets.clone(),
        mocks.features.clone(),
        mocks.credentials.clone(),
    );
    let space_id = Uuid::new_v4();
    let def = test_definition("test-server", "Test Server");

    svc.install(space_id, "test-server", &def, HashMap::new())
        .await
        .unwrap();
    let _ = rx.try_recv(); // drain install

    svc.set_oauth_connected(space_id, "test-server", true)
        .await
        .unwrap();

    let server = mocks
        .installed_servers
        .get_by_server_id(&space_id.to_string(), "test-server")
        .await
        .unwrap()
        .unwrap();
    assert!(server.oauth_connected);

    // No event should be emitted
    assert!(
        rx.try_recv().is_err(),
        "No event expected for set_oauth_connected"
    );
}
