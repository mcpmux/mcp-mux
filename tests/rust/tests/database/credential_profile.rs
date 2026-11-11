//! CredentialProfile integration tests
//!
//! Tests for central credential profiles:
//! - CRUD operations with encrypted storage
//! - Ciphertext row-binding (tamper resistance)
//! - Linkage with InstalledServer
//! - Preservation across server uninstalls
//! - ON DELETE SET NULL on profile deletion
//! - Single-point rotation propagation across multiple Spaces

use std::collections::HashMap;
use std::sync::Arc;
use tests::fixtures;
use tokio::sync::Mutex;

use mcpmux_core::repository::{
    CredentialProfileRepository, InstalledServerRepository, SpaceRepository,
};
use mcpmux_core::service::CredentialProfileService;
use mcpmux_core::{CredentialProfile, InstalledServer};
use mcpmux_storage::{
    generate_master_key, Database, FieldEncryptor, SqliteCredentialProfileRepository,
    SqliteInstalledServerRepository, SqliteSpaceRepository,
};

struct TestContext {
    db: Arc<Mutex<Database>>,
    _enc: Arc<FieldEncryptor>,
    profile_repo: Arc<SqliteCredentialProfileRepository>,
    server_repo: Arc<SqliteInstalledServerRepository>,
    space_repo: Arc<SqliteSpaceRepository>,
    _dir: tempfile::TempDir,
}

async fn setup() -> TestContext {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("mcpmux.db");
    let db = Arc::new(Mutex::new(Database::open(&db_path).unwrap()));
    let master_key = generate_master_key().unwrap();
    let enc = Arc::new(FieldEncryptor::new(&master_key).unwrap());

    let profile_repo = Arc::new(SqliteCredentialProfileRepository::new(
        Arc::clone(&db),
        Arc::clone(&enc),
    ));
    let server_repo = Arc::new(SqliteInstalledServerRepository::new(
        Arc::clone(&db),
        Arc::clone(&enc),
    ));
    let space_repo = Arc::new(SqliteSpaceRepository::new(Arc::clone(&db)));

    TestContext {
        db,
        _enc: enc,
        profile_repo,
        server_repo,
        space_repo,
        _dir: dir,
    }
}

#[tokio::test]
async fn test_credential_profile_crud() {
    let ctx = setup().await;

    let mut values = HashMap::new();
    values.insert("api_key".to_string(), "sk-live-123456789".to_string());
    values.insert(
        "endpoint".to_string(),
        "https://api.example.com".to_string(),
    );

    let profile = CredentialProfile::new("Work Profile", "com.example.mcp", values.clone())
        .with_server_name("Example MCP");
    let profile_id = profile.id;

    // 1. Save
    ctx.profile_repo.save(&profile).await.unwrap();

    // 2. Get by ID
    let loaded = ctx.profile_repo.get(&profile_id).await.unwrap().unwrap();
    assert_eq!(loaded.id, profile_id);
    assert_eq!(loaded.name, "Work Profile");
    assert_eq!(loaded.server_id, "com.example.mcp");
    assert_eq!(loaded.server_name.as_deref(), Some("Example MCP"));
    assert_eq!(loaded.values, values);

    // 3. List
    let all = ctx.profile_repo.list().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, profile_id);

    // 4. List for server
    let for_srv = ctx
        .profile_repo
        .list_for_server("com.example.mcp")
        .await
        .unwrap();
    assert_eq!(for_srv.len(), 1);

    let for_other = ctx
        .profile_repo
        .list_for_server("com.other.mcp")
        .await
        .unwrap();
    assert!(for_other.is_empty());

    // 5. Update
    let mut updated = loaded;
    updated.name = "Updated Work Profile".to_string();
    updated
        .values
        .insert("api_key".to_string(), "sk-live-NEW-KEY".to_string());
    ctx.profile_repo.save(&updated).await.unwrap();

    let reloaded = ctx.profile_repo.get(&profile_id).await.unwrap().unwrap();
    assert_eq!(reloaded.name, "Updated Work Profile");
    assert_eq!(reloaded.values.get("api_key").unwrap(), "sk-live-NEW-KEY");

    // 6. Delete
    ctx.profile_repo.delete(&profile_id).await.unwrap();
    let deleted = ctx.profile_repo.get(&profile_id).await.unwrap();
    assert!(deleted.is_none());
}

#[tokio::test]
async fn test_credential_profile_row_binding_fails_on_tampering() {
    let ctx = setup().await;

    let mut vals_a = HashMap::new();
    vals_a.insert("token".to_string(), "token_a".to_string());
    let profile_a = CredentialProfile::new("Profile A", "server-a", vals_a);

    let mut vals_b = HashMap::new();
    vals_b.insert("token".to_string(), "token_b".to_string());
    let profile_b = CredentialProfile::new("Profile B", "server-b", vals_b);

    ctx.profile_repo.save(&profile_a).await.unwrap();
    ctx.profile_repo.save(&profile_b).await.unwrap();

    // Directly overwrite Profile B's ciphertext with Profile A's ciphertext in SQLite
    ctx.db
        .lock()
        .await
        .connection()
        .execute(
            &format!(
                "UPDATE credential_profiles SET values_encrypted =
                   (SELECT values_encrypted FROM credential_profiles WHERE id = '{}')
                 WHERE id = '{}'",
                profile_a.id, profile_b.id
            ),
            [],
        )
        .unwrap();

    // Decrypting Profile B must FAIL because the AEAD authentication tag was bound to Profile A's id
    assert!(ctx.profile_repo.get(&profile_b.id).await.is_err());

    // Profile A must still decrypt cleanly
    let original_a = ctx.profile_repo.get(&profile_a.id).await.unwrap().unwrap();
    assert_eq!(original_a.values.get("token").unwrap(), "token_a");
}

