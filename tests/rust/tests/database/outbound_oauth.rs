//! OutboundOAuthRepository and CredentialRepository integration tests
//!
//! Tests for OUTBOUND flow: McpMux connecting TO backend MCP servers.
//! Handles OAuth client registrations (DCR with servers) and token storage.

use chrono::{Duration, Utc};
use mcpmux_core::domain::{Credential, CredentialType, OutboundOAuthRegistration};
use mcpmux_core::repository::{
    CredentialRepository, InstalledServerRepository, OutboundOAuthRepository, SpaceRepository,
};
use mcpmux_core::{create_shared_event_bus, ServerAppService};
use mcpmux_storage::{
    generate_master_key, FieldEncryptor, SqliteCredentialRepository,
    SqliteInstalledServerRepository, SqliteOutboundOAuthRepository, SqliteSpaceRepository,
};
use std::sync::Arc;
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Create a test encryptor for credential encryption
fn test_encryptor() -> Arc<FieldEncryptor> {
    let key = generate_master_key().expect("Failed to generate key");
    Arc::new(FieldEncryptor::new(&key).expect("Failed to create encryptor"))
}

// =============================================================================
// OutboundOAuthRepository Tests
// =============================================================================

fn create_test_registration(space_id: Uuid, server_id: &str) -> OutboundOAuthRegistration {
    OutboundOAuthRegistration::new(
        space_id,
        server_id,
        "https://server.example.com",
        format!("dcr_client_{}", &Uuid::new_v4().to_string()[..8]),
        "http://127.0.0.1:9876/callback",
    )
}

#[tokio::test]
async fn test_save_and_get_registration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let reg = create_test_registration(space.id, "atlassian-mcp");
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .expect("Failed to save");

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "atlassian-mcp")
        .await
        .expect("Failed to get");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.server_id, "atlassian-mcp");
    assert_eq!(loaded.server_url, "https://server.example.com");
}

#[tokio::test]
async fn test_registration_not_found() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(db, test_encryptor());

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &Uuid::new_v4(), "nonexistent")
        .await
        .unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn test_update_registration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let mut reg = create_test_registration(space.id, "github-mcp");
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();

    // Update redirect_uri
    reg.redirect_uri = Some("http://127.0.0.1:9999/callback".to_string());
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "github-mcp")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded.redirect_uri,
        Some("http://127.0.0.1:9999/callback".to_string())
    );
}

#[tokio::test]
async fn test_delete_registration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let reg = create_test_registration(space.id, "to-delete");
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();

    OutboundOAuthRepository::delete(&oauth_repo, &space.id, "to-delete")
        .await
        .unwrap();

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "to-delete")
        .await
        .unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn test_list_registrations_for_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let reg1 = create_test_registration(space.id, "server-1");
    let reg2 = create_test_registration(space.id, "server-2");
    OutboundOAuthRepository::save(&oauth_repo, &reg1)
        .await
        .unwrap();
    OutboundOAuthRepository::save(&oauth_repo, &reg2)
        .await
        .unwrap();

    let list = oauth_repo.list_for_space(&space.id).await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn test_registrations_isolated_by_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space_a = fixtures::test_space("Space A");
    let space_b = fixtures::test_space("Space B");
    SpaceRepository::create(&space_repo, &space_a)
        .await
        .unwrap();
    SpaceRepository::create(&space_repo, &space_b)
        .await
        .unwrap();

    let reg_a = create_test_registration(space_a.id, "shared-server");
    let reg_b = create_test_registration(space_b.id, "shared-server");
    OutboundOAuthRepository::save(&oauth_repo, &reg_a)
        .await
        .unwrap();
    OutboundOAuthRepository::save(&oauth_repo, &reg_b)
        .await
        .unwrap();

    let list_a = oauth_repo.list_for_space(&space_a.id).await.unwrap();
    let list_b = oauth_repo.list_for_space(&space_b.id).await.unwrap();
    assert_eq!(list_a.len(), 1);
    assert_eq!(list_b.len(), 1);
}

#[tokio::test]
async fn test_client_secret_round_trips_and_is_encrypted_at_rest() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(Arc::clone(&db));

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let reg = create_test_registration(space.id, "confidential")
        .with_client_secret(Some("dcr-secret-value".to_string()));
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "confidential")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.client_secret.as_deref(), Some("dcr-secret-value"));

    let listed = oauth_repo.list_for_space(&space.id).await.unwrap();
    assert_eq!(listed[0].client_secret.as_deref(), Some("dcr-secret-value"));

    let raw: Option<String> = db
        .lock()
        .await
        .connection()
        .query_row(
            "SELECT client_secret_encrypted FROM outbound_oauth_clients WHERE server_id = ?1",
            ["confidential"],
            |r| r.get(0),
        )
        .unwrap();
    let raw = raw.expect("secret column is set");
    assert!(
        !raw.contains("dcr-secret-value"),
        "client secret stored in plaintext: {raw}"
    );
}

