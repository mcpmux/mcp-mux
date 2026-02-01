//! Dynamic Client Registration (RFC 7591)
//!
//! Implements the OAuth 2.0 Dynamic Client Registration Protocol
//! for registering MCP clients with the gateway.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Dynamic Client Registration Request (RFC 7591)
#[derive(Debug, Clone, Deserialize)]
pub struct DcrRequest {
    /// Human-readable name of the client
    pub client_name: String,
    /// Array of allowed redirect URIs
    pub redirect_uris: Vec<String>,
    /// OAuth 2.0 grant types the client may use
    #[serde(default)]
    pub grant_types: Vec<String>,
    /// OAuth 2.0 response types the client may use
    #[serde(default)]
    pub response_types: Vec<String>,
    /// Authentication method for the token endpoint
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
    /// Scope values the client may request
    #[serde(default)]
    pub scope: Option<String>,

    // RFC 7591 Client Metadata
    /// URL for the client's logo
    #[serde(default)]
    pub logo_uri: Option<String>,
    /// URL of the client's homepage
    #[serde(default)]
    pub client_uri: Option<String>,
    /// URL for the client's terms of service
    #[serde(default)]
    pub tos_uri: Option<String>,
    /// URL for the client's privacy policy
    #[serde(default)]
    pub policy_uri: Option<String>,
    /// Contact email addresses
    #[serde(default)]
    pub contacts: Option<Vec<String>>,
    /// Unique identifier for the software (e.g., "com.cursor.app")
    #[serde(default)]
    pub software_id: Option<String>,
    /// Version of the client software
    #[serde(default)]
    pub software_version: Option<String>,
}

/// Dynamic Client Registration Response (RFC 7591)
#[derive(Debug, Clone, Serialize)]
pub struct DcrResponse {
    /// Unique client identifier
    pub client_id: String,
    /// Human-readable name of the client
    pub client_name: String,
    /// Array of allowed redirect URIs
    pub redirect_uris: Vec<String>,
    /// OAuth 2.0 grant types the client may use
    pub grant_types: Vec<String>,
    /// OAuth 2.0 response types the client may use
    pub response_types: Vec<String>,
    /// Authentication method for the token endpoint
    pub token_endpoint_auth_method: String,
    /// Timestamp of when the client was registered
    pub client_id_issued_at: u64,
    /// Scope values the client may request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    // RFC 7591 Client Metadata
    /// URL for the client's logo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    /// URL of the client's homepage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    /// URL for the client's terms of service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,
    /// URL for the client's privacy policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,
    /// Contact email addresses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,
    /// Unique identifier for the software
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    /// Version of the client software
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,
}

/// Helper to build InboundClient from DcrRequest
/// Eliminates ~100 lines of duplication between update and create paths
#[allow(clippy::too_many_arguments)]
fn build_inbound_client_from_request(
    request: &DcrRequest,
    client_id: String,
    redirect_uris: Vec<String>,
    grant_types: Vec<String>,
    response_types: Vec<String>,
    token_endpoint_auth_method: String,
    client_alias: Option<String>,
    connection_mode: String,
    locked_space_id: Option<String>,
    last_seen: Option<String>,
    created_at: String,
    updated_at: String,
) -> mcpmux_storage::InboundClient {
    mcpmux_storage::InboundClient {
        client_id,
        registration_type: mcpmux_storage::RegistrationType::Dcr,
        client_name: request.client_name.clone(),
        client_alias,
        redirect_uris,
        grant_types,
        response_types,
        token_endpoint_auth_method,
        scope: request.scope.clone(),
        // Not approved until user explicitly consents
        approved: false,
        // RFC 7591 client metadata
        logo_uri: request.logo_uri.clone(),
        client_uri: request.client_uri.clone(),
        software_id: request.software_id.clone(),
        software_version: request.software_version.clone(),
        // CIMD fields (empty for DCR)
        metadata_url: None,
        metadata_cached_at: None,
        metadata_cache_ttl: None,
        // MCP client settings
        connection_mode,
        locked_space_id,
        last_seen,
        created_at,
        updated_at,
    }
}

