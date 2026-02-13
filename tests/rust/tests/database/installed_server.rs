//! InstalledServerRepository integration tests

use mcpmux_core::repository::{InstalledServerRepository, SpaceRepository};
use mcpmux_storage::{
    generate_master_key, FieldEncryptor, SqliteInstalledServerRepository, SqliteSpaceRepository,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::sync::Arc;
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;

fn test_encryptor() -> Arc<FieldEncryptor> {
    let key = generate_master_key().expect("Failed to generate key");
    Arc::new(FieldEncryptor::new(&key).expect("Failed to create encryptor"))
}

#[tokio::test]
async fn test_installed_server_install_and_get() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
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

#[tokio::test]
async fn test_installed_server_env_overrides_persist() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut server = fixtures::test_installed_server(&space.id.to_string(), "env-server");
    server
        .env_overrides
        .insert("NODE_ENV".to_string(), "production".to_string());
    server
        .env_overrides
        .insert("DEBUG".to_string(), "true".to_string());

    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .expect("Failed to install server");

    // Retrieve and verify env_overrides persisted
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.env_overrides.len(), 2);
    assert_eq!(
        loaded.env_overrides.get("NODE_ENV"),
        Some(&"production".to_string())
    );
    assert_eq!(loaded.env_overrides.get("DEBUG"), Some(&"true".to_string()));
}

#[tokio::test]
async fn test_installed_server_args_append_persist() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut server = fixtures::test_installed_server(&space.id.to_string(), "args-server");
    server.args_append = vec![
        "--verbose".to_string(),
        "--port".to_string(),
        "8080".to_string(),
    ];

    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .expect("Failed to install server");

    // Retrieve and verify args_append persisted
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.args_append.len(), 3);
    assert_eq!(loaded.args_append[0], "--verbose");
    assert_eq!(loaded.args_append[1], "--port");
    assert_eq!(loaded.args_append[2], "8080");
}

#[tokio::test]
async fn test_installed_server_extra_headers_persist() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut server = fixtures::test_installed_server(&space.id.to_string(), "headers-server");
    server
        .extra_headers
        .insert("Authorization".to_string(), "Bearer token123".to_string());
    server
        .extra_headers
        .insert("X-Custom".to_string(), "value".to_string());

    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .expect("Failed to install server");

    // Retrieve and verify extra_headers persisted
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.extra_headers.len(), 2);
    assert_eq!(
        loaded.extra_headers.get("Authorization"),
        Some(&"Bearer token123".to_string())
    );
    assert_eq!(
        loaded.extra_headers.get("X-Custom"),
        Some(&"value".to_string())
    );
}

#[tokio::test]
async fn test_installed_server_update_preserves_custom_fields() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut server = fixtures::test_installed_server(&space.id.to_string(), "update-server");
    server
        .env_overrides
        .insert("KEY".to_string(), "value".to_string());
    server.args_append = vec!["--flag".to_string()];
    server
        .extra_headers
        .insert("X-Test".to_string(), "test".to_string());

    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Update the server via the update method
    let mut loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();

    // Modify custom fields
    loaded
        .env_overrides
        .insert("KEY2".to_string(), "value2".to_string());
    loaded.args_append.push("--extra".to_string());
    loaded
        .extra_headers
        .insert("X-New".to_string(), "new".to_string());

    InstalledServerRepository::update(&server_repo, &loaded)
        .await
        .expect("Failed to update server");

    // Verify updated values persisted
    let reloaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reloaded.env_overrides.len(), 2);
    assert_eq!(
        reloaded.env_overrides.get("KEY"),
        Some(&"value".to_string())
    );
    assert_eq!(
        reloaded.env_overrides.get("KEY2"),
        Some(&"value2".to_string())
    );
    assert_eq!(reloaded.args_append, vec!["--flag", "--extra"]);
    assert_eq!(reloaded.extra_headers.len(), 2);
    assert_eq!(
        reloaded.extra_headers.get("X-Test"),
        Some(&"test".to_string())
    );
    assert_eq!(
        reloaded.extra_headers.get("X-New"),
        Some(&"new".to_string())
    );
}

#[tokio::test]
async fn test_installed_server_empty_custom_fields_by_default() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Install a server without setting any custom fields
    let server = fixtures::test_installed_server(&space.id.to_string(), "default-server");
    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Verify custom fields are empty by default
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(loaded.env_overrides.is_empty());
    assert!(loaded.args_append.is_empty());
    assert!(loaded.extra_headers.is_empty());
}

#[tokio::test]
async fn test_installed_server_clear_custom_fields_via_update() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Install server WITH custom fields
    let mut server = fixtures::test_installed_server(&space.id.to_string(), "clearable-server");
    server
        .env_overrides
        .insert("KEY".to_string(), "value".to_string());
    server.args_append = vec!["--flag".to_string()];
    server
        .extra_headers
        .insert("X-Test".to_string(), "test".to_string());

    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    // Verify they're set
    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.env_overrides.len(), 1);
    assert_eq!(loaded.args_append.len(), 1);
    assert_eq!(loaded.extra_headers.len(), 1);

    // Clear all fields by updating with empty collections
    let mut to_update = loaded;
    to_update.env_overrides = HashMap::new();
    to_update.args_append = Vec::new();
    to_update.extra_headers = HashMap::new();

    InstalledServerRepository::update(&server_repo, &to_update)
        .await
        .expect("Failed to update server");

    // Verify they're cleared
    let cleared = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        cleared.env_overrides.is_empty(),
        "env_overrides should be empty after clearing"
    );
    assert!(
        cleared.args_append.is_empty(),
        "args_append should be empty after clearing"
    );
    assert!(
        cleared.extra_headers.is_empty(),
        "extra_headers should be empty after clearing"
    );
}

#[tokio::test]
async fn test_installed_server_special_characters_persist() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut server = fixtures::test_installed_server(&space.id.to_string(), "special-server");
    // Values with special characters
    server.env_overrides.insert(
        "PATH_WITH=EQUALS".to_string(),
        "value with \"quotes\" and 'apostrophes'".to_string(),
    );
    server.args_append = vec![
        "--config=/path/to/file with spaces".to_string(),
        "unicode: 日本語".to_string(),
    ];
    server
        .extra_headers
        .insert("Authorization".to_string(), "Bearer tok3n+/=".to_string());

    let server_id = server.id;
    InstalledServerRepository::install(&server_repo, &server)
        .await
        .unwrap();

    let loaded = InstalledServerRepository::get(&server_repo, &server_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        loaded.env_overrides.get("PATH_WITH=EQUALS"),
        Some(&"value with \"quotes\" and 'apostrophes'".to_string())
    );
    assert_eq!(loaded.args_append[0], "--config=/path/to/file with spaces");
    assert_eq!(loaded.args_append[1], "unicode: 日本語");
    assert_eq!(
        loaded.extra_headers.get("Authorization"),
        Some(&"Bearer tok3n+/=".to_string())
    );
}
