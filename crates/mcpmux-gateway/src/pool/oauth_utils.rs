//! Shared OAuth utilities for metadata discovery with origin URL fallback.
//!
//! Some OAuth servers (like Atlassian) serve their metadata at the origin URL
//! (e.g., `https://mcp.atlassian.com`) rather than the endpoint path
//! (e.g., `https://mcp.atlassian.com/v1/sse`). This module provides utilities
//! to handle both cases.

use mcpmux_core::StoredOAuthMetadata;
use rmcp::transport::auth::{AuthError, AuthorizationManager, AuthorizationMetadata};
use tracing::info;
use url::Url;

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
    let stored = convert_to_stored_metadata(&metadata);
    Ok((metadata, stored))
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
    AuthorizationMetadata {
        authorization_endpoint: stored.authorization_endpoint.clone(),
        token_endpoint: stored.token_endpoint.clone(),
        registration_endpoint: stored.registration_endpoint.clone(),
        issuer: stored.issuer.clone(),
        jwks_uri: stored.jwks_uri.clone(),
        scopes_supported: stored.scopes_supported.clone(),
        response_types_supported: stored.response_types_supported.clone(),
        additional_fields: stored.additional_fields.clone(),
    }
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
}
