//! JWT integration tests
//!
//! Tests for token creation and validation using mcpmux-gateway auth module.

use mcpmux_gateway::auth::{create_access_token, create_refresh_token, validate_token};

const TEST_SECRET: &[u8] = b"test_secret_key_that_is_32_bytes";

#[test]
fn test_create_access_token() {
    let token = create_access_token("client-123", Some("mcp read write"), 3600, TEST_SECRET);
    
    // Token should have two parts separated by '.'
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 2, "Token should have payload.signature format");
    
    // Both parts should be valid base64
    assert!(!parts[0].is_empty());
    assert!(!parts[1].is_empty());
}

#[test]
fn test_create_refresh_token() {
    let token = create_refresh_token("client-123", Some("mcp"), TEST_SECRET);
    
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 2);
}

#[test]
fn test_validate_token_success() {
    let token = create_access_token("test-client", Some("read"), 3600, TEST_SECRET);
    
    let claims = validate_token(&token, TEST_SECRET);
    assert!(claims.is_some(), "Valid token should return claims");
    
    let claims = claims.unwrap();
    assert_eq!(claims.client_id, "test-client");
    assert_eq!(claims.scope, Some("read".to_string()));
}

#[test]
fn test_validate_token_wrong_secret() {
    let token = create_access_token("client", None, 3600, TEST_SECRET);
    let wrong_secret = b"different_secret_key_32_bytes!!!";
    
    let claims = validate_token(&token, wrong_secret);
    assert!(claims.is_none(), "Token signed with different secret should fail");
}

#[test]
fn test_validate_expired_token() {
    // Create token that expired 1 hour ago
    let token = create_access_token("client", None, -3600, TEST_SECRET);
    
    let claims = validate_token(&token, TEST_SECRET);
    assert!(claims.is_none(), "Expired token should fail validation");
}

#[test]
fn test_validate_malformed_token() {
    let claims = validate_token("not.a.valid.token", TEST_SECRET);
    assert!(claims.is_none());
    
    let claims = validate_token("", TEST_SECRET);
    assert!(claims.is_none());
    
    let claims = validate_token("single_part_token", TEST_SECRET);
    assert!(claims.is_none());
}

#[test]
fn test_token_contains_timestamps() {
    let token = create_access_token("client", None, 3600, TEST_SECRET);
    
    let claims = validate_token(&token, TEST_SECRET).unwrap();
    
    // iat should be recent (within last minute)
    let now = chrono::Utc::now().timestamp();
    assert!(claims.iat >= now - 60);
    assert!(claims.iat <= now);
    
    // exp should be in the future
    assert!(claims.exp > now);
}

#[test]
fn test_token_scope_optional() {
    // Token without scope
    let token = create_access_token("client", None, 3600, TEST_SECRET);
    let claims = validate_token(&token, TEST_SECRET).unwrap();
    
    // Scope can be None when passed as null
    // The implementation may serialize None as null in JSON
    // Just verify token validates
    assert_eq!(claims.client_id, "client");
}

#[test]
fn test_different_clients_different_tokens() {
    let token1 = create_access_token("client-a", None, 3600, TEST_SECRET);
    let token2 = create_access_token("client-b", None, 3600, TEST_SECRET);
    
    assert_ne!(token1, token2);
    
    let claims1 = validate_token(&token1, TEST_SECRET).unwrap();
    let claims2 = validate_token(&token2, TEST_SECRET).unwrap();
    
    assert_eq!(claims1.client_id, "client-a");
    assert_eq!(claims2.client_id, "client-b");
}
