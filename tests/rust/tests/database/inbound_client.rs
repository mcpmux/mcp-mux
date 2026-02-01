//! InboundClientRepository integration tests
//!
//! Tests for DCR registration, OAuth authorization codes, tokens, and client grants.
//! These test the INBOUND flow: AI clients (Cursor, Claude) connecting TO McpMux.

use mcpmux_core::repository::SpaceRepository;
use mcpmux_storage::{
    AuthorizationCode, InboundClient, InboundClientRepository, RegistrationType,
    SqliteSpaceRepository, TokenRecord, TokenType,
};
use std::sync::Arc;
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;

fn create_test_client(name: &str) -> InboundClient {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    InboundClient {
        client_id: format!("mcp_{}", &uuid::Uuid::new_v4().to_string()[..8]),
        registration_type: RegistrationType::Dcr,
        client_name: name.to_string(),
        client_alias: None,
        redirect_uris: vec!["http://127.0.0.1:8080/callback".to_string()],
        grant_types: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        response_types: vec!["code".to_string()],
        token_endpoint_auth_method: "none".to_string(),
        scope: Some("openid mcp".to_string()),
        approved: false,
        logo_uri: None,
        client_uri: None,
        software_id: Some("com.test.app".to_string()),
        software_version: Some("1.0.0".to_string()),
        metadata_url: None,
        metadata_cached_at: None,
        metadata_cache_ttl: None,
        connection_mode: "follow_active".to_string(),
        locked_space_id: None,
        last_seen: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

// =============================================================================
// Client Registration Tests (DCR)
// =============================================================================

#[tokio::test]
async fn test_save_and_get_client() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Test Client");
    let client_id = client.client_id.clone();

    repo.save_client(&client)
        .await
        .expect("Failed to save client");

    let loaded = repo
        .get_client(&client_id)
        .await
        .expect("Failed to get client");
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.client_name, "Test Client");
    assert_eq!(loaded.registration_type, RegistrationType::Dcr);
    assert!(!loaded.approved);
}

#[tokio::test]
async fn test_find_client_by_name() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Cursor IDE");
    repo.save_client(&client).await.unwrap();

    // Find by name
    let found = repo
        .find_client_by_name("Cursor IDE")
        .await
        .expect("Failed to find");
    assert!(found.is_some());
    assert_eq!(found.unwrap().client_id, client.client_id);

    // Non-existent name
    let not_found = repo
        .find_client_by_name("NonExistent")
        .await
        .expect("Failed to query");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_client_update_preserves_fields() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let mut client = create_test_client("Original");
    repo.save_client(&client).await.unwrap();

    // Update with new redirect URI
    client.redirect_uris.push("cursor://callback".to_string());
    client.software_version = Some("2.0.0".to_string());
    repo.save_client(&client).await.unwrap();

    let loaded = repo.get_client(&client.client_id).await.unwrap().unwrap();
    assert_eq!(loaded.redirect_uris.len(), 2);
    assert_eq!(loaded.software_version, Some("2.0.0".to_string()));
}

#[tokio::test]
async fn test_approve_client() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Pending Approval");
    repo.save_client(&client).await.unwrap();

    // Initially not approved
    assert!(!repo.is_client_approved(&client.client_id).await.unwrap());

    // Approve
    repo.approve_client(&client.client_id)
        .await
        .expect("Failed to approve");

    // Now approved
    assert!(repo.is_client_approved(&client.client_id).await.unwrap());

    // Verify in loaded client
    let loaded = repo.get_client(&client.client_id).await.unwrap().unwrap();
    assert!(loaded.approved);
}

#[tokio::test]
async fn test_list_clients() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client1 = create_test_client("Client A");
    let client2 = create_test_client("Client B");
    repo.save_client(&client1).await.unwrap();
    repo.save_client(&client2).await.unwrap();

    let clients = repo.list_clients().await.expect("Failed to list");
    assert!(clients.len() >= 2);
}

#[tokio::test]
async fn test_delete_client() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("To Delete");
    let client_id = client.client_id.clone();
    repo.save_client(&client).await.unwrap();

    // Delete
    let deleted = repo
        .delete_client(&client_id)
        .await
        .expect("Failed to delete");
    assert!(deleted);

    // Verify gone
    let loaded = repo.get_client(&client_id).await.unwrap();
    assert!(loaded.is_none());

    // Delete non-existent returns false
    let deleted_again = repo.delete_client(&client_id).await.unwrap();
    assert!(!deleted_again);
}

