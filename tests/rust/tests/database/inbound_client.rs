//! InboundClientRepository integration tests
//!
//! Tests for DCR registration, OAuth authorization codes, tokens, and client grants.
//! These test the INBOUND flow: AI clients (Cursor, Claude) connecting TO McpMux.

use mcpmux_storage::{
    AuthorizationCode, InboundClient, InboundClientRepository, RegistrationType, TokenRecord,
    TokenType,
};
use std::sync::Arc;
use tests::db::TestDatabase;
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
// Client Grants Tests — REMOVED in migration 003.
//
// The `client_grants` table and the repository methods that backed it were
// dropped once the FeatureSetResolver (pin > workspace binding > space-active)
// became authoritative. The trait methods remain as no-op shims for API
// compatibility with Tauri commands, but they no longer persist anything.
//
// For resolver decision-table tests see
// `tests/integration/feature_set_resolver.rs`.
// =============================================================================

// =============================================================================
// Client Settings Update Tests
// =============================================================================

#[tokio::test]
async fn test_update_client_alias() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let repo = InboundClientRepository::new(db);

    let client = create_test_client("Alias Test");
    repo.save_client(&client).await.unwrap();

    let updated = repo
        .update_client_alias(&client.client_id, Some("My Cursor".to_string()))
        .await
        .expect("Failed to update alias");

    let updated = updated.expect("client should exist after alias update");
    assert_eq!(updated.client_alias, Some("My Cursor".to_string()));
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
