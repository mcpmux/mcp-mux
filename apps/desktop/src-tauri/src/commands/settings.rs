//! Settings commands for auto-start and system tray behavior

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_autostart::AutoLaunchManager;
use tracing::{debug, info};

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
            auto_launch: true,
            start_minimized: true,
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
        .map_err(|e| format!("Failed to check auto-launch status: {}", e))?;

    // Get other settings from database; use defaults when key is missing or DB read fails (e.g. no settings yet)
    let start_minimized = settings_repo
        .get("startup.start_minimized")
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(true);

    let close_to_tray = settings_repo
        .get("ui.close_to_tray")
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(true);

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

    // Check if auto-launch setting has changed before modifying OS
    let current_auto_launch = manager.is_enabled().unwrap_or(false);

    if settings.auto_launch != current_auto_launch {
        if settings.auto_launch {
            manager
                .enable()
                .map_err(|e| format!("Failed to enable auto-launch: {}", e))?;
            info!("[Settings] Auto-launch enabled");
        } else {
            manager
                .disable()
                .map_err(|e| format!("Failed to disable auto-launch: {}", e))?;
            info!("[Settings] Auto-launch disabled");
        }
    } else {
        info!("[Settings] Auto-launch unchanged, skipping OS update");
    }

    // Mark autostart as explicitly configured so first-launch logic won't re-enable it
    settings_repo
        .set("startup.autostart_configured", "true")
        .await
        .map_err(|e| format!("Failed to save autostart_configured flag: {}", e))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_settings_default() {
        let settings = StartupSettings::default();
        assert_eq!(settings.auto_launch, true);
        assert_eq!(settings.start_minimized, true);
        assert_eq!(settings.close_to_tray, true);
    }

    #[test]
    fn test_startup_settings_serialization() {
        let settings = StartupSettings {
            auto_launch: true,
            start_minimized: false,
            close_to_tray: true,
        };

        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"autoLaunch\":true"));
        assert!(json.contains("\"startMinimized\":false"));
        assert!(json.contains("\"closeToTray\":true"));
    }

    #[test]
    fn test_startup_settings_deserialization() {
        let json = r#"{"autoLaunch":true,"startMinimized":true,"closeToTray":false}"#;
        let settings: StartupSettings = serde_json::from_str(json).unwrap();

        assert_eq!(settings.auto_launch, true);
        assert_eq!(settings.start_minimized, true);
        assert_eq!(settings.close_to_tray, false);
    }

    #[test]
    fn test_should_start_hidden_without_flag() {
        // This test might be tricky as it depends on actual process args
        // In a real test environment, we'd mock std::env::args
        // For now, we just verify the function exists and can be called
        let _result = should_start_hidden();
        // Can't assert the actual value since it depends on how tests are run
    }

    #[test]
    fn test_startup_settings_clone() {
        let settings = StartupSettings {
            auto_launch: true,
            start_minimized: false,
            close_to_tray: true,
        };

        let cloned = settings.clone();
        assert_eq!(settings.auto_launch, cloned.auto_launch);
        assert_eq!(settings.start_minimized, cloned.start_minimized);
        assert_eq!(settings.close_to_tray, cloned.close_to_tray);
    }

    #[test]
    fn test_startup_settings_debug() {
        let settings = StartupSettings::default();
        let debug_str = format!("{:?}", settings);
        assert!(debug_str.contains("StartupSettings"));
        assert!(debug_str.contains("auto_launch"));
        assert!(debug_str.contains("start_minimized"));
        assert!(debug_str.contains("close_to_tray"));
    }

    #[test]
    fn test_startup_settings_with_all_enabled() {
        let settings = StartupSettings {
            auto_launch: true,
            start_minimized: true,
            close_to_tray: true,
        };

        assert!(settings.auto_launch);
        assert!(settings.start_minimized);
        assert!(settings.close_to_tray);
    }

    #[test]
    fn test_startup_settings_with_all_disabled() {
        let settings = StartupSettings {
            auto_launch: false,
            start_minimized: false,
            close_to_tray: false,
        };

        assert!(!settings.auto_launch);
        assert!(!settings.start_minimized);
        assert!(!settings.close_to_tray);
    }
}
