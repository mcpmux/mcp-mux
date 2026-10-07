//! Tauri commands for server log management

use crate::state::AppState;
use mcpmux_core::{AppSettingsService, LogLevel, ServerLog};
use serde::Serialize;
use tauri::State;
use tracing::{info, warn};
use uuid::Uuid;

/// Logs are written per Space (the gateway tags each entry with the Space the
/// server resolved in), so every read has to name that Space. The id arrives
/// over IPC and lands in a filesystem path, hence the UUID guard.
fn parse_space_id(space_id: &str) -> Result<String, String> {
    Uuid::parse_str(space_id)
        .map_err(|_| "Invalid Space id".to_string())
        .map(|uuid| uuid.to_string())
}

/// The server id is the other half of that path, and clearing logs deletes the
/// directory it names, so it has to be a single plain path segment.
fn check_server_id(server_id: &str) -> Result<(), String> {
    if server_id.is_empty()
        || server_id == "."
        || server_id == ".."
        || server_id.contains(['/', '\\'])
    {
        return Err("Invalid server id".to_string());
    }
    Ok(())
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
    space_id: String,
    limit: Option<usize>,
    level_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ServerLogEntry>, String> {
    info!(
        "[Logs] Getting logs for server {} in space {} (limit: {:?}, filter: {:?})",
        server_id, space_id, limit, level_filter
    );

    let space_id = parse_space_id(&space_id)?;
    check_server_id(&server_id)?;

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
    space_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(
        "[Logs] Clearing logs for server {} in space {}",
        server_id, space_id
    );

    let space_id = parse_space_id(&space_id)?;
    check_server_id(&server_id)?;

    state
        .server_log_manager
        .clear_logs(&space_id, &server_id)
        .await
        .map_err(|e| {
            warn!("[Logs] Failed to clear logs for {}: {}", server_id, e);
            format!("Failed to clear logs: {}", e)
        })?;

    info!(
        "[Logs] Cleared logs for server {} in space {}",
        server_id, space_id
    );
    Ok(())
}

/// Get log file path for a server (for external viewers)
#[tauri::command]
pub async fn get_server_log_file(
    server_id: String,
    space_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let space_id = parse_space_id(&space_id)?;
    check_server_id(&server_id)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_id_is_accepted_and_normalized() {
        assert_eq!(
            parse_space_id("02233890-a6ee-4b1e-aea3-fca8e3b4c09d").unwrap(),
            "02233890-a6ee-4b1e-aea3-fca8e3b4c09d"
        );
        // Braced / uppercase forms normalize to the canonical hyphenated form
        // so the value used as a path segment is stable.
        assert_eq!(
            parse_space_id("{02233890-A6EE-4B1E-AEA3-FCA8E3B4C09D}").unwrap(),
            "02233890-a6ee-4b1e-aea3-fca8e3b4c09d"
        );
    }

    #[test]
    fn a_space_id_that_is_not_a_uuid_is_rejected() {
        // The id becomes a directory name under the logs dir, so anything that
        // could climb out of it must never reach the log manager.
        for candidate in [
            "..",
            "../../../Windows",
            "00000000-0000-0000-0000-000000000001/../..",
            "not-a-uuid",
            "",
        ] {
            assert!(
                parse_space_id(candidate).is_err(),
                "expected {candidate:?} to be rejected"
            );
        }
    }

    #[test]
    fn registry_and_custom_server_ids_are_accepted() {
        for id in [
            "com.cloudflare-docs",
            "com.cloudflare:docs",
            "github-server",
        ] {
            assert!(
                check_server_id(id).is_ok(),
                "expected {id:?} to be accepted"
            );
        }
    }

    #[test]
    fn a_server_id_that_is_not_one_path_segment_is_rejected() {
        // Clearing logs removes <logs>/<space>/<server> recursively, so a
        // server id that names a parent or another directory must never get
        // that far.
        for candidate in [
            "",
            ".",
            "..",
            "../02233890-a6ee-4b1e-aea3-fca8e3b4c09d",
            "../../..",
            "a/b",
            "/etc",
            "..\\..\\Windows",
            "a\\b",
        ] {
            assert!(
                check_server_id(candidate).is_err(),
                "expected {candidate:?} to be rejected"
            );
        }
    }
}
