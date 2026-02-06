//! Settings commands for auto-start and system tray behavior

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_autostart::AutoLaunchManager;
use tracing::{debug, error, info};

use crate::state::AppState;

/// Startup and system tray settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSettings {
    /// Whether to launch the app at system startup
    pub auto_launch: bool,
    /// Whether to start minimized to tray
    pub start_minimized: bool,
    /// Whether to minimize to tray instead of closing
    pub close_to_tray: bool,
}

impl Default for StartupSettings {
    fn default() -> Self {
        Self {
            auto_launch: false,
            start_minimized: false,
            close_to_tray: true, // Default to close-to-tray behavior
        }
    }
}

/// Get current startup settings
#[tauri::command]
pub async fn get_startup_settings(
    app_state: State<'_, AppState>,
    manager: State<'_, AutoLaunchManager>,
) -> Result<StartupSettings, String> {
    debug!("[Settings] Getting startup settings");

    let settings_repo = &app_state.settings_repository;

    // Get auto-launch status from the OS
    let auto_launch = manager
        .is_enabled()
        .await
        .map_err(|e| format!("Failed to check auto-launch status: {}", e))?;

    // Get other settings from database
    let start_minimized = settings_repo
        .get("startup.start_minimized")
        .await
        .map_err(|e| format!("Failed to get start_minimized setting: {}", e))?
        .map(|v| v == "true")
        .unwrap_or(false);

    let close_to_tray = settings_repo
        .get("ui.close_to_tray")
        .await
        .map_err(|e| format!("Failed to get close_to_tray setting: {}", e))?
        .map(|v| v == "true")
        .unwrap_or(true); // Default to true

    Ok(StartupSettings {
        auto_launch,
        start_minimized,
        close_to_tray,
    })
}

/// Update startup settings
#[tauri::command]
pub async fn update_startup_settings(
    settings: StartupSettings,
    app_state: State<'_, AppState>,
    manager: State<'_, AutoLaunchManager>,
) -> Result<(), String> {
    info!("[Settings] Updating startup settings: {:?}", settings);

    let settings_repo = &app_state.settings_repository;

    // Update auto-launch in OS
    if settings.auto_launch {
        manager
            .enable()
            .await
            .map_err(|e| format!("Failed to enable auto-launch: {}", e))?;
        info!("[Settings] Auto-launch enabled");
    } else {
        manager
            .disable()
            .await
            .map_err(|e| format!("Failed to disable auto-launch: {}", e))?;
        info!("[Settings] Auto-launch disabled");
    }

    // Update other settings in database
    settings_repo
        .set(
            "startup.start_minimized",
            &settings.start_minimized.to_string(),
        )
        .await
        .map_err(|e| format!("Failed to save start_minimized setting: {}", e))?;

    settings_repo
        .set("ui.close_to_tray", &settings.close_to_tray.to_string())
        .await
        .map_err(|e| format!("Failed to save close_to_tray setting: {}", e))?;

    info!("[Settings] Startup settings updated successfully");
    Ok(())
}

/// Check if app should start hidden (for auto-launch with --hidden flag)
pub fn should_start_hidden() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.contains(&"--hidden".to_string())
}
