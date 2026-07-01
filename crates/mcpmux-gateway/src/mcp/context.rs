//! Context utilities for extracting OAuth and space information from MCP requests

use anyhow::{anyhow, Result};
use http;
use rmcp::{model::Extensions, service::RequestContext, RoleServer};
use uuid::Uuid;

/// OAuth claims extracted from JWT token, plus optional per-device machine identity.
#[derive(Debug, Clone)]
pub struct OAuthContext {
    pub client_id: String,
    pub space_id: Uuid,
    /// Physical device identity from the client's `X-Mcpmux-Machine-Id` header.
    /// Distinct from the gateway's `local_machine_id` when callers reach a shared
    /// tunneled gateway from multiple machines.
    pub request_machine_id: Option<Uuid>,
}

/// Extract OAuth context from extensions
///
/// The context is injected via custom HTTP headers by OAuth middleware.
/// If headers are missing, returns None (caller should check session metadata fallback).
pub fn extract_oauth_context(extensions: &Extensions) -> Result<OAuthContext> {
    // OAuth context is passed via custom headers (preserved by rmcp)
    let parts = extensions
        .get::<http::request::Parts>()
        .ok_or_else(|| anyhow!("HTTP request parts not found in extensions"))?;

    // Extract client_id from header
    let client_id = parts.headers.get("x-mcpmux-client-id")
        .ok_or_else(|| {
            let has_auth = parts.headers.get("authorization").is_some();
            anyhow!(
                "x-mcpmux-client-id header missing (Authorization header present: {}). Middleware may not have run or client reconnected without auth.",
                has_auth
            )
        })?
        .to_str()
        .map_err(|_| anyhow!("Invalid x-mcpmux-client-id header value"))?
        .to_string();

    // Extract space_id from header
    let space_id_str = parts
        .headers
        .get("x-mcpmux-space-id")
        .ok_or_else(|| anyhow!("x-mcpmux-space-id header missing"))?
        .to_str()
        .map_err(|_| anyhow!("Invalid x-mcpmux-space-id header value"))?;

    let space_id =
        Uuid::parse_str(space_id_str).map_err(|e| anyhow!("Failed to parse space_id: {}", e))?;

    let request_machine_id = parts
        .headers
        .get("x-mcpmux-machine-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok());

    Ok(OAuthContext {
        client_id,
        space_id,
        request_machine_id,
    })
}

/// Extract session ID from request headers
pub fn extract_session_id(extensions: &Extensions) -> Option<String> {
    extensions
        .get::<http::request::Parts>()
        .and_then(|parts| {
            parts
                .headers
                .get("mcp-session-id")
                .or_else(|| parts.headers.get("Mcp-Session-Id"))
        })
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Extract client ID from request context
pub fn extract_client_id(context: &RequestContext<RoleServer>) -> Result<String> {
    Ok(extract_oauth_context(&context.extensions)?.client_id)
}

/// Extract space ID from request context
pub fn extract_space_id(context: &RequestContext<RoleServer>) -> Result<Uuid> {
    Ok(extract_oauth_context(&context.extensions)?.space_id)
}
