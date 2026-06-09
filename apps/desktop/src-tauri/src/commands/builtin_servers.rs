//! Tauri commands for the per-Space built-in server config.
//!
//! Built-in servers (today: "Tool Optimization", the `mcpmux_*` tools) and
//! their individual tools are enabled/disabled **per Space**. The descriptors
//! (ids, names, tool sets) come from `mcpmux_core::builtin_servers()`; the
//! per-Space enable state comes from `SpaceBuiltinConfigRepository`. Toggling
//! emits `BuiltinServerConfigChanged` so the gateway re-pushes
//! `tools/list_changed` to that Space's connected clients.

use std::sync::Arc;

use mcpmux_core::DomainEvent;
use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::gateway::GatewayAppState;
use crate::state::AppState;

/// One tool of a built-in server, with its per-Space enabled state.
#[derive(Debug, Clone, Serialize)]
pub struct BuiltinToolDto {
    pub name: String,
    pub description: String,
    /// Mutating tool — gated behind a native approval dialog at call time.
    pub write: bool,
    pub enabled: bool,
}

/// A built-in server as configured for a specific Space.
#[derive(Debug, Clone, Serialize)]
pub struct BuiltinServerDto {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Whether this built-in server is enabled for the Space.
    pub enabled: bool,
    pub tools: Vec<BuiltinToolDto>,
}

/// Publish `BuiltinServerConfigChanged` so MCPNotifier re-pushes
/// `tools/list_changed` to the Space's peers. Best-effort: gateway not running
/// (no subscribers) is a normal startup condition and must not fail the toggle.
async fn emit_builtin_changed(gateway_state: &Arc<RwLock<GatewayAppState>>, space_id: Uuid) {
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        gw.read()
            .await
            .emit_domain_event(DomainEvent::BuiltinServerConfigChanged { space_id });
    }
}

/// List every built-in server with its per-Space enable state and per-tool
/// toggles. Combines the static descriptors with the Space's stored overrides
/// (absence of an override = the descriptor default / tool-on).
#[tauri::command]
pub async fn list_builtin_servers(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<BuiltinServerDto>, String> {
    let repo = &state.space_builtin_config_repository;
    let mut out = Vec::new();
    for d in mcpmux_core::builtin_servers() {
        let enabled = repo
            .server_enabled_override(&space_id, d.id)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or(d.default_enabled);
        let disabled = repo
            .disabled_tools(&space_id, d.id)
            .await
            .map_err(|e| e.to_string())?;
        let tools = d
            .tools
            .iter()
            .map(|t| BuiltinToolDto {
                name: t.name.to_string(),
                description: t.description.to_string(),
                write: t.write,
                enabled: !disabled.iter().any(|n| n == t.name),
            })
            .collect();
        out.push(BuiltinServerDto {
            id: d.id.to_string(),
            name: d.name.to_string(),
            description: d.description.to_string(),
            enabled,
            tools,
        });
    }
    Ok(out)
}

/// Enable/disable a built-in server for a Space.
#[tauri::command]
pub async fn set_builtin_server_enabled(
    space_id: String,
    server_id: String,
    enabled: bool,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let sid = Uuid::parse_str(&space_id).map_err(|e| format!("bad space_id: {e}"))?;
    state
        .space_builtin_config_repository
        .set_server_enabled(&space_id, &server_id, enabled)
        .await
        .map_err(|e| e.to_string())?;
    emit_builtin_changed(gateway_state.inner(), sid).await;
    Ok(())
}

/// Enable/disable a single tool of a built-in server for a Space.
#[tauri::command]
pub async fn set_builtin_tool_enabled(
    space_id: String,
    server_id: String,
    tool_name: String,
    enabled: bool,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let sid = Uuid::parse_str(&space_id).map_err(|e| format!("bad space_id: {e}"))?;
    state
        .space_builtin_config_repository
        .set_tool_enabled(&space_id, &server_id, &tool_name, enabled)
        .await
        .map_err(|e| e.to_string())?;
    emit_builtin_changed(gateway_state.inner(), sid).await;
    Ok(())
}
