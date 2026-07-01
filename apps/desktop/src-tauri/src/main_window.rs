//! Main window show/hide helpers shared by tray, deep links, and gateway focus.

use tauri::{AppHandle, Manager, Runtime};

use crate::macos_dock;

/// Unminimize, show, and focus the main window; restore Dock presence on macOS.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    macos_dock::set_dock_visible(app, true);
}

/// Hide the main window to the tray and remove Dock presence on macOS.
pub fn hide_main_window_to_tray<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    macos_dock::set_dock_visible(app, false);
}
