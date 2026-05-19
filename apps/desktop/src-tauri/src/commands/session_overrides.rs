//! Tauri commands for inspecting and clearing session-scoped server overrides.

use std::sync::Arc;

use mcpmux_gateway::services::SessionOverrideEntry;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tracing::info;

use super::gateway::GatewayAppState;

/// Per-session override state surfaced to the Workspaces inspector.
#[derive(Debug, Clone, Serialize)]
pub struct SessionOverrideDto {
    pub session_id: String,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
    pub roots: Vec<String>,
}

impl SessionOverrideDto {
    fn from_entry(entry: SessionOverrideEntry, roots: Vec<String>) -> Self {
        Self {
            session_id: entry.session_id,
            enabled: entry.enabled,
            disabled: entry.disabled,
            roots,
        }
    }
}

fn build_dtos(gateway: &GatewayAppState) -> Vec<SessionOverrideDto> {
    let Some(ref overrides) = gateway.session_overrides else {
        return vec![];
    };
    let roots_by_session: std::collections::HashMap<String, Vec<String>> = gateway
        .session_roots
        .as_ref()
        .map(|reg| {
            reg.list_all_sessions()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();

    overrides
        .list_all()
        .into_iter()
        .map(|entry| {
            let roots = roots_by_session
                .get(&entry.session_id)
                .cloned()
                .unwrap_or_default();
            SessionOverrideDto::from_entry(entry, roots)
        })
        .collect()
}

/// List override state for one session, or every session when `session_id`
/// is omitted. Returns an empty list when the gateway is not running.
#[tauri::command]
pub async fn list_session_overrides(
    session_id: Option<String>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Vec<SessionOverrideDto>, String> {
    let guard = gateway_state.read().await;
    let mut dtos = build_dtos(&guard);
    if let Some(sid) = session_id {
        dtos.retain(|d| d.session_id == sid);
    }
    Ok(dtos)
}

/// Drop all enable/disable overrides for a session and push list_changed so
/// the client's tool list reverts to binding-only routing.
#[tauri::command]
pub async fn clear_session_overrides(
    session_id: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let notifier = {
        let guard = gateway_state.read().await;
        let overrides = guard
            .session_overrides
            .as_ref()
            .ok_or("Gateway is not running")?;
        overrides.clear(&session_id);
        guard.mcp_notifier.clone()
    };

    if let Some(notifier) = notifier {
        notifier.notify_session_lists_changed(&session_id).await;
    }

    info!("[session_overrides] cleared overrides for session {}", session_id);

    if let Err(e) = app_handle.emit(
        "session-overrides-changed",
        serde_json::json!({ "session_id": session_id }),
    ) {
        tracing::warn!("[session_overrides] failed to emit session-overrides-changed: {e}");
    }

    Ok(())
}
