//! OAuth Flow integration tests with mock HTTP server

use mcpmux_gateway::oauth::{
    AuthorizationCallback, OAuthConfig, OAuthFlow, OAuthManager, OAuthMetadata,
};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_metadata(server_url: &str) -> OAuthMetadata {
    OAuthMetadata {
        issuer: server_url.to_string(),
        authorization_endpoint: format!("{}/authorize", server_url),
        token_endpoint: format!("{}/token", server_url),
        userinfo_endpoint: None,
        revocation_endpoint: None,
        registration_endpoint: None,
        jwks_uri: None,
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        scopes_supported: vec!["openid".to_string(), "profile".to_string()],
        code_challenge_methods_supported: vec!["S256".to_string()],
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
    }
}

// =============================================================================
// OAuthFlow Tests
// =============================================================================

#[test]
fn test_authorization_request_includes_required_params() {
    let metadata = test_metadata("https://auth.example.com");
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);

    let request = flow
        .create_authorization_request("http://localhost:8080/callback", &["openid".to_string()])
        .unwrap();

    assert!(request.authorization_url.contains("response_type=code"));
    assert!(request.authorization_url.contains("client_id=client_123"));
    assert!(request
        .authorization_url
        .contains("redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fcallback"));
    assert!(request.authorization_url.contains("scope=openid"));
}

#[test]
fn test_authorization_request_includes_pkce() {
    let metadata = test_metadata("https://auth.example.com");
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);

    let request = flow
        .create_authorization_request("http://localhost:8080/callback", &["openid".to_string()])
        .unwrap();

    assert!(request.authorization_url.contains("code_challenge="));
    assert!(request
        .authorization_url
        .contains("code_challenge_method=S256"));
    assert!(!request.pkce_verifier.is_empty());
}

#[test]
fn test_authorization_request_state_is_unique() {
    let metadata = test_metadata("https://auth.example.com");
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);

    let request1 = flow
        .create_authorization_request("http://localhost:8080/callback", &["openid".to_string()])
        .unwrap();
    let request2 = flow
        .create_authorization_request("http://localhost:8080/callback", &["openid".to_string()])
        .unwrap();

    assert_ne!(request1.state, request2.state);
    assert_ne!(request1.pkce_verifier, request2.pkce_verifier);
}

#[test]
fn test_authorization_request_multiple_scopes() {
    let metadata = test_metadata("https://auth.example.com");
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);

    let request = flow
        .create_authorization_request(
            "http://localhost:8080/callback",
            &[
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
        )
        .unwrap();

    // Scopes are space-separated and URL encoded
    assert!(
        request
            .authorization_url
            .contains("scope=openid+profile+email")
            || request
                .authorization_url
                .contains("scope=openid%20profile%20email")
    );
}

#[tokio::test]
async fn test_exchange_code_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=auth_code_123"))
        .and(body_string_contains("code_verifier=verifier_abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access_token_xyz",
            "token_type": "Bearer",
            "refresh_token": "refresh_token_abc",
            "expires_in": 3600,
            "scope": "openid profile"
        })))
        .mount(&mock_server)
        .await;

    let metadata = test_metadata(&mock_server.uri());
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);
    let http_client = reqwest::Client::new();

    let token = flow
        .exchange_code(
            &http_client,
            "auth_code_123",
            "http://localhost:8080/callback",
            "verifier_abc",
        )
        .await
        .unwrap();

    assert_eq!(token.access_token, "access_token_xyz");
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.refresh_token, Some("refresh_token_abc".to_string()));
    assert!(token.expires_at.is_some());
}

#[tokio::test]
async fn test_exchange_code_with_client_secret() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("client_secret=secret_123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "token",
            "token_type": "Bearer"
        })))
        .mount(&mock_server)
        .await;

    let metadata = test_metadata(&mock_server.uri());
    let flow = OAuthFlow::new(
        metadata,
        "client_123".to_string(),
        Some("secret_123".to_string()),
    );
    let http_client = reqwest::Client::new();

    let result = flow
        .exchange_code(&http_client, "code", "http://localhost/cb", "verifier")
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_exchange_code_error_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_grant",
            "error_description": "Authorization code expired"
        })))
        .mount(&mock_server)
        .await;

    let metadata = test_metadata(&mock_server.uri());
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);
    let http_client = reqwest::Client::new();

    let result = flow
        .exchange_code(
            &http_client,
            "expired_code",
            "http://localhost/cb",
            "verifier",
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("400") || err.contains("invalid_grant"));
}

#[tokio::test]
async fn test_refresh_token_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=old_refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new_access_token",
            "token_type": "Bearer",
            "refresh_token": "new_refresh_token",
            "expires_in": 7200
        })))
        .mount(&mock_server)
        .await;

    let metadata = test_metadata(&mock_server.uri());
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);
    let http_client = reqwest::Client::new();

    let token = flow
        .refresh_token(&http_client, "old_refresh")
        .await
        .unwrap();

    assert_eq!(token.access_token, "new_access_token");
    assert_eq!(token.refresh_token, Some("new_refresh_token".to_string()));
}

#[tokio::test]
async fn test_refresh_token_without_new_refresh() {
    let mock_server = MockServer::start().await;

    // Some OAuth servers don't return a new refresh token
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new_access",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .mount(&mock_server)
        .await;

    let metadata = test_metadata(&mock_server.uri());
    let flow = OAuthFlow::new(metadata, "client_123".to_string(), None);
    let http_client = reqwest::Client::new();

    let token = flow
        .refresh_token(&http_client, "old_refresh")
        .await
        .unwrap();

    assert_eq!(token.access_token, "new_access");
    assert!(token.refresh_token.is_none());
}

