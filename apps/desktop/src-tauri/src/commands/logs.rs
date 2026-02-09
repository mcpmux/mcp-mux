//! Tauri commands for server log management

use crate::state::AppState;
use mcpmux_core::{AppSettingsService, LogLevel, ServerLog};
use serde::Serialize;
use tauri::State;
use tracing::{info, warn};

/// Helper to get the default space ID
async fn get_default_space_id(state: &AppState) -> Result<String, String> {
    let space = state
        .space_service
        .get_active()
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or("No active space found")?;
    Ok(space.id.to_string())
}

/// Server log entry for frontend
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerLogEntry {
    pub timestamp: String,
    pub level: String,
    pub source: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl From<ServerLog> for ServerLogEntry {
    fn from(log: ServerLog) -> Self {
        Self {
            timestamp: log.timestamp.to_rfc3339(),
            level: log.level.as_str().to_string(),
            source: log.source.as_str().to_string(),
            message: log.message,
            metadata: log.metadata,
        }
    }
}

/// Get recent logs for a server
#[tauri::command]
pub async fn get_server_logs(
    server_id: String,
    limit: Option<usize>,
    level_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ServerLogEntry>, String> {
    info!(
        "[Logs] Getting logs for server {} (limit: {:?}, filter: {:?})",
        server_id, limit, level_filter
    );

    let space_id = get_default_space_id(&state).await?;

    // Parse level filter
    let level = level_filter.and_then(|s| LogLevel::parse(&s));

    // Get logs
    let logs = state
        .server_log_manager
        .read_logs(&space_id, &server_id, limit.unwrap_or(100), level)
        .await
        .map_err(|e| {
            warn!("[Logs] Failed to read logs for {}: {}", server_id, e);
            format!("Failed to read logs: {}", e)
        })?;

    Ok(logs.into_iter().map(ServerLogEntry::from).collect())
}

/// Clear logs for a server
#[tauri::command]
pub async fn clear_server_logs(
    server_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("[Logs] Clearing logs for server {}", server_id);

    let space_id = get_default_space_id(&state).await?;

    state
        .server_log_manager
        .clear_logs(&space_id, &server_id)
        .await
        .map_err(|e| {
            warn!("[Logs] Failed to clear logs for {}: {}", server_id, e);
            format!("Failed to clear logs: {}", e)
        })?;

    info!("[Logs] Cleared logs for server {}", server_id);
    Ok(())
}

/// Get log file path for a server (for external viewers)
#[tauri::command]
pub async fn get_server_log_file(
    server_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let space_id = get_default_space_id(&state).await?;

    let path = state.server_log_manager.get_log_file(&space_id, &server_id);

    Ok(path.to_string_lossy().to_string())
}

/// Get log retention period in days (0 = keep forever)
#[tauri::command]
pub async fn get_log_retention_days(state: State<'_, AppState>) -> Result<u32, String> {
    let settings = AppSettingsService::new(state.settings_repository.clone());
    Ok(settings.get_log_retention_days().await)
}

/// Set log retention period in days (0 = keep forever)
#[tauri::command]
pub async fn set_log_retention_days(days: u32, state: State<'_, AppState>) -> Result<(), String> {
    info!("[Logs] Setting log retention to {} days", days);

    let settings = AppSettingsService::new(state.settings_repository.clone());
    settings
        .set_log_retention_days(days)
        .await
        .map_err(|e| format!("Failed to save log retention setting: {}", e))?;

    // Run cleanup immediately with the new setting if retention is enabled
    if days > 0 {
        match state.server_log_manager.cleanup_logs_older_than(days).await {
            Ok(n) if n > 0 => info!("[Logs] Cleaned up {} old log file(s)", n),
            Ok(_) => {}
            Err(e) => warn!("[Logs] Cleanup after setting change failed: {}", e),
        }
    }

    Ok(())
}