#[tokio::test]
async fn test_public_client_has_no_secret() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let reg = create_test_registration(space.id, "public");
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "public")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.client_secret, None);
}

/// During the token exchange the credential store saves a bare registration (no
/// secret, no metadata) before the OAuth flow saves the full one. The upsert must
/// then write the secret, or it never lands.
#[tokio::test]
async fn test_upsert_adds_secret_to_bare_registration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let bare = create_test_registration(space.id, "confidential");
    OutboundOAuthRepository::save(&oauth_repo, &bare)
        .await
        .unwrap();

    let full = bare
        .clone()
        .with_client_secret(Some("dcr-secret-value".to_string()));
    OutboundOAuthRepository::save(&oauth_repo, &full)
        .await
        .unwrap();

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "confidential")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.client_id, bare.client_id);
    assert_eq!(loaded.client_secret.as_deref(), Some("dcr-secret-value"));
}

/// A secret encrypted under a different master key can't be decrypted. The lookup
/// fails, like a credential that can't be decrypted, rather than returning the
/// client without its secret: the sign-in flow then registers a fresh client on
/// its first attempt instead of reusing one that can only fail. The client_id
/// alone (all the per-request credential load needs) still reads.
#[tokio::test]
async fn test_undecryptable_secret_fails_the_lookup() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let space_repo = SqliteSpaceRepository::new(Arc::clone(&db));

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let old_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let reg = create_test_registration(space.id, "confidential")
        .with_client_secret(Some("dcr-secret-value".to_string()));
    OutboundOAuthRepository::save(&old_repo, &reg)
        .await
        .unwrap();

    let new_repo = SqliteOutboundOAuthRepository::new(db, test_encryptor());
    let err = OutboundOAuthRepository::get(&new_repo, &space.id, "confidential")
        .await
        .expect_err("a secret that can't be decrypted fails the lookup");
    assert!(
        err.to_string().contains("decrypt"),
        "unexpected error: {err}"
    );
    assert!(new_repo.list_for_space(&space.id).await.is_err());

    let client_id = new_repo
        .get_client_id(&space.id, "confidential")
        .await
        .unwrap();
    assert_eq!(client_id.as_deref(), Some(reg.client_id.as_str()));

    // Saving a fresh registration replaces the unreadable row
    let fresh = create_test_registration(space.id, "confidential")
        .with_client_secret(Some("new-secret".to_string()));
    OutboundOAuthRepository::save(&new_repo, &fresh)
        .await
        .unwrap();
    let loaded = OutboundOAuthRepository::get(&new_repo, &space.id, "confidential")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.client_id, fresh.client_id);
    assert_eq!(loaded.client_secret.as_deref(), Some("new-secret"));
}

#[tokio::test]
async fn test_client_auth_method_and_secret_expiry_round_trip() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Second precision, as RFC 7591 sends it
    let expires_at = chrono::DateTime::from_timestamp(Utc::now().timestamp() + 3600, 0).unwrap();
    let reg = create_test_registration(space.id, "confidential")
        .with_client_secret(Some("dcr-secret-value".to_string()))
        .with_client_secret_expires_at(Some(expires_at))
        .with_token_endpoint_auth_method(Some("client_secret_post".to_string()));
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "confidential")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.client_secret_expires_at, Some(expires_at));
    assert_eq!(
        loaded.token_endpoint_auth_method.as_deref(),
        Some("client_secret_post")
    );

    let listed = oauth_repo.list_for_space(&space.id).await.unwrap();
    assert_eq!(listed[0].client_secret_expires_at, Some(expires_at));

    // A registration saved without them (e.g. the credential store's bare one)
    // reads back as None
    let bare = create_test_registration(space.id, "public");
    OutboundOAuthRepository::save(&oauth_repo, &bare)
        .await
        .unwrap();
    let loaded = OutboundOAuthRepository::get(&oauth_repo, &space.id, "public")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.client_secret_expires_at, None);
    assert_eq!(loaded.token_endpoint_auth_method, None);
}

#[tokio::test]
async fn test_get_client_id() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    assert_eq!(
        oauth_repo.get_client_id(&space.id, "server").await.unwrap(),
        None
    );

    let reg = create_test_registration(space.id, "server");
    OutboundOAuthRepository::save(&oauth_repo, &reg)
        .await
        .unwrap();
    assert_eq!(
        oauth_repo.get_client_id(&space.id, "server").await.unwrap(),
        Some(reg.client_id)
    );
}

// =============================================================================
// CredentialRepository Tests (Typed Rows)
// =============================================================================

