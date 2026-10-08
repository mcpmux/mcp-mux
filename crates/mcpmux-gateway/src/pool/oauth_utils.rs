//! Shared OAuth utilities for metadata discovery with origin URL fallback.
//!
//! Some OAuth servers (like Atlassian) serve their metadata at the origin URL
//! (e.g., `https://mcp.atlassian.com`) rather than the endpoint path
//! (e.g., `https://mcp.atlassian.com/v1/sse`). This module provides utilities
//! to handle both cases.

use mcpmux_core::{OutboundOAuthRegistration, StoredOAuthMetadata};
use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationMetadata, OAuthClientConfig,
};
use tracing::{info, warn};
use url::Url;

use super::oauth_registration::RegisteredClient;

/// Extract the origin (scheme + host + port) from a URL.
///
/// # Example
/// ```ignore
/// extract_origin("https://mcp.atlassian.com/v1/sse") // -> Some("https://mcp.atlassian.com")
/// extract_origin("http://localhost:8080/api") // -> Some("http://localhost:8080")
/// ```
pub fn extract_origin(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let mut origin = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        origin = format!("{}:{}", origin, port);
    }
    Some(origin)
}

/// Discover OAuth metadata with fallback to origin URL.
///
/// This tries to discover metadata at the server URL first. If that fails with
/// `NoAuthorizationSupport`, it extracts the origin and tries there.
///
/// Returns the discovered metadata if successful, or an error if both attempts fail.
pub async fn discover_metadata_with_fallback(
    manager: &mut AuthorizationManager,
    server_url: &str,
) -> Result<AuthorizationMetadata, AuthError> {
    // First try the direct URL
    match manager.discover_metadata().await {
        Ok(metadata) => {
            info!("[OAuth] Metadata discovered at endpoint: {}", server_url);
            Ok(metadata)
        }
        Err(AuthError::NoAuthorizationSupport) => {
            // Try origin URL as fallback
            let origin_url = extract_origin(server_url).ok_or(AuthError::NoAuthorizationSupport)?;

            info!(
                "[OAuth] Metadata not at endpoint, trying origin: {}",
                origin_url
            );

            let origin_manager = AuthorizationManager::new(&origin_url)
                .await
                .map_err(|_| AuthError::NoAuthorizationSupport)?;

            let metadata = origin_manager.discover_metadata().await?;

            info!("[OAuth] Metadata discovered at origin: {}", origin_url);

            Ok(metadata)
        }
        Err(e) => Err(e),
    }
}

/// Discover metadata and return both the RMCP metadata (for setting on manager)
/// and our stored format (for persistence).
///
/// Use this when you need to both configure RMCP and save metadata for future reconnects.
pub async fn discover_and_convert_metadata(
    manager: &mut AuthorizationManager,
    server_url: &str,
) -> Result<(AuthorizationMetadata, StoredOAuthMetadata), AuthError> {
    let metadata = discover_metadata_with_fallback(manager, server_url).await?;
    check_discovered_endpoints(&metadata)?;
    let stored = convert_to_stored_metadata(&metadata);
    Ok((metadata, stored))
}

/// Whether McpMux may send a browser or credentials to this OAuth endpoint:
/// `https`, or `http` on a loopback host (local development servers).
///
/// The endpoints come from metadata the MCP server (or its authorization
/// server) controls; anything else (`file:`, `smb:`, OS protocol handlers,
/// plain http on the network) is refused before it reaches the browser or a
/// token request.
pub fn is_acceptable_oauth_endpoint(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            None => false,
        },
        _ => false,
    }
}

fn check_discovered_endpoints(metadata: &AuthorizationMetadata) -> Result<(), AuthError> {
    let endpoints = [
        (
            "authorization_endpoint",
            Some(&metadata.authorization_endpoint),
        ),
        ("token_endpoint", Some(&metadata.token_endpoint)),
        (
            "registration_endpoint",
            metadata.registration_endpoint.as_ref(),
        ),
    ];
    for (name, url) in endpoints {
        if let Some(url) = url {
            if !is_acceptable_oauth_endpoint(url) {
                warn!("[OAuth] Refusing server metadata: {name} is not an https URL");
                return Err(AuthError::MetadataError(format!(
                    "{name} must be an https URL (or http on localhost)"
                )));
            }
        }
    }
    Ok(())
}

/// Convert RMCP's AuthorizationMetadata to our StoredOAuthMetadata format.
///
/// This allows us to persist discovered metadata and later use it to bypass
/// RMCP's metadata discovery (which can fail on non-spec-compliant servers).
pub fn convert_to_stored_metadata(metadata: &AuthorizationMetadata) -> StoredOAuthMetadata {
    StoredOAuthMetadata {
        authorization_endpoint: metadata.authorization_endpoint.clone(),
        token_endpoint: metadata.token_endpoint.clone(),
        registration_endpoint: metadata.registration_endpoint.clone(),
        issuer: metadata.issuer.clone(),
        jwks_uri: metadata.jwks_uri.clone(),
        scopes_supported: metadata.scopes_supported.clone(),
        response_types_supported: metadata.response_types_supported.clone(),
        additional_fields: metadata.additional_fields.clone(),
    }
}

