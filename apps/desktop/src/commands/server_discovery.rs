use tauri::State;
use mcpmux_core::service::server_discovery::ServerDiscoveryService;
use mcpmux_core::domain::server::ServerDefinition;
use mcpmux_core::application::ServerAppService;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tauri::command]
pub async fn discover_servers(
    service: State<'_, Arc<ServerDiscoveryService>>,
) -> Result<Vec<ServerDefinition>, String> {
    service.refresh().await.map_err(|e| e.to_string())?;
    Ok(service.list().await)
}

#[tauri::command]
pub async fn get_server_definition(
    service: State<'_, Arc<ServerDiscoveryService>>,
    id: String,
) -> Result<Option<ServerDefinition>, String> {
    Ok(service.get(&id).await)
}

#[tauri::command]
pub async fn toggle_server_enabled(
    app_service: State<'_, Arc<RwLock<Option<ServerAppService>>>>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let service = app_service.read().await;
    let service = service.as_ref().ok_or("ServerAppService not initialized")?;
    
    // This connects the UI action to the DB state update
    service.set_enabled(&id, enabled).await.map_err(|e| e.to_string())
}