/// Helper: save access_token + refresh_token as separate rows
async fn save_oauth_credentials(
    repo: &SqliteCredentialRepository,
    space_id: Uuid,
    server_id: &str,
) {
    let expires_at = Some(Utc::now() + Duration::hours(1));
    let access = Credential::access_token(space_id, server_id, "access_token_xyz", expires_at);
    let refresh = Credential::refresh_token(space_id, server_id, "refresh_token_abc", None);
    CredentialRepository::save(repo, &access).await.unwrap();
    CredentialRepository::save(repo, &refresh).await.unwrap();
}

fn create_api_key_credential(space_id: Uuid, server_id: &str, api_key: &str) -> Credential {
    Credential::api_key(space_id, server_id, api_key)
}

#[tokio::test]
async fn test_save_and_get_credential() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    save_oauth_credentials(&cred_repo, space.id, "server-oauth").await;

    // Load access_token
    let loaded = CredentialRepository::get(
        &cred_repo,
        &space.id,
        "server-oauth",
        &CredentialType::AccessToken,
    )
    .await
    .expect("Failed to get")
    .expect("Should find access token");

    assert_eq!(loaded.value, "access_token_xyz");
    assert_eq!(loaded.credential_type, CredentialType::AccessToken);
    assert_eq!(loaded.token_type, Some("Bearer".to_string()));

    // Load refresh_token
    let refresh = CredentialRepository::get(
        &cred_repo,
        &space.id,
        "server-oauth",
        &CredentialType::RefreshToken,
    )
    .await
    .unwrap()
    .expect("Should find refresh token");

    assert_eq!(refresh.value, "refresh_token_abc");
    assert_eq!(refresh.credential_type, CredentialType::RefreshToken);
}

#[tokio::test]
async fn test_credential_not_found() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(db, encryptor);

    let loaded = CredentialRepository::get(
        &cred_repo,
        &Uuid::new_v4(),
        "nonexistent",
        &CredentialType::AccessToken,
    )
    .await
    .unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn test_save_api_key_credential() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let cred = create_api_key_credential(space.id, "api-server", "my_secret_api_key");
    CredentialRepository::save(&cred_repo, &cred).await.unwrap();

    let loaded =
        CredentialRepository::get(&cred_repo, &space.id, "api-server", &CredentialType::ApiKey)
            .await
            .unwrap()
            .unwrap();

    assert_eq!(loaded.value, "my_secret_api_key");
    assert_eq!(loaded.credential_type, CredentialType::ApiKey);
}

#[tokio::test]
async fn test_update_credential() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Save initial access token
    let cred = Credential::access_token(space.id, "server-1", "old_token", None);
    CredentialRepository::save(&cred_repo, &cred).await.unwrap();

    // Update with new token
    let updated = Credential::access_token(space.id, "server-1", "new_access_token", None);
    CredentialRepository::save(&cred_repo, &updated)
        .await
        .unwrap();

    let loaded = CredentialRepository::get(
        &cred_repo,
        &space.id,
        "server-1",
        &CredentialType::AccessToken,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(loaded.value, "new_access_token");
}

#[tokio::test]
async fn test_delete_credential() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    save_oauth_credentials(&cred_repo, space.id, "to-delete").await;

    // Delete just the access token
    CredentialRepository::delete(
        &cred_repo,
        &space.id,
        "to-delete",
        &CredentialType::AccessToken,
    )
    .await
    .unwrap();

    // Access token gone, refresh token still there
    let access = CredentialRepository::get(
        &cred_repo,
        &space.id,
        "to-delete",
        &CredentialType::AccessToken,
    )
    .await
    .unwrap();
    assert!(access.is_none());

    let refresh = CredentialRepository::get(
        &cred_repo,
        &space.id,
        "to-delete",
        &CredentialType::RefreshToken,
    )
    .await
    .unwrap();
    assert!(refresh.is_some());

    // Delete all remaining
    cred_repo.delete_all(&space.id, "to-delete").await.unwrap();
    let all = cred_repo.get_all(&space.id, "to-delete").await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_list_credentials_for_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // 2 rows for server-1 (access + refresh), 2 rows for server-2
    save_oauth_credentials(&cred_repo, space.id, "server-1").await;
    save_oauth_credentials(&cred_repo, space.id, "server-2").await;

    let list = cred_repo.list_for_space(&space.id).await.unwrap();
    assert_eq!(list.len(), 4); // 2 servers × 2 types each
}

#[tokio::test]
async fn test_credentials_isolated_by_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space_a = fixtures::test_space("Space A");
    let space_b = fixtures::test_space("Space B");
    SpaceRepository::create(&space_repo, &space_a)
        .await
        .unwrap();
    SpaceRepository::create(&space_repo, &space_b)
        .await
        .unwrap();

    save_oauth_credentials(&cred_repo, space_a.id, "shared-server").await;
    save_oauth_credentials(&cred_repo, space_b.id, "shared-server").await;

    let list_a = cred_repo.list_for_space(&space_a.id).await.unwrap();
    let list_b = cred_repo.list_for_space(&space_b.id).await.unwrap();
    assert_eq!(list_a.len(), 2); // access + refresh
    assert_eq!(list_b.len(), 2);
}

