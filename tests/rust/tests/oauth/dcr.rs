//! Dynamic Client Registration (DCR) tests
//!
//! Tests for DCR validation logic. Full integration tests with database
//! are in the database test suite.

use mcpmux_gateway::oauth::{validate_redirect_uris, DcrError, DcrRequest};

// =============================================================================
// Redirect URI Validation Tests
// =============================================================================

#[test]
fn test_loopback_ipv4_valid() {
    assert!(validate_redirect_uris(&["http://127.0.0.1:8080/callback".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["http://127.0.0.1/oauth".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["http://127.0.0.1:3000/auth/callback".to_string()]).is_ok());
}

#[test]
fn test_loopback_localhost_valid() {
    assert!(validate_redirect_uris(&["http://localhost:8080/callback".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["http://localhost/oauth".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["http://localhost:9999/".to_string()]).is_ok());
}

#[test]
fn test_loopback_ipv6_valid() {
    assert!(validate_redirect_uris(&["http://[::1]:8080/callback".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["http://[::1]/oauth".to_string()]).is_ok());
}

#[test]
fn test_custom_scheme_valid() {
    assert!(validate_redirect_uris(&["cursor://callback".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["vscode://callback".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["claude://auth/callback".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["myapp://oauth".to_string()]).is_ok());
    assert!(validate_redirect_uris(&["com.example.app://callback".to_string()]).is_ok());
}

#[test]
fn test_multiple_valid_uris() {
    let uris = vec![
        "http://127.0.0.1:8080/callback".to_string(),
        "cursor://callback".to_string(),
        "http://localhost:3000/auth".to_string(),
    ];
    assert!(validate_redirect_uris(&uris).is_ok());
}

#[test]
fn test_empty_uris_rejected() {
    let result = validate_redirect_uris(&[]);
    assert!(result.is_err());
    match result {
        Err(e) => assert!(e.error_description.unwrap().contains("At least one")),
        Ok(_) => panic!("Expected error"),
    }
}

#[test]
fn test_external_http_rejected() {
    let result = validate_redirect_uris(&["http://example.com/callback".to_string()]);
    assert!(result.is_err());
}

#[test]
fn test_external_https_rejected() {
    let result = validate_redirect_uris(&["https://example.com/callback".to_string()]);
    assert!(result.is_err());
}

#[test]
fn test_mixed_valid_invalid_rejected() {
    // One invalid URI should fail the whole validation
    let uris = vec![
        "http://127.0.0.1:8080/callback".to_string(),
        "https://evil.com/steal".to_string(), // invalid
    ];
    assert!(validate_redirect_uris(&uris).is_err());
}

// =============================================================================
// DcrError Tests
// =============================================================================

#[test]
fn test_dcr_error_invalid_redirect_uri() {
    let error = DcrError::invalid_redirect_uri("Bad URI format");
    assert_eq!(error.error, "invalid_redirect_uri");
    assert_eq!(error.error_description, Some("Bad URI format".to_string()));
}

#[test]
fn test_dcr_error_invalid_client_metadata() {
    let error = DcrError::invalid_client_metadata("Missing required field");
    assert_eq!(error.error, "invalid_client_metadata");
    assert_eq!(
        error.error_description,
        Some("Missing required field".to_string())
    );
}

// =============================================================================
// DcrRequest Tests
// =============================================================================

fn minimal_dcr_request() -> DcrRequest {
    DcrRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://127.0.0.1:8080/callback".to_string()],
        grant_types: vec![],
        response_types: vec![],
        token_endpoint_auth_method: None,
        scope: None,
        logo_uri: None,
        client_uri: None,
        tos_uri: None,
        policy_uri: None,
        contacts: None,
        software_id: None,
        software_version: None,
    }
}

// =============================================================================
// DcrRequest Structure Tests
// =============================================================================

#[test]
fn test_dcr_request_with_minimal_fields() {
    let request = minimal_dcr_request();

    assert_eq!(request.client_name, "Test Client");
    assert_eq!(request.redirect_uris.len(), 1);
    assert!(request.grant_types.is_empty()); // defaults applied by process_dcr_request
    assert!(request.response_types.is_empty());
    assert!(request.scope.is_none());
}

#[test]
fn test_dcr_request_with_full_fields() {
    let request = DcrRequest {
        client_name: "Cursor IDE".to_string(),
        redirect_uris: vec![
            "cursor://callback".to_string(),
            "http://127.0.0.1:8080/callback".to_string(),
        ],
        grant_types: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        response_types: vec!["code".to_string()],
        token_endpoint_auth_method: Some("none".to_string()),
        scope: Some("openid mcp:read mcp:write".to_string()),
        logo_uri: Some("https://cursor.com/logo.png".to_string()),
        client_uri: Some("https://cursor.com".to_string()),
        tos_uri: Some("https://cursor.com/tos".to_string()),
        policy_uri: Some("https://cursor.com/privacy".to_string()),
        contacts: Some(vec!["support@cursor.com".to_string()]),
        software_id: Some("com.cursor.app".to_string()),
        software_version: Some("1.0.0".to_string()),
    };

    assert_eq!(request.client_name, "Cursor IDE");
    assert_eq!(request.redirect_uris.len(), 2);
    assert_eq!(request.grant_types.len(), 2);
    assert_eq!(request.scope, Some("openid mcp:read mcp:write".to_string()));
    assert_eq!(request.software_id, Some("com.cursor.app".to_string()));
}

// Note: Full DCR integration tests with database are in tests/database/dcr.rs
