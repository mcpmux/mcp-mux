//! Space management commands
//!
//! IPC commands for managing spaces (isolated environments).

use mcpmux_core::{ConnectionMode, Space};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::commands::gateway::GatewayAppState;
use crate::state::AppState;
use crate::tray;

/// Space change event payload
#[derive(Debug, Clone, Serialize)]
pub struct SpaceChangeEvent {
    /// Previous active space ID
    pub from_space_id: Option<String>,
    /// New active space ID
    pub to_space_id: String,
    /// New active space name
    pub to_space_name: String,
    /// Clients that need confirmation (AskOnChange mode)
    pub clients_needing_confirmation: Vec<ClientConfirmation>,
}

/// Client that needs confirmation for space change
#[derive(Debug, Clone, Serialize)]
pub struct ClientConfirmation {
    /// Client ID
    pub id: String,
    /// Client name
    pub name: String,
}

/// List all spaces.
#[tauri::command]
pub async fn list_spaces(state: State<'_, AppState>) -> Result<Vec<Space>, String> {
    tracing::info!("[list_spaces] Command invoked");

    let spaces = state.space_service.list().await.map_err(|e| {
        tracing::error!("[list_spaces] Error: {}", e);
        e.to_string()
    })?;

    tracing::info!("[list_spaces] Returning {} spaces", spaces.len());
    for space in &spaces {
        tracing::info!("[list_spaces] Space: {} ({})", space.name, space.id);
    }

    Ok(spaces)
}

/// Get a space by ID.
#[tauri::command]
pub async fn get_space(id: String, state: State<'_, AppState>) -> Result<Option<Space>, String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state
        .space_service
        .get(&uuid)
        .await
        .map_err(|e| e.to_string())
}

/// Default space configuration template
const DEFAULT_SPACE_CONFIG: &str = r#"{
  "mcpServers": {
  }
}
"#;

/// Create a new space.
#[tauri::command]
pub async fn create_space(
    name: String,
    icon: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Space, String> {
    let space = state
        .space_service
        .create(name.clone(), icon.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Create default config file for the space (spaces_dir already exists via AppState::new)
    let config_path = state.space_config_path(&space.id.to_string());

    // Create default config file if it doesn't exist
    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_SPACE_CONFIG)
            .map_err(|e| format!("Failed to create config file: {}", e))?;
        info!(
            "[create_space] Created config file: {}",
            config_path.display()
        );
    }

    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        gw.emit_domain_event(mcpmux_core::DomainEvent::SpaceCreated {
            space_id: space.id,
            name: space.name.clone(),
            icon: icon.clone(),
        });
    }

    // Update system tray menu
    if let Err(e) = tray::update_tray_spaces(&app, &state).await {
        warn!("Failed to update tray menu: {}", e);
    }

    Ok(space)
}

/// Delete a space.
#[tauri::command]
pub async fn delete_space(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    state
        .space_service
        .delete(&uuid)
        .await
        .map_err(|e| e.to_string())?;

    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        gw.emit_domain_event(mcpmux_core::DomainEvent::SpaceDeleted { space_id: uuid });
    }

    // Update system tray menu
    if let Err(e) = tray::update_tray_spaces(&app, &state).await {
        warn!("Failed to update tray menu: {}", e);
    }

    Ok(())
}

/// Get the active (default) space.
#[tauri::command]
pub async fn get_active_space(state: State<'_, AppState>) -> Result<Option<Space>, String> {
    tracing::info!("[get_active_space] Command invoked");

    let active = state.space_service.get_active().await.map_err(|e| {
        tracing::error!("[get_active_space] Error: {}", e);
        e.to_string()
    })?;

    if let Some(ref space) = active {
        tracing::info!(
            "[get_active_space] Returning: {} ({})",
            space.name,
            space.id
        );
    } else {
        tracing::warn!("[get_active_space] No active space found");
    }

    Ok(active)
}