// =============================================================================
// Token Expiration Tests
// =============================================================================

#[tokio::test]
async fn test_credential_expiration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Create access token that expires in the future
    let future_expiry = Some(Utc::now() + Duration::hours(2));
    let not_expired =
        Credential::access_token(space.id, "valid-server", "access_token", future_expiry);
    CredentialRepository::save(&cred_repo, &not_expired)
        .await
        .unwrap();

    let loaded = CredentialRepository::get(
        &cred_repo,
        &space.id,
        "valid-server",
        &CredentialType::AccessToken,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!loaded.is_expired());
    assert!(loaded.is_oauth());
}

#[tokio::test]
async fn test_clear_tokens_preserves_api_keys() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Save access_token, refresh_token, and api_key
    save_oauth_credentials(&cred_repo, space.id, "server").await;
    let api_key = Credential::api_key(space.id, "server", "key123");
    CredentialRepository::save(&cred_repo, &api_key)
        .await
        .unwrap();

    // clear_tokens should remove access + refresh, keep api_key
    let cleared = cred_repo.clear_tokens(&space.id, "server").await.unwrap();
    assert!(cleared);

    let all = cred_repo.get_all(&space.id, "server").await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].credential_type, CredentialType::ApiKey);
}

// =============================================================================
// Encryption Tests
// =============================================================================

#[tokio::test]
async fn test_different_encryptors_cannot_read_each_others_data() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor1 = test_encryptor();
    let space_repo = SqliteSpaceRepository::new(Arc::clone(&db));
    let cred_repo1 = SqliteCredentialRepository::new(Arc::clone(&db), encryptor1);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let cred = create_api_key_credential(space.id, "encrypted-server", "my_secret");
    CredentialRepository::save(&cred_repo1, &cred)
        .await
        .unwrap();

    // Create new encryptor with different key
    let encryptor2 = test_encryptor();
    let cred_repo2 = SqliteCredentialRepository::new(db, encryptor2);

    // Reading with wrong key should fail
    let result = CredentialRepository::get(
        &cred_repo2,
        &space.id,
        "encrypted-server",
        &CredentialType::ApiKey,
    )
    .await;
    assert!(
        result.is_err() || result.unwrap().is_none(),
        "Should fail to decrypt with wrong key"
    );
}

/// Uninstalling a server deletes its OAuth client registration, client secret
/// included, along with its tokens. Other servers' registrations stay.
#[tokio::test]
async fn test_uninstall_deletes_the_client_registration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let oauth_repo = Arc::new(SqliteOutboundOAuthRepository::new(
        Arc::clone(&db),
        Arc::clone(&encryptor),
    ));
    let cred_repo = Arc::new(SqliteCredentialRepository::new(
        Arc::clone(&db),
        Arc::clone(&encryptor),
    ));
    let server_repo = Arc::new(SqliteInstalledServerRepository::new(
        Arc::clone(&db),
        encryptor,
    ));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    for server_id in ["removed-server", "kept-server"] {
        let server = fixtures::test_installed_server(&space.id.to_string(), server_id);
        InstalledServerRepository::install(server_repo.as_ref(), &server)
            .await
            .unwrap();
        let registration = create_test_registration(space.id, server_id)
            .with_client_secret(Some("s3cr3t-value".to_string()));
        OutboundOAuthRepository::save(oauth_repo.as_ref(), &registration)
            .await
            .unwrap();
        let token = Credential::access_token(space.id, server_id, "token", None);
        CredentialRepository::save(cred_repo.as_ref(), &token)
            .await
            .unwrap();
    }

    let service = ServerAppService::new(
        server_repo,
        None,
        Some(cred_repo.clone()),
        create_shared_event_bus().sender(),
    )
    .with_outbound_oauth_repo(oauth_repo.clone());
    service.uninstall(space.id, "removed-server").await.unwrap();

    let removed = OutboundOAuthRepository::get(oauth_repo.as_ref(), &space.id, "removed-server")
        .await
        .unwrap();
    assert!(
        removed.is_none(),
        "registration left behind after uninstall"
    );
    let removed_token = CredentialRepository::get(
        cred_repo.as_ref(),
        &space.id,
        "removed-server",
        &CredentialType::AccessToken,
    )
    .await
    .unwrap();
    assert!(removed_token.is_none());

    let kept = OutboundOAuthRepository::get(oauth_repo.as_ref(), &space.id, "kept-server")
        .await
        .unwrap()
        .expect("other server's registration kept");
    assert_eq!(kept.client_secret.as_deref(), Some("s3cr3t-value"));
}
