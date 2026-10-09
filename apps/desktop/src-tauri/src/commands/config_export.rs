//! Config export commands
//!
//! IPC commands that locate and back up AI clients' MCP configuration files.
//! Generating a config with resolved credentials is deliberately not exposed
//! over IPC: nothing in the UI needs it, and it would hand out every secret.

use mcpmux_core::ConfigFormat;
use std::collections::HashMap;

/// Get the config format from client type
fn get_format(client_type: &str) -> Result<ConfigFormat, String> {
    match client_type.to_lowercase().as_str() {
        "cursor" => Ok(ConfigFormat::Cursor),
        "vscode" | "vscode-continue" | "continue" => Ok(ConfigFormat::VsCodeContinue),
        "claude" | "claude-desktop" => Ok(ConfigFormat::ClaudeDesktop),
        _ => Err(format!("Unknown client type: {}", client_type)),
    }
}

/// Get default config paths for all clients
#[tauri::command]
pub async fn get_config_paths() -> Result<HashMap<String, Option<String>>, String> {
    let mut paths = HashMap::new();

    paths.insert(
        "cursor".to_string(),
        ConfigFormat::Cursor
            .default_path()
            .map(|p| p.to_string_lossy().to_string()),
    );
    paths.insert(
        "vscode".to_string(),
        ConfigFormat::VsCodeContinue
            .default_path()
            .map(|p| p.to_string_lossy().to_string()),
    );
    paths.insert(
        "claude".to_string(),
        ConfigFormat::ClaudeDesktop
            .default_path()
            .map(|p| p.to_string_lossy().to_string()),
    );

    Ok(paths)
}

/// Check if config file exists at default location
#[tauri::command]
pub async fn check_config_exists(client_type: String) -> Result<bool, String> {
    let format = get_format(&client_type)?;

    match format.default_path() {
        Some(path) => Ok(path.exists()),
        None => Ok(false),
    }
}

/// Backup existing config before writing
#[tauri::command]
pub async fn backup_existing_config(client_type: String) -> Result<Option<String>, String> {
    let format = get_format(&client_type)?;

    match format.default_path() {
        Some(path) if path.exists() => {
            let backup_path = path.with_extension("json.bak");
            std::fs::copy(&path, &backup_path).map_err(|e| e.to_string())?;
            Ok(Some(backup_path.to_string_lossy().to_string()))
        }
        _ => Ok(None),
    }
}
