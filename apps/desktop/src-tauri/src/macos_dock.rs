//! macOS Dock visibility for tray-only mode.

#[cfg(target_os = "macos")]
use tauri::{ActivationPolicy, AppHandle, Runtime};
#[cfg(target_os = "macos")]
use tracing::warn;

/// Show or hide the app in the macOS Dock (no-op on other platforms).
#[cfg(target_os = "macos")]
pub fn set_dock_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    let policy = if visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };
    if let Err(e) = app.set_activation_policy(policy) {
        warn!("[macOS] Failed to set activation policy (visible={visible}): {e}");
    }
    if let Err(e) = app.set_dock_visibility(visible) {
        warn!("[macOS] Failed to set dock visibility (visible={visible}): {e}");
    }
}

/// Show or hide the app in the macOS Dock (no-op on other platforms).
#[cfg(not(target_os = "macos"))]
pub fn set_dock_visible<R: tauri::Runtime>(_app: &tauri::AppHandle<R>, _visible: bool) {}