#[tokio::test]
async fn test_validate_redirect_uri() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let mut client = create_test_client("URI Test");
    client.redirect_uris = vec![
        "http://127.0.0.1:8080/callback".to_string(),
        "cursor://callback".to_string(),
    ];
    repo.save_client(&client).await.unwrap();

    // Valid URIs
    assert!(repo
        .validate_redirect_uri(&client.client_id, "http://127.0.0.1:8080/callback")
        .await
        .unwrap());
    assert!(repo
        .validate_redirect_uri(&client.client_id, "cursor://callback")
        .await
        .unwrap());

    // Invalid URI
    assert!(!repo
        .validate_redirect_uri(&client.client_id, "https://evil.com")
        .await
        .unwrap());
}

#[tokio::test]
async fn test_merge_redirect_uris() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let mut client = create_test_client("Merge Test");
    client.redirect_uris = vec!["http://127.0.0.1:8080/callback".to_string()];
    repo.save_client(&client).await.unwrap();

    // Merge new URIs
    let merged = repo
        .merge_redirect_uris(
            &client.client_id,
            vec![
                "cursor://callback".to_string(),
                "http://127.0.0.1:8080/callback".to_string(), // duplicate
            ],
        )
        .await
        .expect("Failed to merge");

    // Should have 2 (duplicate removed)
    assert_eq!(merged.len(), 2);
    assert!(merged.contains(&"http://127.0.0.1:8080/callback".to_string()));
    assert!(merged.contains(&"cursor://callback".to_string()));
}

// =============================================================================
// Authorization Code Tests
// =============================================================================

#[tokio::test]
async fn test_authorization_code_save_and_consume() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    // First save a client (FK constraint)
    let client = create_test_client("Auth Code Client");
    repo.save_client(&client).await.unwrap();

    let code = AuthorizationCode {
        code: "auth_code_123".to_string(),
        client_id: client.client_id.clone(),
        redirect_uri: "http://127.0.0.1:8080/callback".to_string(),
        scope: Some("openid".to_string()),
        code_challenge: Some("challenge_abc".to_string()),
        code_challenge_method: Some("S256".to_string()),
        expires_at: "2030-01-01T00:00:00Z".to_string(),
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };

    repo.save_authorization_code(&code)
        .await
        .expect("Failed to save code");

    // Consume (one-time use)
    let consumed = repo
        .consume_authorization_code("auth_code_123")
        .await
        .expect("Failed to consume");
    assert!(consumed.is_some());
    let consumed = consumed.unwrap();
    assert_eq!(consumed.client_id, client.client_id);
    assert_eq!(consumed.code_challenge, Some("challenge_abc".to_string()));

    // Second consume should return None
    let consumed_again = repo
        .consume_authorization_code("auth_code_123")
        .await
        .unwrap();
    assert!(consumed_again.is_none());
}

#[tokio::test]
async fn test_authorization_code_not_found() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let consumed = repo
        .consume_authorization_code("nonexistent")
        .await
        .unwrap();
    assert!(consumed.is_none());
}

// =============================================================================
// Token Tests
// =============================================================================

#[tokio::test]
async fn test_token_hash_consistency() {
    let hash1 = InboundClientRepository::hash_token("my_secret_token");
    let hash2 = InboundClientRepository::hash_token("my_secret_token");
    let hash3 = InboundClientRepository::hash_token("different_token");

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
    assert_eq!(hash1.len(), 64); // SHA-256 hex
}

#[tokio::test]
async fn test_save_and_find_token() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Token Client");
    repo.save_client(&client).await.unwrap();

    let token_value = "access_token_xyz";
    let token_hash = InboundClientRepository::hash_token(token_value);

    let record = TokenRecord {
        id: uuid::Uuid::new_v4().to_string(),
        client_id: client.client_id.clone(),
        token_type: TokenType::Access,
        token_hash: token_hash.clone(),
        scope: Some("openid".to_string()),
        expires_at: Some("2030-01-01T00:00:00Z".to_string()),
        revoked: false,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        parent_token_id: None,
    };

    repo.save_token(&record)
        .await
        .expect("Failed to save token");

    // Find by hash
    let found = repo
        .find_token_by_hash(&token_hash)
        .await
        .expect("Failed to find");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.client_id, client.client_id);
    assert_eq!(found.token_type, TokenType::Access);
    assert!(!found.revoked);
}