/// Set the active space.
#[tauri::command]
pub async fn set_active_space<R: tauri::Runtime>(
    id: String,
    app_handle: AppHandle<R>,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let new_space_uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    // Get current active space before changing
    let old_space = state
        .space_service
        .get_active()
        .await
        .map_err(|e| e.to_string())?;

    // Set new active space
    state
        .space_service
        .set_active(&new_space_uuid)
        .await
        .map_err(|e| e.to_string())?;

    // Get new space details
    let new_space = state
        .space_service
        .get(&new_space_uuid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Space not found")?;

    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;

        // Emit activated event with transition info
        gw.emit_domain_event(mcpmux_core::DomainEvent::SpaceActivated {
            from_space_id: old_space.as_ref().map(|s| s.id),
            to_space_id: new_space.id,
            to_space_name: new_space.name.clone(),
        });
    }

    // Find clients with AskOnChange mode
    let clients = state
        .client_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;

    let clients_needing_confirmation: Vec<ClientConfirmation> = clients
        .into_iter()
        .filter(|c| matches!(c.connection_mode, ConnectionMode::AskOnChange { .. }))
        .map(|c| ClientConfirmation {
            id: c.id.to_string(),
            name: c.name,
        })
        .collect();

    // Emit legacy space-changed event for backward compatibility (can be removed later)
    let event = SpaceChangeEvent {
        from_space_id: old_space.map(|s| s.id.to_string()),
        to_space_id: new_space.id.to_string(),
        to_space_name: new_space.name,
        clients_needing_confirmation: clients_needing_confirmation.clone(),
    };

    if let Err(e) = app_handle.emit("space-changed", &event) {
        warn!("Failed to emit space-changed event: {}", e);
    } else {
        info!(
            "Emitted space-changed event: {} clients need confirmation",
            clients_needing_confirmation.len()
        );
    }

    // Note: MCP list_changed notifications for follow_active clients
    // will be emitted by the gateway when they make their next request
    // and the SpaceResolver returns the new active space.

    // Update system tray menu to reflect new active space
    if let Err(e) = tray::update_tray_spaces(&app_handle, &state).await {
        warn!("Failed to update tray menu: {}", e);
    }

    Ok(())
}

/// Open space configuration file in external editor
#[tauri::command]
pub async fn open_space_config_file(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use std::process::Command;

    let config_path = state.space_config_path(&space_id);

    if !config_path.exists() {
        return Err(format!(
            "Space config file not found: {}",
            config_path.display()
        ));
    }

    // Open in default editor based on platform
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", config_path.to_str().unwrap()])
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&config_path)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&config_path)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }

    Ok(())
}

/// Read space configuration file
#[tauri::command]
pub async fn read_space_config(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let config_path = state.space_config_path(&space_id);

    // Create default config if it doesn't exist (for spaces created before this feature)
    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_SPACE_CONFIG)
            .map_err(|e| format!("Failed to create config file: {}", e))?;
        info!(
            "[read_space_config] Created default config file: {}",
            config_path.display()
        );
    }

    std::fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config file: {}", e))
}

/// Save space configuration file
#[tauri::command]
pub async fn save_space_config(
    space_id: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config_path = state.space_config_path(&space_id);

    // Validate JSON before saving
    serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| format!("Invalid JSON: {}", e))?;

    std::fs::write(&config_path, content).map_err(|e| format!("Failed to write config file: {}", e))
}

/// Remove a server from the space configuration file
#[tauri::command]
pub async fn remove_server_from_config(
    space_id: String,
    server_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let config_path = state.space_config_path(&space_id);

    // If config file doesn't exist, nothing to remove
    if !config_path.exists() {
        return Ok(false);
    }

    // Read current config
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    // Parse as JSON
    let mut config: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

    // Get mcpServers object
    let servers = config.get_mut("mcpServers").and_then(|v| v.as_object_mut());

    if let Some(servers) = servers {
        // Check if server exists
        if servers.remove(&server_id).is_some() {
            // Write back the modified config
            let new_content = serde_json::to_string_pretty(&config)
                .map_err(|e| format!("Failed to serialize config: {}", e))?;

            std::fs::write(&config_path, new_content)
                .map_err(|e| format!("Failed to write config file: {}", e))?;

            info!(
                "[remove_server_from_config] Removed server '{}' from space '{}'",
                server_id, space_id
            );
            return Ok(true);
        }
    }

    Ok(false)
}

/// Refresh the system tray menu to reflect current spaces
#[tauri::command]
pub async fn refresh_tray_menu(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    tray::update_tray_spaces(&app, &state)
        .await
        .map_err(|e| format!("Failed to update tray menu: {}", e))
}
