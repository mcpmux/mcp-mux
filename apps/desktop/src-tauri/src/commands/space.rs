//! Space management commands
//!
//! IPC commands for managing spaces (isolated environments). There's no
//! "active space" — gateway routing is decided per reported workspace
//! root via `WorkspaceBinding`, with the `is_default` Space as the
//! built-in fallback. The desktop UI tracks which space the user is
//! viewing in its own Zustand store (frontend-only state).

use mcpmux_core::{
    application::ServerAppService, validate_workspace_root, InstalledServer, Space, SpaceBaseDir,
    UserServerEntry, WorkspaceRootValidation,
};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::commands::gateway::GatewayAppState;
use crate::state::AppState;
use crate::tray;

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
    let config_path = state.space_config_path(&space.id.to_string())?;

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

    // Update system tray menu to show the new space
    // Only reached if both space creation and config file writing succeeded
    if let Err(e) = tray::update_tray_spaces(&app, &state).await {
        warn!("Failed to update tray menu: {}", e);
    }

    info!("[create_space] Space '{}' created successfully", space.name);

    Ok(space)
}

/// Update a space's display metadata (name, icon, description).
#[tauri::command]
pub async fn update_space(
    id: String,
    name: Option<String>,
    icon: Option<String>,
    description: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Space, String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    let space = state
        .space_service
        .update(uuid, name, icon, description)
        .await
        .map_err(|e| e.to_string())?;

    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        gw.emit_domain_event(mcpmux_core::DomainEvent::SpaceUpdated {
            space_id: uuid,
            name: space.name.clone(),
        });
    }

    // Update system tray menu to reflect the rename
    if let Err(e) = tray::update_tray_spaces(&app, &state).await {
        warn!("Failed to update tray menu: {}", e);
    }

    info!("[update_space] Space '{}' updated successfully", uuid);

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

    // Update system tray menu to remove the deleted space
    // Only reached if space deletion from DB succeeded
    if let Err(e) = tray::update_tray_spaces(&app, &state).await {
        warn!("Failed to update tray menu: {}", e);
    }

    info!("[delete_space] Space '{}' deleted successfully", uuid);

    Ok(())
}

/// Open space configuration file in external editor
#[tauri::command]
pub async fn open_space_config_file(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config_path = state.space_config_path(&space_id)?;

    if !config_path.exists() {
        return Err(format!(
            "Space config file not found: {}",
            config_path.display()
        ));
    }

    // Open with the OS default handler via the opener plugin — never via a
    // shell. The previous `cmd /C start <path>` form let cmd.exe interpret
    // metacharacters in the (then-unvalidated) path: OS command injection.
    tauri_plugin_opener::open_path(&config_path, None::<&str>)
        .map_err(|e| format!("Failed to open file: {}", e))
}

/// Read space configuration file
#[tauri::command]
pub async fn read_space_config(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let config_path = state.space_config_path(&space_id)?;

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
    let config_path = state.space_config_path(&space_id)?;

    // Validate JSON before saving
    serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| format!("Invalid JSON: {}", e))?;

    std::fs::write(&config_path, content)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    Ok(())
}

/// Remove a server from the space configuration file
#[tauri::command]
pub async fn remove_server_from_config(
    space_id: String,
    server_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let config_path = state.space_config_path(&space_id)?;

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

/// Replace a custom server's entry in the space configuration file.
///
/// Matches the target `mcpServers` key by comparing its normalized form
/// (see `UserServerEntry::normalize_server_id`) against `server_id`, since
/// the installed server id is normalized but the raw JSON key may not be.
#[tauri::command]
pub async fn update_server_in_config(
    space_id: String,
    server_id: String,
    entry: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !entry.is_object() {
        return Err("Server entry must be a JSON object".to_string());
    }

    let config_path = state.space_config_path(&space_id)?;

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config file: {}", e))?;

    let mut config: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

    let servers = config
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| "Config file has no mcpServers object".to_string())?;

    let matching_key = servers
        .keys()
        .find(|key| UserServerEntry::normalize_server_id(key) == server_id)
        .cloned()
        .ok_or_else(|| format!("Server '{}' not found in config", server_id))?;

    servers.insert(matching_key, entry);

    let new_content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    std::fs::write(&config_path, new_content)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    info!(
        "[update_server_in_config] Updated server '{}' in space '{}'",
        server_id, space_id
    );

    Ok(())
}

/// Persist a manual-entry clone's definition to `installed_servers.cached_definition`.
#[tauri::command]
pub async fn update_cloned_server_definition(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    space_id: String,
    server_id: String,
    entry: serde_json::Value,
) -> Result<InstalledServer, String> {
    if !entry.is_object() {
        return Err("Server entry must be a JSON object".to_string());
    }

    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .update_definition(space_uuid, &server_id, entry)
        .await
        .map_err(|e| e.to_string())
}

/// Refresh the system tray menu to reflect current spaces
#[tauri::command]
pub async fn refresh_tray_menu(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    tray::update_tray_spaces(&app, &state)
        .await
        .map_err(|e| format!("Failed to update tray menu: {}", e))
}

// ---------------------------------------------------------------------------
// Space base directories — scope a workspace root to a Space by folder prefix.
// A reported root at or under a base dir falls back to that Space's Starter
// (and scopes the meta-tools / mapping popup to it). Takes effect on a
// connected client's next request.
// ---------------------------------------------------------------------------

/// List a Space's configured base directories.
#[tauri::command]
pub async fn list_space_base_dirs(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<SpaceBaseDir>, String> {
    let uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;
    state
        .space_base_dir_repository
        .list_by_space(&uuid)
        .await
        .map_err(|e| e.to_string())
}

/// Add a base directory to a Space. The path is validated (must be an absolute
/// folder) and normalized before storing; an error is returned if it's already
/// claimed by another Space.
#[tauri::command]
pub async fn add_space_base_dir(
    space_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<SpaceBaseDir, String> {
    let uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    let normalized = match validate_workspace_root(&path) {
        WorkspaceRootValidation::Ok { normalized } => normalized,
        WorkspaceRootValidation::Empty => return Err("Pick a folder first.".to_string()),
        WorkspaceRootValidation::Invalid { reason } => return Err(reason),
    };

    info!(
        "[add_space_base_dir] space={} path={} (normalized {})",
        space_id, path, normalized
    );
    state
        .space_base_dir_repository
        .add(&uuid, &normalized)
        .await
        .map_err(|e| e.to_string())
}

/// Remove a base directory (by its row id).
#[tauri::command]
pub async fn remove_space_base_dir(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .space_base_dir_repository
        .remove(&id)
        .await
        .map_err(|e| e.to_string())
}
