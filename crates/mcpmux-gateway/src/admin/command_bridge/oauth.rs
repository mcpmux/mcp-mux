//! OAuth consent bridge — pending consent reads and approve/reject writes.

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

use crate::oauth::{
    approve_oauth_consent as process_consent, get_pending_consent as fetch_pending,
    ConsentApprovalRequest,
};

use super::super::bridge_context::AdminBridgeCtx;

/// Body for admin HTTP consent approve/reject endpoints.
#[derive(Debug, serde::Deserialize)]
pub struct OAuthConsentBody {
    pub request_id: String,
    pub consent_token: String,
    #[serde(default)]
    pub client_alias: Option<String>,
}

/// Shared gateway state handle for inbound OAuth consent (web admin HTTP path).
async fn gateway_state(
    ctx: &AdminBridgeCtx,
) -> anyhow::Result<std::sync::Arc<tokio::sync::RwLock<crate::GatewayState>>> {
    ctx.gateway_writes
        .gateway_state()
        .await
        .context("Gateway not running")
}

/// Validate a pending OAuth consent request and return authoritative details.
pub async fn get_pending_consent(
    ctx: &AdminBridgeCtx,
    request_id: String,
) -> Result<Value, anyhow::Error> {
    let gw = gateway_state(ctx).await?;
    let details = fetch_pending(&gw, request_id)
        .await
        .map_err(|e| anyhow!("{}: {}", e.code, e.message))?;
    Ok(json!(details))
}

/// Approve a pending OAuth consent request.
pub async fn approve_oauth_consent(
    ctx: &AdminBridgeCtx,
    body: OAuthConsentBody,
) -> Result<Value, anyhow::Error> {
    let gw = gateway_state(ctx).await?;
    let response = process_consent(
        &gw,
        ConsentApprovalRequest {
            request_id: body.request_id,
            approved: true,
            consent_token: body.consent_token,
            client_alias: body.client_alias,
        },
    )
    .await
    .map_err(|e| anyhow!(e))?;
    Ok(json!(response))
}

/// Reject a pending OAuth consent request.
pub async fn reject_oauth_consent(
    ctx: &AdminBridgeCtx,
    body: OAuthConsentBody,
) -> Result<Value, anyhow::Error> {
    let gw = gateway_state(ctx).await?;
    let response = process_consent(
        &gw,
        ConsentApprovalRequest {
            request_id: body.request_id,
            approved: false,
            consent_token: body.consent_token,
            client_alias: None,
        },
    )
    .await
    .map_err(|e| anyhow!(e))?;
    Ok(json!(response))
}