#[tokio::test]
async fn test_validate_token() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Validate Client");
    repo.save_client(&client).await.unwrap();

    let token_value = "valid_token_abc";
    let record = TokenRecord {
        id: uuid::Uuid::new_v4().to_string(),
        client_id: client.client_id.clone(),
        token_type: TokenType::Access,
        token_hash: InboundClientRepository::hash_token(token_value),
        scope: None,
        expires_at: Some("2030-01-01T00:00:00Z".to_string()),
        revoked: false,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        parent_token_id: None,
    };
    repo.save_token(&record).await.unwrap();

    // Valid token
    let validated = repo
        .validate_token(token_value)
        .await
        .expect("Failed to validate");
    assert!(validated.is_some());

    // Invalid token
    let invalid = repo.validate_token("wrong_token").await.unwrap();
    assert!(invalid.is_none());
}

#[tokio::test]
async fn test_validate_expired_token() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Expired Client");
    repo.save_client(&client).await.unwrap();

    let token_value = "expired_token";
    let record = TokenRecord {
        id: uuid::Uuid::new_v4().to_string(),
        client_id: client.client_id.clone(),
        token_type: TokenType::Access,
        token_hash: InboundClientRepository::hash_token(token_value),
        scope: None,
        expires_at: Some("2020-01-01T00:00:00Z".to_string()), // expired
        revoked: false,
        created_at: "2020-01-01T00:00:00Z".to_string(),
        parent_token_id: None,
    };
    repo.save_token(&record).await.unwrap();

    // Expired token should not validate
    let validated = repo.validate_token(token_value).await.unwrap();
    assert!(validated.is_none());
}

#[tokio::test]
async fn test_validate_revoked_token() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Revoked Client");
    repo.save_client(&client).await.unwrap();

    let token_value = "revoked_token";
    let record = TokenRecord {
        id: uuid::Uuid::new_v4().to_string(),
        client_id: client.client_id.clone(),
        token_type: TokenType::Access,
        token_hash: InboundClientRepository::hash_token(token_value),
        scope: None,
        expires_at: Some("2030-01-01T00:00:00Z".to_string()),
        revoked: true, // revoked
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        parent_token_id: None,
    };
    repo.save_token(&record).await.unwrap();

    // Revoked token should not validate
    let validated = repo.validate_token(token_value).await.unwrap();
    assert!(validated.is_none());
}

#[tokio::test]
async fn test_revoke_token_and_children() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Revoke Test");
    repo.save_client(&client).await.unwrap();

    let refresh_id = uuid::Uuid::new_v4().to_string();
    let access_id = uuid::Uuid::new_v4().to_string();

    // Create refresh token
    let refresh = TokenRecord {
        id: refresh_id.clone(),
        client_id: client.client_id.clone(),
        token_type: TokenType::Refresh,
        token_hash: InboundClientRepository::hash_token("refresh_token"),
        scope: None,
        expires_at: None,
        revoked: false,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        parent_token_id: None,
    };
    repo.save_token(&refresh).await.unwrap();

    // Create access token linked to refresh
    let access = TokenRecord {
        id: access_id.clone(),
        client_id: client.client_id.clone(),
        token_type: TokenType::Access,
        token_hash: InboundClientRepository::hash_token("access_token"),
        scope: None,
        expires_at: Some("2030-01-01T00:00:00Z".to_string()),
        revoked: false,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        parent_token_id: Some(refresh_id.clone()),
    };
    repo.save_token(&access).await.unwrap();

    // Revoke refresh token (should also revoke child access token)
    repo.revoke_token(&refresh_id)
        .await
        .expect("Failed to revoke");

    // Both should be revoked
    let refresh_check = repo
        .find_token_by_hash(&InboundClientRepository::hash_token("refresh_token"))
        .await
        .unwrap()
        .unwrap();
    let access_check = repo
        .find_token_by_hash(&InboundClientRepository::hash_token("access_token"))
        .await
        .unwrap()
        .unwrap();
    assert!(refresh_check.revoked);
    assert!(access_check.revoked);
}

