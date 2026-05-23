//! Server clone commands

use mcpmux_core::application::ServerAppService;
use mcpmux_core::domain::InstalledServer;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Clone an installed server into a new suffixed manual-entry install in the same space.
#[tauri::command]
pub async fn clone_server(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    space_id: String,
    source_server_id: String,
    suffix: String,
    alias: Option<String>,
) -> Result<InstalledServer, String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .clone_server(
            space_uuid,
            &source_server_id,
            &suffix,
            alias.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Return whether a suffixed clone ID is available in the given space.
#[tauri::command]
pub async fn is_clone_id_available(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    space_id: String,
    source_server_id: String,
    suffix: String,
) -> Result<bool, String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .is_clone_id_available(space_uuid, &source_server_id, &suffix)
        .await
        .map_err(|e| e.to_string())
}

/// Suggest the first available default suffix for cloning a server.
#[tauri::command]
pub async fn suggest_clone_suffix(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    space_id: String,
    source_server_id: String,
) -> Result<String, String> {
    let service_lock = app_service.read().await;
    let service = service_lock
        .as_ref()
        .ok_or("ServerAppService not initialized")?;

    let space_uuid = uuid::Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    service
        .suggest_clone_suffix(space_uuid, &source_server_id)
        .await
        .map_err(|e| e.to_string())
}