/// Convert our StoredOAuthMetadata back to RMCP's AuthorizationMetadata format.
///
/// This is used when loading saved metadata and setting it on the RMCP manager
/// to bypass discovery.
pub fn convert_from_stored_metadata(stored: &StoredOAuthMetadata) -> AuthorizationMetadata {
    let mut metadata = AuthorizationMetadata::default();
    metadata.authorization_endpoint = stored.authorization_endpoint.clone();
    metadata.token_endpoint = stored.token_endpoint.clone();
    metadata.registration_endpoint = stored.registration_endpoint.clone();
    metadata.issuer = stored.issuer.clone();
    metadata.jwks_uri = stored.jwks_uri.clone();
    metadata.scopes_supported = stored.scopes_supported.clone();
    metadata.response_types_supported = stored.response_types_supported.clone();
    metadata.additional_fields = stored.additional_fields.clone();
    metadata
}

/// The token endpoint auth methods rmcp can send a client secret with
const SECRET_AUTH_METHODS: [&str; 2] = ["client_secret_basic", "client_secret_post"];

/// Configure `manager` with a client McpMux registered, including its secret.
///
/// rmcp picks how to send the secret from the server's advertised
/// `token_endpoint_auth_methods_supported`, not from the method the server
/// registered the client with: HTTP Basic unless only `client_secret_post` is
/// listed. A server that advertises both but enforces the registered method would
/// reject a `client_secret_post` client with `invalid_client`. So when the client
/// has a secret and the server named its method, rmcp is configured with
/// `metadata` advertising only that method. The narrowed list stays in memory;
/// what's stored is the server's own metadata.
pub fn configure_registered_client(
    manager: &mut AuthorizationManager,
    metadata: Option<&StoredOAuthMetadata>,
    client: &RegisteredClient,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<(), AuthError> {
    let mut config =
        OAuthClientConfig::new(&client.client_id, redirect_uri).with_scopes(scopes.to_vec());

    if let Some(secret) = client.client_secret.as_deref() {
        config = config.with_client_secret(secret);

        let registered_method = client
            .token_endpoint_auth_method
            .as_deref()
            .filter(|method| SECRET_AUTH_METHODS.contains(method));
        if let (Some(method), Some(metadata)) = (registered_method, metadata) {
            let mut metadata = convert_from_stored_metadata(metadata);
            metadata.additional_fields.insert(
                "token_endpoint_auth_methods_supported".to_string(),
                serde_json::json!([method]),
            );
            manager.set_metadata(metadata);
        }
    }

    manager.configure_client(config)
}

/// Initialize `manager` from its credential store, including what rmcp's
/// `AuthorizationManager::initialize_from_store` leaves out. Call this instead of
/// that method.
///
/// rmcp restores only the client_id, so a confidential client would send its token
/// refresh without its secret, or with the wrong auth method, and get
/// `invalid_client`. This configures the client again from `registration`, the
/// stored registration for the same server.
///
/// Returns `Ok(false)` when there are no stored tokens, and also when the stored
/// client can't be configured (logged), so the caller asks the user to sign in.
pub async fn initialize_from_store(
    manager: &mut AuthorizationManager,
    registration: Option<&OutboundOAuthRegistration>,
) -> Result<bool, AuthError> {
    if !manager.initialize_from_store().await? {
        return Ok(false);
    }
    let Some(registration) = registration else {
        return Ok(true);
    };
    if registration.client_secret.is_none() {
        // A public client: the client_id rmcp restored is all it needs
        return Ok(true);
    }

    // The redirect URI only matters for the authorization request, not refresh.
    // Fall back to the server URL (what rmcp's configure_client_id uses) when the
    // stored one is missing or isn't a URL.
    let redirect_uri = registration
        .redirect_uri
        .as_deref()
        .filter(|uri| Url::parse(uri).is_ok())
        .unwrap_or(&registration.server_url);
    let client = RegisteredClient::from(registration);
    if let Err(e) = configure_registered_client(
        manager,
        registration.metadata.as_ref(),
        &client,
        redirect_uri,
        &[],
    ) {
        warn!(
            server_id = %registration.server_id,
            "[OAuth] Can't configure the stored OAuth client, sign-in required: {}", e
        );
        return Ok(false);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_origin_with_path() {
        assert_eq!(
            extract_origin("https://mcp.atlassian.com/v1/sse"),
            Some("https://mcp.atlassian.com".to_string())
        );
    }

    #[test]
    fn test_extract_origin_with_port() {
        assert_eq!(
            extract_origin("http://localhost:8080/api/v1"),
            Some("http://localhost:8080".to_string())
        );
    }

    #[test]
    fn test_extract_origin_no_path() {
        assert_eq!(
            extract_origin("https://example.com"),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn test_extract_origin_invalid_url() {
        assert_eq!(extract_origin("not a url"), None);
    }

    const SERVER_URL: &str = "https://mcp.example.com/mcp";

    fn stored_metadata(token_endpoint: &str) -> StoredOAuthMetadata {
        StoredOAuthMetadata {
            authorization_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: token_endpoint.to_string(),
            registration_endpoint: None,
            issuer: None,
            jwks_uri: None,
            scopes_supported: None,
            response_types_supported: None,
            additional_fields: Default::default(),
        }
    }

    /// A manager with metadata set (so no discovery) and, if `with_tokens`, stored
    /// tokens for "client-123"
    async fn manager(with_tokens: bool) -> AuthorizationManager {
        use oauth2::{basic::BasicTokenType, AccessToken, StandardTokenResponse};
        use rmcp::transport::auth::{
            CredentialStore, InMemoryCredentialStore, StoredCredentials, VendorExtraTokenFields,
        };

        let mut manager = AuthorizationManager::new(SERVER_URL).await.unwrap();
        manager.set_metadata(convert_from_stored_metadata(&stored_metadata(
            "https://auth.example.com/token",
        )));
        let store = InMemoryCredentialStore::new();
        if with_tokens {
            let tokens = StandardTokenResponse::new(
                AccessToken::new("access".to_string()),
                BasicTokenType::Bearer,
                VendorExtraTokenFields::default(),
            );
            store
                .save(StoredCredentials::new(
                    "client-123".to_string(),
                    Some(tokens),
                    Vec::new(),
                    None,
                ))
                .await
                .unwrap();
        }
        manager.set_credential_store(store);
        manager
    }

    fn confidential_registration() -> OutboundOAuthRegistration {
        OutboundOAuthRegistration::with_metadata(
            uuid::Uuid::new_v4(),
            "server",
            SERVER_URL,
            "client-123",
            "http://127.0.0.1:45819/oauth2redirect",
            stored_metadata("https://auth.example.com/token"),
        )
        .with_client_secret(Some("s3cr3t".to_string()))
        .with_token_endpoint_auth_method(Some("client_secret_post".to_string()))
    }

    #[tokio::test]
    async fn initialize_without_stored_tokens_needs_sign_in() {
        let mut manager = manager(false).await;
        let registration = confidential_registration();
        assert!(!initialize_from_store(&mut manager, Some(&registration))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn initialize_restores_a_confidential_client() {
        let mut manager = manager(true).await;
        let registration = confidential_registration();
        assert!(initialize_from_store(&mut manager, Some(&registration))
            .await
            .unwrap());
        let (client_id, _) = manager.get_credentials().await.unwrap();
        assert_eq!(client_id, "client-123");
    }

    #[tokio::test]
    async fn initialize_falls_back_to_server_url_for_a_bad_redirect_uri() {
        let mut manager = manager(true).await;
        let mut registration = confidential_registration();
        registration.redirect_uri = Some("not a url".to_string());
        assert!(initialize_from_store(&mut manager, Some(&registration))
            .await
            .unwrap());
    }

    /// A stored client that can't be configured means signing in again, not a hard
    /// connection failure
    #[tokio::test]
    async fn initialize_with_an_unusable_stored_client_needs_sign_in() {
        let mut manager = manager(true).await;
        let mut registration = confidential_registration();
        registration.metadata = Some(stored_metadata("not a url"));
        assert!(!initialize_from_store(&mut manager, Some(&registration))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn initialize_public_client_keeps_rmcp_configuration() {
        let mut manager = manager(true).await;
        let mut registration = confidential_registration();
        registration.client_secret = None;
        // Metadata that would fail if it were applied: a public client doesn't touch it
        registration.metadata = Some(stored_metadata("not a url"));
        assert!(initialize_from_store(&mut manager, Some(&registration))
            .await
            .unwrap());
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::is_acceptable_oauth_endpoint;

    #[test]
    fn only_https_or_loopback_http_endpoints_are_acceptable() {
        for url in [
            "https://auth.example.com/authorize",
            "http://localhost:8080/authorize",
            "http://127.0.0.1:9000/token",
            "http://[::1]:9000/token",
        ] {
            assert!(is_acceptable_oauth_endpoint(url), "{url}");
        }
        for url in [
            "http://auth.example.com/authorize",
            "file:///etc/passwd",
            "search-ms:query=x",
            "smb://host/share",
            "javascript:alert(1)",
            "not a url",
        ] {
            assert!(!is_acceptable_oauth_endpoint(url), "{url}");
        }
    }
}
