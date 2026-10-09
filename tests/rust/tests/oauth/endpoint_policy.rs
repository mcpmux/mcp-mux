//! Outbound OAuth must only send the browser and credentials where the
//! server's sign-in established: https endpoints (loopback http for local
//! development), and on refresh the token endpoint saved at sign-in.

use std::sync::Arc;

use chrono::Utc;
use mcpmux_core::{Credential, OutboundOAuthRegistration, StoredOAuthMetadata};
use mcpmux_gateway::OutboundOAuthManager;
use tests::mocks::{MockCredentialRepository, MockOutboundOAuthRepository};
use uuid::Uuid;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Serve protected-resource and authorization-server metadata from `server`,
/// using the given endpoints.
async fn mount_metadata(server: &MockServer, authorization_endpoint: &str, token_endpoint: &str) {
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource/mcp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": format!("{base}/mcp"),
            "authorization_servers": [base],
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": base,
            "authorization_endpoint": authorization_endpoint,
            "token_endpoint": token_endpoint,
            "registration_endpoint": format!("{base}/register"),
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"],
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn non_https_authorization_endpoint_is_refused() {
    let server = MockServer::start().await;
    mount_metadata(
        &server,
        "file:///etc/passwd",
        &format!("{}/token", server.uri()),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "client_id": "dcr-client",
            "redirect_uris": [],
        })))
        .expect(0)
        .mount(&server)
        .await;

    let result = OutboundOAuthManager::new()
        .start_oauth_flow(
            Arc::new(MockCredentialRepository::new()),
            Arc::new(MockOutboundOAuthRepository::new()),
            Uuid::new_v4(),
            "file-endpoint",
            &format!("{}/mcp", server.uri()),
        )
        .await;

    assert!(
        result.is_err(),
        "a file: authorization endpoint must never reach the browser: {result:?}"
    );
}

#[tokio::test]
async fn refresh_uses_the_token_endpoint_saved_at_sign_in() {
    // The server's metadata now names another token endpoint (`moved`); the
    // refresh token must still only go to the one saved at sign-in (`signed_in`).
    let signed_in = MockServer::start().await;
    let moved = MockServer::start().await;
    mount_metadata(
        &moved,
        &format!("{}/authorize", moved.uri()),
        &format!("{}/token", moved.uri()),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "from-the-wrong-place",
            "token_type": "Bearer",
            "expires_in": 3600,
        })))
        .expect(0)
        .mount(&moved)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=stored-refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "refreshed-access",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "rotated-refresh",
        })))
        .expect(1)
        .mount(&signed_in)
        .await;

    let space_id = Uuid::new_v4();
    let server_id = "pinned-metadata";
    let server_url = format!("{}/mcp", moved.uri());
    let metadata = StoredOAuthMetadata {
        authorization_endpoint: format!("{}/authorize", signed_in.uri()),
        token_endpoint: format!("{}/token", signed_in.uri()),
        registration_endpoint: None,
        issuer: Some(signed_in.uri()),
        jwks_uri: None,
        scopes_supported: None,
        response_types_supported: Some(vec!["code".to_string()]),
        additional_fields: Default::default(),
    };
    let registration = OutboundOAuthRegistration::with_metadata(
        space_id,
        server_id,
        server_url.clone(),
        "client-1",
        "http://127.0.0.1:45819/oauth2redirect",
        metadata,
    );
    // An access token about to expire, so getting a token refreshes it.
    let credentials = MockCredentialRepository::new()
        .with_credential(Credential::access_token(
            space_id,
            server_id,
            "old-access",
            Some(Utc::now() + chrono::Duration::seconds(5)),
        ))
        .with_credential(Credential::refresh_token(
            space_id,
            server_id,
            "stored-refresh",
            None,
        ));

    let token = OutboundOAuthManager::new()
        .get_access_token(
            Arc::new(credentials),
            Arc::new(MockOutboundOAuthRepository::new().with_registration(registration)),
            space_id,
            server_id,
            &server_url,
        )
        .await
        .expect("refresh succeeds against the saved token endpoint");
    assert_eq!(token, "refreshed-access");
    // Dropping the mock servers verifies `moved` got no token request.
}

