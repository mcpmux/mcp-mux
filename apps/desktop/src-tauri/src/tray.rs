//! System tray implementation for McpMux
//!
//! Provides a system tray icon with quick access to:
//! - Space switching
//! - Open main window
//! - Quit application

use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime,
};
use tracing::{debug, info};

use crate::state::AppState;

/// Tray icon status
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    /// All systems healthy
    Healthy,
    /// Some warnings present
    Warning,
    /// Errors present
    Error,
    /// Offline or disabled
    Offline,
}

/// Build the system tray for the application
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    info!("Setting up system tray...");

    let menu = build_tray_menu(app)?;

    // Load tray icon - decode PNG and convert to RGBA
    let icon_bytes = include_bytes!("../icons/32x32.png");
    let img = image::load_from_memory(icon_bytes)
        .map_err(|e| {
            tauri::Error::InvalidIcon(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?
        .to_rgba8();
    let (width, height) = img.dimensions();
    let icon = Image::new_owned(img.into_raw(), width, height);

    let _tray = TrayIconBuilder::with_id("mcpmux-tray")
        .tooltip("McpMux - MCP Server Manager")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // Left click - show main window
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    info!("System tray initialized");
    Ok(())
}

/// Build the tray menu
fn build_tray_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    // Space submenu — pure navigation. Clicking a space opens the main
    // window and asks the frontend to switch to that space's view.
    let space_submenu = SubmenuBuilder::new(app, "Switch Space")
        .text("space_default", "🌐 Default")
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&space_submenu)
        .separator()
        .text("open", "Open McpMux")
        .separator()
        .text("quit", "Quit")
        .build()?;

    Ok(menu)
}

/// Handle menu events
fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event_id: &str) {
    debug!("Tray menu event: {}", event_id);

    match event_id {
        // Space switching
        id if id.starts_with("space_") => {
            let space_id = id.strip_prefix("space_").unwrap_or("default");
            handle_switch_space(app, space_id);
        }
        "open" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "quit" => {
            info!("Quit requested from tray");
            app.exit(0);
        }
        _ => {
            debug!("Unknown menu event: {}", event_id);
        }
    }
}

/// Switch to a different space
fn handle_switch_space<R: Runtime>(app: &AppHandle<R>, space_id: &str) {
    info!("Switching to space: {}", space_id);

    // Show window and emit event to frontend
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("tray:switch-space", space_id);
}

/// Update tray menu with current spaces
pub async fn update_tray_spaces<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
) -> tauri::Result<()> {
    let spaces = state.space_service.list().await.unwrap_or_default();
    let default_space = state.space_service.get_default().await.ok().flatten();

    if let Some(tray) = app.tray_by_id("mcpmux-tray") {
        let mut space_menu = SubmenuBuilder::new(app, "Switch Space");

        for space in spaces {
            let icon = space.icon.clone().unwrap_or_else(|| "🌐".to_string());
            // Tag the system default Space so the user can tell which one
            // catches sessions whose reported root has no binding.
            let is_default = default_space
                .as_ref()
                .map(|d| d.id == space.id)
                .unwrap_or(false);
            let suffix = if is_default { " · default" } else { "" };
            let label = format!("{} {}{}", icon, space.name, suffix);
            let id = format!("space_{}", space.id);
            space_menu = space_menu.text(id, label);
        }

        let space_submenu = space_menu.build()?;

        let menu = MenuBuilder::new(app)
            .item(&space_submenu)
            .separator()
            .text("open", "Open McpMux")
            .separator()
            .text("quit", "Quit")
            .build()?;

        tray.set_menu(Some(menu))?;
    }

    Ok(())
}

/// Update tray icon based on status
#[allow(dead_code)]
pub fn update_tray_status<R: Runtime>(app: &AppHandle<R>, status: TrayStatus) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("mcpmux-tray") {
        let tooltip = match status {
            TrayStatus::Healthy => "McpMux - All systems healthy",
            TrayStatus::Warning => "McpMux - Some warnings",
            TrayStatus::Error => "McpMux - Errors present",
            TrayStatus::Offline => "McpMux - Offline",
        };
        tray.set_tooltip(Some(tooltip))?;
    }
    Ok(())
}