/// DCR Error Response
#[derive(Debug, Clone, Serialize)]
pub struct DcrError {
    /// Error code
    pub error: String,
    /// Human-readable error description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl DcrError {
    pub fn invalid_redirect_uri(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_redirect_uri".to_string(),
            error_description: Some(description.into()),
        }
    }

    pub fn invalid_client_metadata(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_client_metadata".to_string(),
            error_description: Some(description.into()),
        }
    }
}

/// Validate redirect URIs per RFC 8252 (OAuth 2.0 for Native Apps)
///
/// Allowed redirect URI types:
/// 1. Loopback: http://127.0.0.1:PORT/... or http://localhost:PORT/...
/// 2. Custom URL schemes: cursor://, vscode://, claude://, etc.
///
/// NOT allowed:
/// - https:// URLs (except for confidential clients with proper secrets)
/// - http:// URLs to non-loopback addresses
pub fn validate_redirect_uris(uris: &[String]) -> Result<(), DcrError> {
    if uris.is_empty() {
        return Err(DcrError::invalid_redirect_uri(
            "At least one redirect_uri is required",
        ));
    }

    for uri in uris {
        let is_loopback = uri.starts_with("http://127.0.0.1")
            || uri.starts_with("http://localhost")
            || uri.starts_with("http://[::1]");

        // Custom URL schemes (like cursor://, vscode://) are allowed
        // They don't start with http:// or https://
        let is_custom_scheme = !uri.starts_with("http://") && !uri.starts_with("https://");

        if !is_loopback && !is_custom_scheme {
            warn!(
                "[DCR] Rejected redirect_uri: {} (must be loopback or custom scheme)",
                uri
            );
            return Err(DcrError::invalid_redirect_uri(
                "Redirect URI must be loopback (http://127.0.0.1 or http://localhost) \
                 or a custom URL scheme (e.g., cursor://, vscode://)",
            ));
        }

        debug!(
            "[DCR] Validated redirect_uri: {} (loopback={}, custom_scheme={})",
            uri, is_loopback, is_custom_scheme
        );
    }

    Ok(())
}