#[tokio::test]
async fn test_uninstall_server_preserves_central_credential_profile() {
    let ctx = setup().await;

    // Create a space
    let space = fixtures::test_space("Test Space");
    ctx.space_repo.create(&space).await.unwrap();

    // Create central profile
    let mut values = HashMap::new();
    values.insert("api_key".to_string(), "permanent_secret".to_string());
    let profile = CredentialProfile::new("Central Account", "github", values.clone());
    let profile_id = profile.id;
    ctx.profile_repo.save(&profile).await.unwrap();

    // Install server linked to this profile
    let server = InstalledServer::new(&space.id.to_string(), "github")
        .with_credential_profile(Some(profile_id))
        .with_inputs(values);
    let server_id = server.id;
    ctx.server_repo.install(&server).await.unwrap();

    // Check usage count
    assert_eq!(ctx.profile_repo.count_usages(&profile_id).await.unwrap(), 1);

    // Verify server has profile ID
    let installed = ctx.server_repo.get(&server_id).await.unwrap().unwrap();
    assert_eq!(installed.credential_profile_id, Some(profile_id));

    // Uninstall server from the space (simulating user clicking uninstall)
    ctx.server_repo.uninstall(&server_id).await.unwrap();
    assert!(ctx.server_repo.get(&server_id).await.unwrap().is_none());

    // CRITICAL: The central credential profile MUST survive unchanged!
    let surviving_profile = ctx.profile_repo.get(&profile_id).await.unwrap().unwrap();
    assert_eq!(surviving_profile.name, "Central Account");
    assert_eq!(
        surviving_profile.values.get("api_key").unwrap(),
        "permanent_secret"
    );
    assert_eq!(ctx.profile_repo.count_usages(&profile_id).await.unwrap(), 0);
}

#[tokio::test]
async fn test_deleting_credential_profile_sets_installed_server_ref_to_null() {
    let ctx = setup().await;

    let space = fixtures::test_space("Test Space");
    ctx.space_repo.create(&space).await.unwrap();

    let mut values = HashMap::new();
    values.insert("token".to_string(), "secret".to_string());
    let profile = CredentialProfile::new("Temporary Profile", "slack", values.clone());
    let profile_id = profile.id;
    ctx.profile_repo.save(&profile).await.unwrap();

    let server = InstalledServer::new(&space.id.to_string(), "slack")
        .with_credential_profile(Some(profile_id))
        .with_inputs(values);
    let server_id = server.id;
    ctx.server_repo.install(&server).await.unwrap();

    // Delete the profile
    ctx.profile_repo.delete(&profile_id).await.unwrap();

    // The installed server should still exist, but credential_profile_id should now be None
    let remaining_server = ctx.server_repo.get(&server_id).await.unwrap().unwrap();
    assert_eq!(remaining_server.credential_profile_id, None);
    // Existing input_values are retained so running processes don't immediately break
    assert_eq!(
        remaining_server.input_values.get("token").unwrap(),
        "secret"
    );
}

#[tokio::test]
async fn test_single_point_rotation_propagates_to_all_linked_servers() {
    let ctx = setup().await;

    // Create 2 spaces: "Work" and "Client"
    let space_work = fixtures::test_space("Work");
    let space_client = fixtures::test_space("Client");
    ctx.space_repo.create(&space_work).await.unwrap();
    ctx.space_repo.create(&space_client).await.unwrap();

    // Create initial profile
    let mut initial_values = HashMap::new();
    initial_values.insert("api_key".to_string(), "v1_key".to_string());
    let profile = CredentialProfile::new("Shared API Key", "openai", initial_values.clone());
    let profile_id = profile.id;
    ctx.profile_repo.save(&profile).await.unwrap();

    // Install in Space Work
    let s1 = InstalledServer::new(&space_work.id.to_string(), "openai")
        .with_credential_profile(Some(profile_id))
        .with_inputs(initial_values.clone());
    let s1_id = s1.id;
    ctx.server_repo.install(&s1).await.unwrap();

    // Install in Space Client
    let s2 = InstalledServer::new(&space_client.id.to_string(), "openai")
        .with_credential_profile(Some(profile_id))
        .with_inputs(initial_values);
    let s2_id = s2.id;
    ctx.server_repo.install(&s2).await.unwrap();

    assert_eq!(ctx.profile_repo.count_usages(&profile_id).await.unwrap(), 2);

    // Now execute single-point rotation via CredentialProfileService
    let service = CredentialProfileService::new(
        Arc::clone(&ctx.profile_repo) as Arc<dyn CredentialProfileRepository>,
        Arc::clone(&ctx.server_repo) as Arc<dyn InstalledServerRepository>,
    );

    let mut rotated_values = HashMap::new();
    rotated_values.insert("api_key".to_string(), "v2_ROTATED_SECRET".to_string());

    service
        .update(&profile_id, "Shared API Key (Rotated)", rotated_values)
        .await
        .unwrap();

    // Verify both server installations in both spaces were automatically updated!
    let s1_updated = ctx.server_repo.get(&s1_id).await.unwrap().unwrap();
    let s2_updated = ctx.server_repo.get(&s2_id).await.unwrap().unwrap();

    assert_eq!(
        s1_updated.input_values.get("api_key").unwrap(),
        "v2_ROTATED_SECRET"
    );
    assert_eq!(
        s2_updated.input_values.get("api_key").unwrap(),
        "v2_ROTATED_SECRET"
    );
}
