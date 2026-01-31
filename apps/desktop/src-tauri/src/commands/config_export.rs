//! Config export commands
//!
//! IPC commands for generating MCP configuration files for clients.

use mcpmux_core::{ConfigExporter, ConfigFormat, ResolvedTransport, ResolvedServer, TransportConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::State;

use crate::state::AppState;

/// Request for exporting configuration
#[derive(Debug, Deserialize)]
pub struct ExportConfigRequest {
    /// Client type: "cursor", "vscode", "claude"
    pub client_type: String,
    /// Space ID to export config for (use "default" for default space)
    pub space_id: String,
    /// Whether to mask credentials
    #[serde(default)]
    pub mask_credentials: bool,
}

/// Response for config export
#[derive(Debug, Serialize)]
pub struct ExportConfigResponse {
    /// Generated JSON config
    pub content: String,
    /// Default file path for this format
    pub default_path: Option<String>,
    /// File name suggestion
    pub suggested_filename: String,
}

/// Get the config format from client type
fn get_format(client_type: &str) -> Result<ConfigFormat, String> {
    match client_type.to_lowercase().as_str() {
        "cursor" => Ok(ConfigFormat::Cursor),
        "vscode" | "vscode-continue" | "continue" => Ok(ConfigFormat::VsCodeContinue),
        "claude" | "claude-desktop" => Ok(ConfigFormat::ClaudeDesktop),
        _ => Err(format!("Unknown client type: {}", client_type)),
    }
}

/// Get the space ID (resolves "default" to active space)
async fn get_space_id(state: &AppState, space_id: &str) -> Result<String, String> {
    if space_id == "default" || space_id.is_empty() {
        let space = state
            .space_service
            .get_active()
            .await
            .map_err(|e: anyhow::Error| e.to_string())?
            .ok_or("No active space found")?;
        Ok(space.id.to_string())
    } else {
        Ok(space_id.to_string())
    }
}

/// Build resolved servers from installed servers (using cached definitions)
async fn build_resolved_servers(
    state: &AppState,
    space_id: &str,
    mask_credentials: bool,
) -> Result<Vec<ResolvedServer>, String> {
    // Get installed servers for this space
    let installed = state
        .installed_server_repository
        .list_enabled(space_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut resolved = Vec::new();

    for inst in installed {
        // Use cached definition (offline-first)
        if let Some(entry) = inst.get_definition() {
            // Build resolved transport from registry transport + input values
            let transport = match &entry.transport {
                TransportConfig::Stdio { command, args, env, .. } => {
                    // Resolve placeholders in command
                    let resolved_command = resolve_placeholders(command, &inst.input_values);
                    
                    // Resolve placeholders in args
                    let mut resolved_args: Vec<String> = args
                        .iter()
                        .map(|arg| resolve_placeholders(arg, &inst.input_values))
                        .collect();
                    
                    // Append user's extra args
                    resolved_args.extend(inst.args_append.clone());

                    // Build env from registry + input values + env_overrides
                    let mut resolved_env = HashMap::new();
                    
                    // 1. Start with registry env
                    for (k, v) in env {
                        resolved_env.insert(k.clone(), resolve_placeholders(v, &inst.input_values));
                    }
                    
                    // 2. Add input values (for api_key type servers)
                    if !mask_credentials {
                        resolved_env.extend(inst.input_values.clone());
                    } else {
                        for k in inst.input_values.keys() {
                            resolved_env.insert(k.clone(), "***MASKED***".to_string());
                        }
                    }
                    
                    // 3. Apply user's env overrides
                    resolved_env.extend(inst.env_overrides.clone());

                    ResolvedTransport::Stdio {
                        command: resolved_command,
                        args: resolved_args,
                        env: resolved_env,
                    }
                }
                TransportConfig::Http { url, headers, .. } => {
                    let resolved_url = resolve_placeholders(url, &inst.input_values);
                    
                    // Resolve headers from registry
                    let mut resolved_headers: HashMap<String, String> = headers
                        .iter()
                        .map(|(k, v)| (k.clone(), resolve_placeholders(v, &inst.input_values)))
                        .collect();
                    
                    // Add user's extra headers
                    resolved_headers.extend(inst.extra_headers.clone());
                    
                    ResolvedTransport::Http {
                        url: resolved_url,
                        headers: resolved_headers,
                    }
                }
            };

            resolved.push(ResolvedServer {
                server_id: inst.server_id.clone(),
                transport,
            });
        }
    }

    Ok(resolved)
}

/// Resolve ${input:xxx} placeholders in a string
fn resolve_placeholders(template: &str, input_values: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in input_values {
        let placeholder = format!("${{input:{}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Preview config export (returns JSON string)
#[tauri::command]
pub async fn preview_config_export(
    request: ExportConfigRequest,
    state: State<'_, AppState>,
) -> Result<ExportConfigResponse, String> {
    let space_id = get_space_id(&state, &request.space_id).await?;
    let format = get_format(&request.client_type)?;

    // Build resolved servers
    let servers = build_resolved_servers(&state, &space_id, request.mask_credentials).await?;

    // Create exporter and generate config
    let exporter = ConfigExporter::new();
    let content = exporter
        .export_json(format, &servers)
        .map_err(|e| e.to_string())?;

    let default_path = format
        .default_path()
        .map(|p| p.to_string_lossy().to_string());

    let suggested_filename = match format {
        ConfigFormat::Cursor => "mcp.json".to_string(),
        ConfigFormat::VsCodeContinue => "continue-mcp.json".to_string(),
        ConfigFormat::ClaudeDesktop => "claude_desktop_config.json".to_string(),
    };

    Ok(ExportConfigResponse {
        content,
        default_path,
        suggested_filename,
    })
}

/// Export config to file
#[tauri::command]
pub async fn export_config_to_file(
    request: ExportConfigRequest,
    path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let space_id = get_space_id(&state, &request.space_id).await?;
    let format = get_format(&request.client_type)?;

    // Build resolved servers (with actual credentials for file export)
    let servers = build_resolved_servers(&state, &space_id, false).await?;

    // Create exporter and generate config
    let exporter = ConfigExporter::new();
    let content = exporter
        .export_json(format, &servers)
        .map_err(|e| e.to_string())?;

    // Write to file
    let path = PathBuf::from(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
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