/// Process a DCR request and return a registered client or error
///
/// Uses the database as the single source of truth (no in-memory registry)
pub async fn process_dcr_request(
    repo: &mcpmux_storage::InboundClientRepository,
    request: DcrRequest,
) -> Result<DcrResponse, DcrError> {
    info!(
        "[DCR] Processing registration for: {} (redirect_uris: {:?})",
        request.client_name, request.redirect_uris
    );

    // Validate redirect URIs
    validate_redirect_uris(&request.redirect_uris)?;

    // Check for existing client with same name (idempotent registration by client_name)
    let existing = repo
        .find_client_by_name(&request.client_name)
        .await
        .map_err(|e| DcrError::invalid_client_metadata(format!("Database error: {}", e)))?;

    if let Some(existing) = existing {
        info!(
            "[DCR] Updating existing client: {} ({})",
            request.client_name, existing.client_id
        );

        let client_id = existing.client_id.clone();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Merge redirect URIs (accumulate - keep old URIs valid)
        let mut merged_uris = existing.redirect_uris;
        for uri in &request.redirect_uris {
            if !merged_uris.contains(uri) {
                merged_uris.push(uri.clone());
                info!(
                    "[DCR] Adding new redirect_uri: {} to client: {}",
                    uri, client_id
                );
            }
        }

        // Default grant_types and response_types if not provided
        let grant_types = if request.grant_types.is_empty() {
            vec![
                "authorization_code".to_string(),
                "refresh_token".to_string(),
            ]
        } else {
            request.grant_types.clone()
        };

        let response_types = if request.response_types.is_empty() {
            vec!["code".to_string()]
        } else {
            request.response_types.clone()
        };

        let token_endpoint_auth_method = request
            .token_endpoint_auth_method
            .clone()
            .unwrap_or_else(|| "none".to_string());

        // Use helper to build updated client (preserves user settings)
        let updated_client = build_inbound_client_from_request(
            &request,
            client_id.clone(),
            merged_uris.clone(),
            grant_types.clone(),
            response_types.clone(),
            token_endpoint_auth_method.clone(),
            existing.client_alias,    // Preserve user-set alias
            existing.connection_mode, // Preserve connection mode
            existing.locked_space_id, // Preserve locked space
            existing.last_seen,
            existing.created_at,
            now,
        );

        // Save to database (single source of truth)
        repo.save_client(&updated_client).await.map_err(|e| {
            DcrError::invalid_client_metadata(format!("Failed to save client: {}", e))
        })?;

        return Ok(DcrResponse {
            client_id,
            client_name: request.client_name,
            redirect_uris: merged_uris,
            grant_types,
            response_types,
            token_endpoint_auth_method,
            scope: request.scope,
            client_id_issued_at: now_unix,
            // RFC 7591 metadata
            logo_uri: request.logo_uri,
            client_uri: request.client_uri,
            tos_uri: request.tos_uri,
            policy_uri: request.policy_uri,
            contacts: request.contacts,
            software_id: request.software_id,
            software_version: request.software_version,
        });
    }

    // Generate new client_id
    let client_id = format!("mcp_{}", &Uuid::new_v4().to_string()[..8]);
    let now_str = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Default grant_types and response_types per OAuth 2.1
    let grant_types = if request.grant_types.is_empty() {
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ]
    } else {
        request.grant_types.clone()
    };

    let response_types = if request.response_types.is_empty() {
        vec!["code".to_string()]
    } else {
        request.response_types.clone()
    };

    let token_endpoint_auth_method = request
        .token_endpoint_auth_method
        .clone()
        .unwrap_or_else(|| "none".to_string());

    // Use helper to build new client (default settings)
    let client = build_inbound_client_from_request(
        &request,
        client_id.clone(),
        request.redirect_uris.clone(),
        grant_types.clone(),
        response_types.clone(),
        token_endpoint_auth_method.clone(),
        None,                        // No alias yet
        "follow_active".to_string(), // Default connection mode
        None,                        // No locked space
        Some(now_str.clone()),
        now_str.clone(),
        now_str,
    );

    // Save to database (single source of truth)
    repo.save_client(&client)
        .await
        .map_err(|e| DcrError::invalid_client_metadata(format!("Failed to save client: {}", e)))?;

    info!(
        "[DCR] New client registered: {} ({})",
        request.client_name, client_id
    );

    Ok(DcrResponse {
        client_id,
        client_name: request.client_name,
        redirect_uris: request.redirect_uris,
        grant_types,
        response_types,
        token_endpoint_auth_method,
        scope: request.scope,
        client_id_issued_at: now_unix,
        // RFC 7591 client metadata
        logo_uri: request.logo_uri,
        client_uri: request.client_uri,
        tos_uri: request.tos_uri,
        policy_uri: request.policy_uri,
        contacts: request.contacts,
        software_id: request.software_id,
        software_version: request.software_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_loopback_uris() {
        // Valid loopback URIs
        assert!(validate_redirect_uris(&["http://127.0.0.1:8080/callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["http://localhost:3000/callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["http://[::1]:8080/callback".to_string()]).is_ok());
    }

    #[test]
    fn test_validate_custom_scheme_uris() {
        // Valid custom scheme URIs
        assert!(validate_redirect_uris(&["cursor://callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["vscode://callback".to_string()]).is_ok());
        assert!(validate_redirect_uris(&["claude://auth/callback".to_string()]).is_ok());
    }

    #[test]
    fn test_reject_invalid_uris() {
        // Invalid URIs (non-loopback http)
        assert!(validate_redirect_uris(&["http://example.com/callback".to_string()]).is_err());
        assert!(validate_redirect_uris(&["https://example.com/callback".to_string()]).is_err());
    }

    // Note: Integration tests for idempotent registration are better handled
    // in tests that use an actual database, since process_dcr_request now
    // persists directly to the database.
}
