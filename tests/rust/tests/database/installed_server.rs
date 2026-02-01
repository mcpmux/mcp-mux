//! InstalledServerRepository integration tests

use mcpmux_core::repository::{InstalledServerRepository, SpaceRepository};
use mcpmux_storage::{SqliteInstalledServerRepository, SqliteSpaceRepository};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::sync::Arc;
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;

#[tokio::test]
async fn test_installed_server_install_and_get() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // First create a space (needed for foreign key)
    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Install server
    let server = fixtures::test_installed_server(&space.id.to_string(), "test-server-1");
    let server_id = server.id;

    InstalledServerRepository::install(&server_repo, &server)
        .await
        .expect("Failed to install server");

    // Get by ID
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .expect("Failed to get server");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.server_id, "test-server-1");
    assert!(loaded.enabled);
}

#[tokio::test]
async fn test_installed_server_get_by_server_id() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let server = fixtures::test_installed_server(&space.id.to_string(), "my-mcp-server");
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Get by space_id + server_id
    let loaded = InstalledServerRepository::get_by_server_id(
        &server_repo,
        &space.id.to_string(),
        "my-mcp-server",
    )
    .await
    .expect("Failed to get server by server_id");
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().server_id, "my-mcp-server");

    // Try non-existent
    let not_found = InstalledServerRepository::get_by_server_id(
        &server_repo,
        &space.id.to_string(),
        "nonexistent",
    )
    .await
    .expect("Failed to query");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_installed_server_list_for_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // Create two spaces
    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    SpaceRepository::create(&space_repo, &space1).await.unwrap();
    SpaceRepository::create(&space_repo, &space2).await.unwrap();

    // Install servers in each space
    let server1a = fixtures::test_installed_server(&space1.id.to_string(), "server-a");
    let server1b = fixtures::test_installed_server(&space1.id.to_string(), "server-b");
    let server2a = fixtures::test_installed_server(&space2.id.to_string(), "server-a");

    InstalledServerRepository::install(&server_repo, &server1a)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server1b)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server2a)
        .await
        .unwrap();

    // List for space1
    let space1_servers =
        InstalledServerRepository::list_for_space(&server_repo, &space1.id.to_string())
            .await
            .expect("Failed to list servers");
    assert_eq!(space1_servers.len(), 2);

    // List for space2
    let space2_servers =
        InstalledServerRepository::list_for_space(&server_repo, &space2.id.to_string())
            .await
            .expect("Failed to list servers");
    assert_eq!(space2_servers.len(), 1);
}

#[tokio::test]
async fn test_installed_server_uninstall() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let server = fixtures::test_installed_server(&space.id.to_string(), "to-uninstall");
    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Uninstall
    InstalledServerRepository::uninstall(&server_repo, &server_id)
        .await
        .expect("Failed to uninstall server");

    // Verify deleted
    let deleted = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .expect("Failed to check deleted");
    assert!(deleted.is_none());
}

#[tokio::test]
async fn test_installed_server_set_enabled() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let server = fixtures::test_installed_server(&space.id.to_string(), "toggle-server");
    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Initially enabled
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.enabled);

    // Disable
    InstalledServerRepository::set_enabled(&server_repo, &server_id, false)
        .await
        .expect("Failed to disable");
    let disabled = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!disabled.enabled);

    // Re-enable
    InstalledServerRepository::set_enabled(&server_repo, &server_id, true)
        .await
        .expect("Failed to re-enable");
    let enabled = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(enabled.enabled);
}

#[tokio::test]
async fn test_installed_server_list_enabled() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Install 3 servers, 2 enabled, 1 disabled
    let server1 = fixtures::test_installed_server(&space.id.to_string(), "enabled-1");
    let server2 = fixtures::test_installed_server(&space.id.to_string(), "enabled-2");
    let mut server3 = fixtures::test_installed_server(&space.id.to_string(), "disabled-1");
    server3.enabled = false;

    InstalledServerRepository::install(&server_repo, &server1)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server2)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server3)
        .await
        .unwrap();

    // List enabled only
    let enabled = InstalledServerRepository::list_enabled(&server_repo, &space.id.to_string())
        .await
        .expect("Failed to list enabled");
    assert_eq!(enabled.len(), 2);
    assert!(enabled.iter().all(|s| s.enabled));
}

