//! OAuth Token management tests

use chrono::{Duration, Utc};
use mcpmux_gateway::oauth::{OAuthToken, TokenManager};

// =============================================================================
// OAuthToken Tests
// =============================================================================

fn create_token(expires_in_secs: i64, has_refresh: bool) -> OAuthToken {
    OAuthToken {
        access_token: "test_access_token".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: if has_refresh {
            Some("test_refresh_token".to_string())
        } else {
            None
        },
        expires_at: if expires_in_secs == 0 {
            None
        } else {
            Some(Utc::now() + Duration::seconds(expires_in_secs))
        },
        scope: Some("openid profile".to_string()),
        id_token: None,
    }
}

#[test]
fn test_token_not_expired() {
    let token = create_token(3600, false); // expires in 1 hour
    assert!(!token.is_expired());
}

#[test]
fn test_token_expired() {
    let token = OAuthToken {
        access_token: "old".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: Some(Utc::now() - Duration::hours(1)), // expired 1 hour ago
        scope: None,
        id_token: None,
    };
    assert!(token.is_expired());
}

#[test]
fn test_token_no_expiry_never_expires() {
    let token = create_token(0, false); // no expiry set
    assert!(!token.is_expired());
}

#[test]
fn test_token_expires_soon() {
    let token = create_token(60, false); // expires in 60 seconds

    // With 30 second buffer, not expiring soon
    assert!(!token.expires_soon(30));

    // With 120 second buffer, expiring soon
    assert!(token.expires_soon(120));
}

#[test]
fn test_token_no_expiry_never_expires_soon() {
    let token = create_token(0, false);
    assert!(!token.expires_soon(86400)); // even with 1 day buffer
}

#[test]
fn test_token_can_refresh_with_refresh_token() {
    let token = create_token(3600, true);
    assert!(token.can_refresh());
}

#[test]
fn test_token_cannot_refresh_without_refresh_token() {
    let token = create_token(3600, false);
    assert!(!token.can_refresh());
}

#[test]
fn test_authorization_header_bearer() {
    let token = OAuthToken {
        access_token: "abc123".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: None,
        id_token: None,
    };

    assert_eq!(token.authorization_header(), "Bearer abc123");
}

#[test]
fn test_authorization_header_custom_type() {
    let token = OAuthToken {
        access_token: "xyz789".to_string(),
        token_type: "MAC".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: None,
        id_token: None,
    };

    assert_eq!(token.authorization_header(), "MAC xyz789");
}

#[test]
fn test_scopes_parsing() {
    let token = OAuthToken {
        access_token: "token".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: Some("openid profile email offline_access".to_string()),
        id_token: None,
    };

    let scopes = token.scopes();
    assert_eq!(scopes.len(), 4);
    assert!(scopes.contains(&"openid".to_string()));
    assert!(scopes.contains(&"profile".to_string()));
    assert!(scopes.contains(&"email".to_string()));
    assert!(scopes.contains(&"offline_access".to_string()));
}

#[test]
fn test_scopes_empty_when_none() {
    let token = OAuthToken {
        access_token: "token".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: None,
        id_token: None,
    };

    assert!(token.scopes().is_empty());
}

#[test]
fn test_scopes_empty_string() {
    let token = OAuthToken {
        access_token: "token".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: None,
        scope: Some("".to_string()),
        id_token: None,
    };

    // Empty string split returns empty vec
    assert!(token.scopes().is_empty());
}

// =============================================================================
// TokenManager Tests
// =============================================================================

#[test]
fn test_token_manager_default() {
    let manager = TokenManager::new();
    let token = create_token(3600, true);

    // Token valid for 1 hour, default buffer is 5 minutes
    assert!(!manager.needs_refresh(&token));
    assert!(manager.is_usable(&token));
}

#[test]
fn test_token_manager_needs_refresh() {
    let manager = TokenManager::new(); // 5 minute buffer

    // Token expires in 4 minutes - should need refresh
    let token = create_token(240, true);
    assert!(manager.needs_refresh(&token));
}

#[test]
fn test_token_manager_no_refresh_without_refresh_token() {
    let manager = TokenManager::new();

    // Token expiring soon but has no refresh token
    let token = create_token(60, false);
    assert!(!manager.needs_refresh(&token)); // can_refresh() is false
}

#[test]
fn test_token_manager_custom_buffer() {
    let manager = TokenManager::new().with_refresh_buffer(600); // 10 minute buffer

    // Token expires in 8 minutes - with 10 min buffer, needs refresh
    let token = create_token(480, true);
    assert!(manager.needs_refresh(&token));

    // Token expires in 15 minutes - doesn't need refresh yet
    let token2 = create_token(900, true);
    assert!(!manager.needs_refresh(&token2));
}

#[test]
fn test_token_manager_usable_check() {
    let manager = TokenManager::new();

    // Valid token
    let valid_token = create_token(3600, false);
    assert!(manager.is_usable(&valid_token));

    // Expired token
    let expired_token = OAuthToken {
        access_token: "old".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: Some(Utc::now() - Duration::minutes(5)),
        scope: None,
        id_token: None,
    };
    assert!(!manager.is_usable(&expired_token));
}

#[test]
fn test_token_manager_no_expiry_always_usable() {
    let manager = TokenManager::new();
    let token = create_token(0, false); // no expiry

    assert!(manager.is_usable(&token));
    assert!(!manager.needs_refresh(&token)); // no refresh token anyway
}

#[test]
fn test_token_manager_zero_buffer() {
    let manager = TokenManager::new().with_refresh_buffer(0);

    // Token expires in 1 second
    let token = create_token(1, true);

    // With 0 buffer, only needs refresh when actually expired
    assert!(!manager.needs_refresh(&token));
}

// =============================================================================
// Token Lifecycle Tests
// =============================================================================

#[test]
fn test_token_lifecycle_fresh() {
    let manager = TokenManager::new();
    let token = create_token(7200, true); // 2 hours

    assert!(manager.is_usable(&token));
    assert!(!manager.needs_refresh(&token));
    assert!(token.can_refresh());
}

#[test]
fn test_token_lifecycle_approaching_expiry() {
    let manager = TokenManager::new();
    let token = create_token(200, true); // ~3 minutes

    assert!(manager.is_usable(&token)); // still valid
    assert!(manager.needs_refresh(&token)); // but should refresh soon
}

#[test]
fn test_token_lifecycle_expired_but_refreshable() {
    let manager = TokenManager::new();
    let token = OAuthToken {
        access_token: "expired".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: Some("still_valid_refresh".to_string()),
        expires_at: Some(Utc::now() - Duration::minutes(1)),
        scope: None,
        id_token: None,
    };

    assert!(!manager.is_usable(&token)); // expired
    assert!(token.can_refresh()); // but can get new token
}

#[test]
fn test_token_lifecycle_expired_not_refreshable() {
    let manager = TokenManager::new();
    let token = OAuthToken {
        access_token: "dead".to_string(),
        token_type: "Bearer".to_string(),
        refresh_token: None,
        expires_at: Some(Utc::now() - Duration::hours(1)),
        scope: None,
        id_token: None,
    };

    assert!(!manager.is_usable(&token));
    assert!(!token.can_refresh());
    // Need to re-authenticate
}
