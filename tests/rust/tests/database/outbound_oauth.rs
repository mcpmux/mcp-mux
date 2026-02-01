//! OutboundOAuthRepository and CredentialRepository integration tests
//!
//! Tests for OUTBOUND flow: McpMux connecting TO backend MCP servers.
//! Handles OAuth client registrations (DCR with servers) and token storage.

use chrono::{Duration, Utc};
use mcpmux_core::domain::{Credential, CredentialValue, OutboundOAuthRegistration};
use mcpmux_core::repository::{CredentialRepository, OutboundOAuthRepository, SpaceRepository};
use mcpmux_storage::{
    generate_master_key, FieldEncryptor, SqliteCredentialRepository, SqliteOutboundOAuthRepository,
    SqliteSpaceRepository,
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
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db));
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
    let oauth_repo = SqliteOutboundOAuthRepository::new(db);

    let loaded = OutboundOAuthRepository::get(&oauth_repo, &Uuid::new_v4(), "nonexistent")
        .await
        .unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn test_update_registration() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db));
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
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db));
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
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db));
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
    let oauth_repo = SqliteOutboundOAuthRepository::new(Arc::clone(&db));
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

// =============================================================================
// CredentialRepository Tests
// =============================================================================

fn create_oauth_credential(space_id: Uuid, server_id: &str) -> Credential {
    let expires_at = Some(Utc::now() + Duration::hours(1));
    Credential::oauth(
        space_id,
        server_id,
        "access_token_xyz",
        Some("refresh_token_abc".to_string()),
        expires_at,
    )
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

    let cred = create_oauth_credential(space.id, "server-oauth");
    CredentialRepository::save(&cred_repo, &cred)
        .await
        .expect("Failed to save");

    let loaded = CredentialRepository::get(&cred_repo, &space.id, "server-oauth")
        .await
        .expect("Failed to get");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();

    match &loaded.value {
        CredentialValue::OAuth {
            access_token,
            refresh_token,
            ..
        } => {
            assert_eq!(access_token, "access_token_xyz");
            assert_eq!(refresh_token.as_deref(), Some("refresh_token_abc"));
        }
        _ => panic!("Expected OAuth credential"),
    }
}

#[tokio::test]
async fn test_credential_not_found() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(db, encryptor);

    let loaded = CredentialRepository::get(&cred_repo, &Uuid::new_v4(), "nonexistent")
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

    let loaded = CredentialRepository::get(&cred_repo, &space.id, "api-server")
        .await
        .unwrap()
        .unwrap();

    match &loaded.value {
        CredentialValue::ApiKey { key } => {
            assert_eq!(key, "my_secret_api_key");
        }
        _ => panic!("Expected ApiKey"),
    }
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

    let mut cred = create_oauth_credential(space.id, "server-1");
    CredentialRepository::save(&cred_repo, &cred).await.unwrap();

    // Update with new tokens
    cred.value = CredentialValue::OAuth {
        access_token: "new_access_token".to_string(),
        refresh_token: Some("new_refresh_token".to_string()),
        expires_at: None,
        token_type: "Bearer".to_string(),
        scope: None,
    };
    CredentialRepository::save(&cred_repo, &cred).await.unwrap();

    let loaded = CredentialRepository::get(&cred_repo, &space.id, "server-1")
        .await
        .unwrap()
        .unwrap();

    match &loaded.value {
        CredentialValue::OAuth { access_token, .. } => {
            assert_eq!(access_token, "new_access_token");
        }
        _ => panic!("Expected OAuth credential"),
    }
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

    let cred = create_oauth_credential(space.id, "to-delete");
    CredentialRepository::save(&cred_repo, &cred).await.unwrap();

    CredentialRepository::delete(&cred_repo, &space.id, "to-delete")
        .await
        .unwrap();

    let loaded = CredentialRepository::get(&cred_repo, &space.id, "to-delete")
        .await
        .unwrap();
    assert!(loaded.is_none());
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

    let cred1 = create_oauth_credential(space.id, "server-1");
    let cred2 = create_oauth_credential(space.id, "server-2");
    CredentialRepository::save(&cred_repo, &cred1)
        .await
        .unwrap();
    CredentialRepository::save(&cred_repo, &cred2)
        .await
        .unwrap();

    let list = cred_repo.list_for_space(&space.id).await.unwrap();
    assert_eq!(list.len(), 2);
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

    let cred_a = create_oauth_credential(space_a.id, "shared-server");
    let cred_b = create_oauth_credential(space_b.id, "shared-server");
    CredentialRepository::save(&cred_repo, &cred_a)
        .await
        .unwrap();
    CredentialRepository::save(&cred_repo, &cred_b)
        .await
        .unwrap();

    let list_a = cred_repo.list_for_space(&space_a.id).await.unwrap();
    let list_b = cred_repo.list_for_space(&space_b.id).await.unwrap();
    assert_eq!(list_a.len(), 1);
    assert_eq!(list_b.len(), 1);
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

    // Create credential that expires in the future
    let future_expiry = Some(Utc::now() + Duration::hours(2));
    let not_expired = Credential::oauth(
        space.id,
        "valid-server",
        "access_token",
        Some("refresh".to_string()),
        future_expiry,
    );
    CredentialRepository::save(&cred_repo, &not_expired)
        .await
        .unwrap();

    let loaded = CredentialRepository::get(&cred_repo, &space.id, "valid-server")
        .await
        .unwrap()
        .unwrap();
    assert!(!loaded.is_expired());
    assert!(loaded.can_refresh());
}

#[tokio::test]
async fn test_credential_without_refresh_token() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let cred_repo = SqliteCredentialRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    // Create credential without refresh token
    let no_refresh = Credential::oauth(space.id, "no-refresh-server", "access_token", None, None);
    CredentialRepository::save(&cred_repo, &no_refresh)
        .await
        .unwrap();

    let loaded = CredentialRepository::get(&cred_repo, &space.id, "no-refresh-server")
        .await
        .unwrap()
        .unwrap();
    assert!(!loaded.can_refresh());
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
    let result = CredentialRepository::get(&cred_repo2, &space.id, "encrypted-server").await;
    assert!(
        result.is_err() || result.unwrap().is_none(),
        "Should fail to decrypt with wrong key"
    );
}