#[tokio::test]
async fn test_revoke_client_tokens() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Multi Token");
    repo.save_client(&client).await.unwrap();

    // Create multiple tokens
    for i in 0..3 {
        let record = TokenRecord {
            id: uuid::Uuid::new_v4().to_string(),
            client_id: client.client_id.clone(),
            token_type: TokenType::Access,
            token_hash: InboundClientRepository::hash_token(&format!("token_{}", i)),
            scope: None,
            expires_at: None,
            revoked: false,
            created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            parent_token_id: None,
        };
        repo.save_token(&record).await.unwrap();
    }

    // Revoke all for client
    let count = repo
        .revoke_client_tokens(&client.client_id)
        .await
        .expect("Failed to revoke all");
    assert_eq!(count, 3);

    // Verify all revoked
    for i in 0..3 {
        let validated = repo.validate_token(&format!("token_{}", i)).await.unwrap();
        assert!(validated.is_none());
    }
}

// =============================================================================
// Client Grants Tests (Feature Set Permissions)
// =============================================================================

#[tokio::test]
async fn test_grant_feature_set() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // Create a space (auto-creates All and Default feature sets)
    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let client = create_test_client("Grant Client");
    repo.save_client(&client).await.unwrap();

    // Grant the auto-created "All" feature set
    let all_fs_id = format!("fs_all_{}", space.id);
    repo.grant_feature_set(&client.client_id, &space.id.to_string(), &all_fs_id)
        .await
        .expect("Failed to grant");

    // Check grants
    let grants = repo
        .get_grants_for_space(&client.client_id, &space.id.to_string())
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants.contains(&all_fs_id));
}

#[tokio::test]
async fn test_grant_multiple_feature_sets() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // Create two spaces
    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    SpaceRepository::create(&space_repo, &space1).await.unwrap();
    SpaceRepository::create(&space_repo, &space2).await.unwrap();

    let client = create_test_client("Multi Grant");
    repo.save_client(&client).await.unwrap();

    // Use auto-created feature set IDs
    let space1_all = format!("fs_all_{}", space1.id);
    let space1_default = format!("fs_default_{}", space1.id);
    let space2_all = format!("fs_all_{}", space2.id);

    repo.grant_feature_set(&client.client_id, &space1.id.to_string(), &space1_all)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &space1.id.to_string(), &space1_default)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &space2.id.to_string(), &space2_all)
        .await
        .unwrap();

    // Space 1 should have 2
    let grants1 = repo
        .get_grants_for_space(&client.client_id, &space1.id.to_string())
        .await
        .unwrap();
    assert_eq!(grants1.len(), 2);

    // Space 2 should have 1
    let grants2 = repo
        .get_grants_for_space(&client.client_id, &space2.id.to_string())
        .await
        .unwrap();
    assert_eq!(grants2.len(), 1);
}

#[tokio::test]
async fn test_grant_idempotent() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let client = create_test_client("Idempotent");
    repo.save_client(&client).await.unwrap();

    let all_fs_id = format!("fs_all_{}", space.id);

    // Grant same thing twice
    repo.grant_feature_set(&client.client_id, &space.id.to_string(), &all_fs_id)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &space.id.to_string(), &all_fs_id)
        .await
        .unwrap();

    // Should still be 1
    let grants = repo
        .get_grants_for_space(&client.client_id, &space.id.to_string())
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
}

#[tokio::test]
async fn test_revoke_feature_set() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Test Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let client = create_test_client("Revoke Grant");
    repo.save_client(&client).await.unwrap();

    let all_fs_id = format!("fs_all_{}", space.id);
    let default_fs_id = format!("fs_default_{}", space.id);

    repo.grant_feature_set(&client.client_id, &space.id.to_string(), &all_fs_id)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &space.id.to_string(), &default_fs_id)
        .await
        .unwrap();

    // Revoke one
    repo.revoke_feature_set(&client.client_id, &space.id.to_string(), &all_fs_id)
        .await
        .expect("Failed to revoke");

    // Only default remains
    let grants = repo
        .get_grants_for_space(&client.client_id, &space.id.to_string())
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants.contains(&default_fs_id));
}