// =============================================================================
// AuthorizationCallback Tests
// =============================================================================

#[test]
fn test_callback_success() {
    let callback = AuthorizationCallback {
        code: "auth_code_123".to_string(),
        state: "state_abc".to_string(),
        error: None,
        error_description: None,
    };

    assert!(!callback.is_error());
    assert!(callback.error_message().is_none());
}

#[test]
fn test_callback_error() {
    let callback = AuthorizationCallback {
        code: "".to_string(),
        state: "state_abc".to_string(),
        error: Some("access_denied".to_string()),
        error_description: Some("User denied the request".to_string()),
    };

    assert!(callback.is_error());
    assert_eq!(
        callback.error_message(),
        Some("access_denied: User denied the request".to_string())
    );
}

#[test]
fn test_callback_error_without_description() {
    let callback = AuthorizationCallback {
        code: "".to_string(),
        state: "state_abc".to_string(),
        error: Some("server_error".to_string()),
        error_description: None,
    };

    assert!(callback.is_error());
    assert_eq!(callback.error_message(), Some("server_error".to_string()));
}

// =============================================================================
// OAuthManager Tests
// =============================================================================

#[tokio::test]
async fn test_oauth_manager_discovery() {
    let mock_server = MockServer::start().await;

    // Setup OIDC discovery endpoint
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": mock_server.uri(),
            "authorization_endpoint": format!("{}/authorize", mock_server.uri()),
            "token_endpoint": format!("{}/token", mock_server.uri()),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code"],
            "code_challenge_methods_supported": ["S256"]
        })))
        .mount(&mock_server)
        .await;

    let config = OAuthConfig::new(&mock_server.uri()).with_client("client_id".to_string(), None);

    let mut manager = OAuthManager::new(config);
    let metadata = manager.discover().await.unwrap();

    assert_eq!(metadata.issuer, mock_server.uri());
    assert!(metadata.supports_pkce());
}

#[tokio::test]
async fn test_oauth_manager_fallback_to_oauth_as() {
    let mock_server = MockServer::start().await;

    // OIDC discovery fails
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // OAuth AS discovery succeeds
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": mock_server.uri(),
            "authorization_endpoint": format!("{}/auth", mock_server.uri()),
            "token_endpoint": format!("{}/token", mock_server.uri())
        })))
        .mount(&mock_server)
        .await;

    let config = OAuthConfig::new(&mock_server.uri()).with_client("client_id".to_string(), None);

    let mut manager = OAuthManager::new(config);
    let metadata = manager.discover().await.unwrap();

    assert_eq!(metadata.issuer, mock_server.uri());
}

#[tokio::test]
async fn test_oauth_manager_start_authorization() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": mock_server.uri(),
            "authorization_endpoint": format!("{}/authorize", mock_server.uri()),
            "token_endpoint": format!("{}/token", mock_server.uri())
        })))
        .mount(&mock_server)
        .await;

    let config = OAuthConfig::new(&mock_server.uri())
        .with_client("my_client".to_string(), None)
        .with_scopes(vec!["openid".to_string(), "mcp".to_string()]);

    let mut manager = OAuthManager::new(config);
    let auth_request = manager
        .start_authorization("http://localhost:8080/callback")
        .await
        .unwrap();

    assert!(auth_request
        .authorization_url
        .starts_with(&format!("{}/authorize", mock_server.uri())));
    assert!(auth_request
        .authorization_url
        .contains("client_id=my_client"));
    assert!(!auth_request.state.is_empty());
    assert!(!auth_request.pkce_verifier.is_empty());
}

#[tokio::test]
async fn test_oauth_manager_requires_client_id() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": mock_server.uri(),
            "authorization_endpoint": format!("{}/authorize", mock_server.uri()),
            "token_endpoint": format!("{}/token", mock_server.uri())
        })))
        .mount(&mock_server)
        .await;

    // No client_id configured
    let config = OAuthConfig::new(&mock_server.uri());

    let mut manager = OAuthManager::new(config);
    let result = manager
        .start_authorization("http://localhost:8080/callback")
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Client ID"));
}

#[tokio::test]
async fn test_oauth_manager_full_flow() {
    let mock_server = MockServer::start().await;

    // Discovery
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": mock_server.uri(),
            "authorization_endpoint": format!("{}/authorize", mock_server.uri()),
            "token_endpoint": format!("{}/token", mock_server.uri())
        })))
        .mount(&mock_server)
        .await;

    // Token exchange
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "final_token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .mount(&mock_server)
        .await;

    let config = OAuthConfig::new(&mock_server.uri())
        .with_client("test_client".to_string(), None)
        .with_scopes(vec!["mcp".to_string()]);

    let mut manager = OAuthManager::new(config);

    // Step 1: Start authorization
    let auth_request = manager
        .start_authorization("http://localhost/cb")
        .await
        .unwrap();

    // Step 2: Exchange code (simulating callback)
    let token = manager
        .exchange_code(
            "auth_code",
            "http://localhost/cb",
            &auth_request.pkce_verifier,
        )
        .await
        .unwrap();

    assert_eq!(token.access_token, "final_token");
}
