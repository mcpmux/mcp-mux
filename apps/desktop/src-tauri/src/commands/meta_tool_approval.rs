//! Tauri commands for meta-tool approval dialogs.
//!
//! Flow:
//!   1. Gateway's [`ApprovalBroker`] emits `meta-tool-approval-request`
//!      event (see gateway.rs `start_gateway`).
//!   2. React dialog renders it, user picks once/always/deny.
//!   3. Dialog calls [`respond_to_meta_tool_approval`], which resolves the
//!      broker's oneshot channel and unblocks the calling tool.

use std::sync::Arc;

use mcpmux_gateway::services::ApprovalDecision;
use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::commands::gateway::GatewayAppState;

#[derive(Debug, Serialize)]
pub struct MetaToolGrantEntry {
    pub client_id: String,
    pub tool_name: String,
}

/// Resolve a pending approval dialog.
///
/// `decision` is one of `"allow_once" | "always_for_this_session_and_client" | "deny"`.
/// Called from the React dialog. If the broker doesn't recognize the
/// request_id (e.g. it already timed out), returns a no-op success so the
/// UI can close its dialog cleanly.
#[tauri::command]
pub async fn respond_to_meta_tool_approval(
    request_id: String,
    client_id: String,
    tool_name: String,
    decision: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<bool, String> {
    let decision = match decision.as_str() {
        "allow_once" => ApprovalDecision::AllowOnce,
        "always_for_this_session_and_client" => ApprovalDecision::AlwaysForThisSessionAndClient,
        "deny" => ApprovalDecision::Deny,
        other => return Err(format!("unknown decision: {other}")),
    };

    let broker = {
        let state = gateway_state.read().await;
        state.approval_broker.clone()
    };
    let Some(broker) = broker else {
        warn!("[meta-tool] respond called but gateway is not running");
        return Ok(false);
    };

    // client_id is opaque (UUID for preset clients, OAuth client_metadata
    // URL for DCR clients like Claude Code). The broker treats it as a
    // hash key only.
    let resolved = broker.respond(&request_id, &client_id, &tool_name, decision);
    info!(
        %request_id,
        %client_id,
        tool = %tool_name,
        ?decision,
        resolved,
        "[meta-tool] approval decision recorded"
    );
    Ok(resolved)
}

/// List every active "always allow from this client for this tool" grant.
///
/// Entries are session-only (cleared on gateway restart by design). The
/// Connections page uses this to show a revoke list.
#[tauri::command]
pub async fn list_meta_tool_grants(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Vec<MetaToolGrantEntry>, String> {
    let broker = {
        let state = gateway_state.read().await;
        state.approval_broker.clone()
    };
    let Some(broker) = broker else {
        return Ok(vec![]);
    };
    Ok(broker
        .list_always_allow()
        .into_iter()
        .map(|(client_id, tool_name)| MetaToolGrantEntry {
            client_id,
            tool_name,
        })
        .collect())
}

/// Revoke an "always allow" entry.
#[tauri::command]
pub async fn revoke_meta_tool_grant(
    client_id: String,
    tool_name: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<bool, String> {
    let broker = {
        let state = gateway_state.read().await;
        state.approval_broker.clone()
    };
    let Some(broker) = broker else {
        return Ok(false);
    };
    Ok(broker.revoke_always_allow(&client_id, &tool_name))
}

/// DEBUG/dev only: toggle auto-approval of all write meta tools.
///
/// When on, every `mcpmux_*` write tool (create_feature_set,
/// bind_current_workspace, …) is approved without a dialog. This exists so a
/// developer (or the in-app assistant) can self-create feature sets / bindings
/// and exercise the routing end-to-end without clicking through approvals.
/// Session-only — it is **not** persisted and resets to the
/// `MCPMUX_DEBUG_AUTO_APPROVE` env default on gateway restart.
#[tauri::command]
pub async fn set_meta_tools_auto_approve(
    enabled: bool,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<bool, String> {
    let broker = {
        let state = gateway_state.read().await;
        state.approval_broker.clone()
    };
    let Some(broker) = broker else {
        warn!("[meta-tool] set_meta_tools_auto_approve called but gateway is not running");
        return Err("gateway is not running".into());
    };
    broker.set_auto_approve(enabled);
    warn!(enabled, "[meta-tool] auto-approve toggled (DEBUG)");
    Ok(enabled)
}

/// Whether write meta tools are currently auto-approved (DEBUG mode state).
#[tauri::command]
pub async fn get_meta_tools_auto_approve(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<bool, String> {
    let broker = {
        let state = gateway_state.read().await;
        state.approval_broker.clone()
    };
    let Some(broker) = broker else {
        return Ok(false);
    };
    Ok(broker.auto_approve_enabled())
}