#[tokio::test]
async fn test_get_all_grants() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let space1 = fixtures::test_space("Space 1");
    let space2 = fixtures::test_space("Space 2");
    SpaceRepository::create(&space_repo, &space1).await.unwrap();
    SpaceRepository::create(&space_repo, &space2).await.unwrap();

    let client = create_test_client("All Grants");
    repo.save_client(&client).await.unwrap();

    let space1_all = format!("fs_all_{}", space1.id);
    let space1_default = format!("fs_default_{}", space1.id);
    let space2_all = format!("fs_all_{}", space2.id);

    repo.grant_feature_set(&client.client_id, &space1.id.to_string(), &space1_all)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &space1.id.to_string(), &space1_default)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &space2.id.to_string(), &space2_all)
        .await
        .unwrap();

    let all_grants = repo
        .get_all_grants(&client.client_id)
        .await
        .expect("Failed to get all");
    assert_eq!(all_grants.len(), 2); // 2 spaces

    assert_eq!(all_grants.get(&space1.id.to_string()).unwrap().len(), 2);
    assert_eq!(all_grants.get(&space2.id.to_string()).unwrap().len(), 1);
}

#[tokio::test]
async fn test_grants_per_space_isolation() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    let work = fixtures::test_space("Work");
    let personal = fixtures::test_space("Personal");
    SpaceRepository::create(&space_repo, &work).await.unwrap();
    SpaceRepository::create(&space_repo, &personal)
        .await
        .unwrap();

    let client = create_test_client("Space Isolation");
    repo.save_client(&client).await.unwrap();

    let work_all = format!("fs_all_{}", work.id);
    let personal_all = format!("fs_all_{}", personal.id);

    // Grant "All" in different spaces
    repo.grant_feature_set(&client.client_id, &work.id.to_string(), &work_all)
        .await
        .unwrap();
    repo.grant_feature_set(&client.client_id, &personal.id.to_string(), &personal_all)
        .await
        .unwrap();

    // Revoke from work only
    repo.revoke_feature_set(&client.client_id, &work.id.to_string(), &work_all)
        .await
        .unwrap();

    // Work should be empty
    let work_grants = repo
        .get_grants_for_space(&client.client_id, &work.id.to_string())
        .await
        .unwrap();
    assert!(work_grants.is_empty());

    // Personal still has grant
    let personal_grants = repo
        .get_grants_for_space(&client.client_id, &personal.id.to_string())
        .await
        .unwrap();
    assert_eq!(personal_grants.len(), 1);
}

// =============================================================================
// Client Settings Update Tests
// =============================================================================

#[tokio::test]
async fn test_update_client_settings() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(Arc::clone(&db));
    let space_repo = SqliteSpaceRepository::new(db);

    // Create a space for locking
    let space = fixtures::test_space("Locked Space");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let client = create_test_client("Settings Test");
    repo.save_client(&client).await.unwrap();

    // Update settings
    let updated = repo
        .update_client_settings(
            &client.client_id,
            Some("My Cursor".to_string()),    // alias
            Some("locked".to_string()),       // connection_mode
            Some(Some(space.id.to_string())), // locked_space_id
        )
        .await
        .expect("Failed to update settings");

    assert!(updated.is_some());
    let updated = updated.unwrap();
    assert_eq!(updated.client_alias, Some("My Cursor".to_string()));
    assert_eq!(updated.connection_mode, "locked");
    assert_eq!(updated.locked_space_id, Some(space.id.to_string()));
}

#[tokio::test]
async fn test_update_last_seen() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Last Seen");
    repo.save_client(&client).await.unwrap();

    // Initially no last_seen
    let loaded = repo.get_client(&client.client_id).await.unwrap().unwrap();
    assert!(loaded.last_seen.is_none());

    // Update last seen
    repo.update_client_last_seen(&client.client_id)
        .await
        .expect("Failed to update last_seen");

    // Now has last_seen
    let updated = repo.get_client(&client.client_id).await.unwrap().unwrap();
    assert!(updated.last_seen.is_some());
}
