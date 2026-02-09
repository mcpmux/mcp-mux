//! Server management commands

use crate::AppState;
use mcpmux_core::application::ServerAppService;
use mcpmux_core::domain::InstalledServer;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

#[tauri::command]
pub async fn install_server(
    state: State<'_, AppState>,
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    id: String,
    space_id: String,
) -> Result<InstalledServer, String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    // Refresh and lookup server definition from server discovery service
    state
        .server_discovery
        .refresh_if_needed()
        .await
        .map_err(|e| e.to_string())?;
    let definition = state
        .server_discovery
        .get(&id)
        .await
        .ok_or("Server definition not found")?;

    // Pass the full definition for caching (offline support)
    service
        .install(space_uuid, &id, &definition, HashMap::new())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn uninstall_server(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    id: String,
    space_id: String,
) -> Result<(), String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .uninstall(space_uuid, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_installed_servers(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    space_id: Option<String>,
) -> Result<Vec<InstalledServer>, String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    if let Some(sid) = space_id {
        service
            .list_for_space(&sid)
            .await
            .map_err(|e| e.to_string())
    } else {
        service.list().await.map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn set_server_enabled(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    id: String,
    enabled: bool,
    space_id: String,
) -> Result<(), String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    if enabled {
        service
            .enable(space_uuid, &id)
            .await
            .map_err(|e| e.to_string())
    } else {
        service
            .disable(space_uuid, &id)
            .await
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn set_server_oauth_connected(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    id: String,
    connected: bool,
    space_id: String,
) -> Result<(), String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .set_oauth_connected(space_uuid, &id, connected)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_server_inputs(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    id: String,
    input_values: HashMap<String, String>,
    space_id: String,
    env_overrides: Option<HashMap<String, String>>,
    args_append: Option<Vec<String>>,
    extra_headers: Option<HashMap<String, String>>,
) -> Result<InstalledServer, String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .update_config(
            space_uuid,
            &id,
            input_values,
            env_overrides,
            args_append,
            extra_headers,
        )
        .await
        .map_err(|e| e.to_string())
}
