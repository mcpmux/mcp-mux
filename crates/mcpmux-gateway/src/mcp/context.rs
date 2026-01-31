//! Context utilities for extracting OAuth and space information from MCP requests

use anyhow::{anyhow, Result};
use http;
use rmcp::{
    RoleServer,
    model::Extensions,
    service::RequestContext,
};
use uuid::Uuid;

/// OAuth claims extracted from JWT token
#[derive(Debug, Clone)]
pub struct OAuthContext {
    pub client_id: String,
    pub space_id: Uuid,
}

/// Extract OAuth context from extensions
/// 
/// The context is injected via custom HTTP headers by OAuth middleware.
/// If headers are missing, returns None (caller should check session metadata fallback).
pub fn extract_oauth_context(extensions: &Extensions) -> Result<OAuthContext> {
    // OAuth context is passed via custom headers (preserved by rmcp)
    let parts = extensions.get::<http::request::Parts>()
        .ok_or_else(|| anyhow!("HTTP request parts not found in extensions"))?;
    
    // Extract client_id from header
    let client_id = parts.headers.get("x-mcmux-client-id")
        .ok_or_else(|| {
            let has_auth = parts.headers.get("authorization").is_some();
            anyhow!(
                "x-mcmux-client-id header missing (Authorization header present: {}). Middleware may not have run or client reconnected without auth.",
                has_auth
            )
        })?
        .to_str()
        .map_err(|_| anyhow!("Invalid x-mcmux-client-id header value"))?
        .to_string();
    
    // Extract space_id from header
    let space_id_str = parts.headers.get("x-mcmux-space-id")
        .ok_or_else(|| anyhow!("x-mcmux-space-id header missing"))?
        .to_str()
        .map_err(|_| anyhow!("Invalid x-mcmux-space-id header value"))?;
    
    let space_id = Uuid::parse_str(space_id_str)
        .map_err(|e| anyhow!("Failed to parse space_id: {}", e))?;
    
    Ok(OAuthContext {
        client_id,
        space_id,
    })
}

/// Extract session ID from request headers
pub fn extract_session_id(extensions: &Extensions) -> Option<String> {
    extensions.get::<http::request::Parts>()
        .and_then(|parts| {
            parts.headers
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




