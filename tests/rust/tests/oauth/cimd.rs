//! Outbound OAuth client identification tests (issue #224)
//!
//! When a remote authorization server advertises
//! `client_id_metadata_document_supported: true`, McpMux must identify itself with its
//! Client ID Metadata Document URL instead of Dynamic Client Registration. Servers that
//! don't advertise CIMD keep using DCR.

use std::sync::Arc;

use mcpmux_core::branding;
use mcpmux_gateway::{OAuthInitResult, OutboundOAuthManager};
use tests::mocks::{MockCredentialRepository, MockOutboundOAuthRepository};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Mount RFC 9728 protected-resource metadata and RFC 8414 authorization-server metadata
/// on `mock_server`, which acts as both the MCP server and its authorization server.
async fn mount_oauth_metadata(
    mock_server: &MockServer,
    supports_cimd: bool,
    with_registration_endpoint: bool,
) {
    let base = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource/mcp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": format!("{}/mcp", base),
            "authorization_servers": [base],
        })))
        .mount(mock_server)
        .await;

    let mut metadata = serde_json::json!({
        "issuer": base,
        "authorization_endpoint": format!("{}/authorize", base),
        "token_endpoint": format!("{}/token", base),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
    });
    if supports_cimd {
        metadata["client_id_metadata_document_supported"] = serde_json::json!(true);
    }
    if with_registration_endpoint {
        metadata["registration_endpoint"] = serde_json::json!(format!("{}/register", base));
    }

    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
        .mount(mock_server)
        .await;
}

/// Start an outbound OAuth flow against `{mock_server}/mcp` and return the authorization URL.
async fn start_flow(mock_server: &MockServer) -> url::Url {
    let manager = OutboundOAuthManager::new();
    let result = manager
        .start_oauth_flow(
            Arc::new(MockCredentialRepository::new()),
            Arc::new(MockOutboundOAuthRepository::new()),
            Uuid::new_v4(),
            "test-server",
            &format!("{}/mcp", mock_server.uri()),
        )
        .await
        .expect("start_oauth_flow should succeed");

    match result {
        OAuthInitResult::Initiated { auth_url } => {
            url::Url::parse(&auth_url).expect("authorization URL should parse")
        }
        other => panic!("expected OAuthInitResult::Initiated, got {:?}", other),
    }
}

fn query_param(url: &url::Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

#[tokio::test]
async fn cimd_used_when_server_has_no_registration_endpoint() {
    // Exact-client servers (e.g. CareClinic) advertise CIMD and disable open DCR
    let mock_server = MockServer::start().await;
    mount_oauth_metadata(&mock_server, true, false).await;

    let auth_url = start_flow(&mock_server).await;

    assert_eq!(
        query_param(&auth_url, "client_id").as_deref(),
        Some(branding::outbound_oauth_client_metadata_url().as_str())
    );
    let redirect_uri = query_param(&auth_url, "redirect_uri").expect("redirect_uri param");
    assert!(
        redirect_uri.starts_with("http://127.0.0.1:") && redirect_uri.ends_with("/oauth2redirect"),
        "unexpected redirect_uri: {redirect_uri}"
    );
    assert!(query_param(&auth_url, "code_challenge").is_some());
}

#[tokio::test]
async fn cimd_preferred_over_dcr_when_both_supported() {
    let mock_server = MockServer::start().await;
    mount_oauth_metadata(&mock_server, true, true).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "client_id": "dcr-client-id",
            "redirect_uris": [],
        })))
        .expect(0)
        .mount(&mock_server)
        .await;

    let auth_url = start_flow(&mock_server).await;

    assert_eq!(
        query_param(&auth_url, "client_id").as_deref(),
        Some(branding::outbound_oauth_client_metadata_url().as_str())
    );
}

#[tokio::test]
async fn dcr_used_when_server_does_not_advertise_cimd() {
    let mock_server = MockServer::start().await;
    mount_oauth_metadata(&mock_server, false, true).await;

    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "client_id": "dcr-client-id",
            "redirect_uris": [],
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let auth_url = start_flow(&mock_server).await;

    assert_eq!(
        query_param(&auth_url, "client_id").as_deref(),
        Some("dcr-client-id")
    );
}