#[tokio::test]
async fn test_installed_server_set_oauth_connected() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let server = fixtures::test_installed_server(&space.id.to_string(), "oauth-server");
    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Initially not connected
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!loaded.oauth_connected);

    // Mark as OAuth connected
    InstalledServerRepository::set_oauth_connected(&server_repo, &server_id, true)
        .await
        .expect("Failed to set oauth_connected");
    let connected = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(connected.oauth_connected);
}

#[tokio::test]
async fn test_installed_server_update_inputs() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let server = fixtures::test_installed_server(&space.id.to_string(), "input-server");
    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Update inputs
    let mut inputs = HashMap::new();
    inputs.insert("api_key".to_string(), "secret123".to_string());
    inputs.insert("region".to_string(), "us-west-2".to_string());

    InstalledServerRepository::update_inputs(&server_repo, &server_id, inputs.clone())
        .await
        .expect("Failed to update inputs");

    // Verify
    let updated = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.input_values.get("api_key"),
        Some(&"secret123".to_string())
    );
    assert_eq!(
        updated.input_values.get("region"),
        Some(&"us-west-2".to_string())
    );
}

#[tokio::test]
async fn test_installed_server_update_cached_definition() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let server = fixtures::test_installed_server(&space.id.to_string(), "cache-server");
    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Update cached definition
    let cached_def = r#"{"name": "Test Server", "version": "1.0"}"#;
    InstalledServerRepository::update_cached_definition(
        &server_repo,
        &server_id,
        Some("Test Server".to_string()),
        Some(cached_def.to_string()),
    )
    .await
    .expect("Failed to update cached definition");

    // Verify
    let updated = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.server_name, Some("Test Server".to_string()));
    assert_eq!(updated.cached_definition, Some(cached_def.to_string()));
}

#[tokio::test]
async fn test_installed_server_list_all() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // Create two spaces with servers
    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    SpaceRepository::create(&space_repo, &space1).await.unwrap();
    SpaceRepository::create(&space_repo, &space2).await.unwrap();

    let server1 = fixtures::test_installed_server(&space1.id.to_string(), "server-1");
    let server2 = fixtures::test_installed_server(&space2.id.to_string(), "server-2");
    InstalledServerRepository::install(&server_repo, &server1)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server2)
        .await
        .unwrap();

    // List all
    let all = InstalledServerRepository::list(&server_repo)
        .await
        .expect("Failed to list all");
    assert!(all.len() >= 2);
}

#[tokio::test]
async fn test_installed_server_list_enabled_all() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    SpaceRepository::create(&space_repo, &space1).await.unwrap();
    SpaceRepository::create(&space_repo, &space2).await.unwrap();

    // Enabled servers across spaces
    let server1 = fixtures::test_installed_server(&space1.id.to_string(), "enabled-1");
    let server2 = fixtures::test_installed_server(&space2.id.to_string(), "enabled-2");
    let mut server3 = fixtures::test_installed_server(&space1.id.to_string(), "disabled");
    server3.enabled = false;

    InstalledServerRepository::install(&server_repo, &server1)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server2)
        .await
        .unwrap();
    InstalledServerRepository::install(&server_repo, &server3)
        .await
        .unwrap();

    // List all enabled across all spaces
    let enabled_all = InstalledServerRepository::list_enabled_all(&server_repo)
        .await
        .expect("Failed to list enabled all");
    assert_eq!(enabled_all.len(), 2);
    assert!(enabled_all.iter().all(|s| s.enabled));
}
