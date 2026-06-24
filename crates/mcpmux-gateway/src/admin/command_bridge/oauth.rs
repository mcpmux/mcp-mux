//! OAuth consent bridge — pending consent reads and approve/reject writes.

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::super::bridge_context::AdminBridgeCtx;

/// Body for admin HTTP consent approve/reject endpoints.
#[derive(Debug, serde::Deserialize)]
pub struct OAuthConsentBody {
    pub request_id: String,
    pub consent_token: String,
    #[serde(default)]
    pub client_alias: Option<String>,
}

/// Validate a pending OAuth consent request and return authoritative details.
///
/// ponytail: full consent flow is Tauri-IPC-gated; this stub prevents HTTP bypass.
pub async fn get_pending_consent(_ctx: &AdminBridgeCtx, _request_id: String) -> Result<Value> {
    Err(anyhow!("OAuth consent requires the desktop Tauri command"))
}

/// Approve a pending OAuth consent request.
///
/// ponytail: approval must go through Tauri IPC, not HTTP, for security.
pub async fn approve_oauth_consent(_ctx: &AdminBridgeCtx, body: OAuthConsentBody) -> Result<Value> {
    let _ = body;
    Err(anyhow!(
        "OAuth consent approval requires the desktop Tauri command"
    ))
}

/// Reject a pending OAuth consent request.
pub async fn reject_oauth_consent(_ctx: &AdminBridgeCtx, body: OAuthConsentBody) -> Result<Value> {
    let _ = body;
    Err(anyhow!(
        "OAuth consent rejection requires the desktop Tauri command"
    ))
}