/// Credentials for `server_id` with an access token about to expire, so
/// getting a token refreshes it.
fn expiring_credentials(space_id: Uuid, server_id: &str) -> MockCredentialRepository {
    MockCredentialRepository::new()
        .with_credential(Credential::access_token(
            space_id,
            server_id,
            "old-access",
            Some(Utc::now() + chrono::Duration::seconds(5)),
        ))
        .with_credential(Credential::refresh_token(
            space_id,
            server_id,
            "stored-refresh",
            None,
        ))
}

#[tokio::test]
async fn refresh_without_saved_metadata_checks_what_it_discovers() {
    // A registration saved before metadata was stored: the refresh discovers
    // it, through the same endpoint check as sign-in.
    let server = MockServer::start().await;
    mount_metadata(
        &server,
        &format!("{}/authorize", server.uri()),
        &format!("{}/token", server.uri()),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("refresh_token=stored-refresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "refreshed-access",
            "token_type": "Bearer",
            "expires_in": 3600,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let space_id = Uuid::new_v4();
    let server_id = "no-saved-metadata";
    let server_url = format!("{}/mcp", server.uri());
    let registration = OutboundOAuthRegistration::new(
        space_id,
        server_id,
        server_url.clone(),
        "client-1",
        "http://127.0.0.1:45819/oauth2redirect",
    );
    let token = OutboundOAuthManager::new()
        .get_access_token(
            Arc::new(expiring_credentials(space_id, server_id)),
            Arc::new(MockOutboundOAuthRepository::new().with_registration(registration)),
            space_id,
            server_id,
            &server_url,
        )
        .await
        .expect("refresh succeeds against a checked, discovered token endpoint");
    assert_eq!(token, "refreshed-access");
}

#[tokio::test]
async fn refresh_refuses_a_discovered_plain_http_token_endpoint() {
    // No saved metadata, and the server's metadata names a plain-http token
    // endpoint on another host: the refresh token is never sent.
    let server = MockServer::start().await;
    mount_metadata(
        &server,
        &format!("{}/authorize", server.uri()),
        "http://192.0.2.10/token",
    )
    .await;

    let space_id = Uuid::new_v4();
    let server_id = "discovered-http";
    let server_url = format!("{}/mcp", server.uri());
    let registration = OutboundOAuthRegistration::new(
        space_id,
        server_id,
        server_url.clone(),
        "client-1",
        "http://127.0.0.1:45819/oauth2redirect",
    );
    let started = std::time::Instant::now();
    let result = OutboundOAuthManager::new()
        .get_access_token(
            Arc::new(expiring_credentials(space_id, server_id)),
            Arc::new(MockOutboundOAuthRepository::new().with_registration(registration)),
            space_id,
            server_id,
            &server_url,
        )
        .await;
    assert!(result.is_err(), "sign-in required: {result:?}");
    // Refused before any refresh: an attempt to reach the TEST-NET token
    // endpoint would hang until the HTTP timeout (30 s) instead.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "a refresh was attempted against the refused endpoint"
    );
}

#[tokio::test]
async fn saved_metadata_with_a_plain_http_token_endpoint_requires_sign_in() {
    let server = MockServer::start().await;
    let space_id = Uuid::new_v4();
    let server_id = "saved-http";
    let server_url = format!("{}/mcp", server.uri());
    let metadata = StoredOAuthMetadata {
        authorization_endpoint: format!("{}/authorize", server.uri()),
        token_endpoint: "http://192.0.2.10/token".to_string(),
        registration_endpoint: None,
        issuer: None,
        jwks_uri: None,
        scopes_supported: None,
        response_types_supported: None,
        additional_fields: Default::default(),
    };
    let registration = OutboundOAuthRegistration::with_metadata(
        space_id,
        server_id,
        server_url.clone(),
        "client-1",
        "http://127.0.0.1:45819/oauth2redirect",
        metadata,
    );
    let started = std::time::Instant::now();
    let result = OutboundOAuthManager::new()
        .get_access_token(
            Arc::new(expiring_credentials(space_id, server_id)),
            Arc::new(MockOutboundOAuthRepository::new().with_registration(registration)),
            space_id,
            server_id,
            &server_url,
        )
        .await;
    assert!(result.is_err(), "sign-in required: {result:?}");
    // Refused before any refresh: an attempt to reach the TEST-NET token
    // endpoint would hang until the HTTP timeout (30 s) instead.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "a refresh was attempted against the refused endpoint"
    );
}
