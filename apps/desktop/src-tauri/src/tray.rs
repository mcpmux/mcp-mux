//! System tray implementation for McpMux
//!
//! Provides a system tray icon with quick access to:
//! - Space switching
//! - Config export
//! - Server status
//! - Open main window

use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
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

    let _tray = TrayIconBuilder::with_id("mcpmux-tray")
        .tooltip("McpMux - MCP Server Manager")
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
    // Space submenu
    let space_submenu = SubmenuBuilder::new(app, "Active Space")
        .text("space_default", "🌐 Default")
        .separator()
        .text("create_space", "➕ Create Space...")
        .build()?;

    // Export submenu
    let export_submenu = SubmenuBuilder::new(app, "📋 Export Config")
        .text("export_cursor", "Cursor")
        .text("export_vscode", "VS Code")
        .text("export_claude", "Claude Desktop")
        .build()?;

    // Build main menu
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("status", "McpMux 🟢").enabled(false).build(app)?)
        .separator()
        .item(&space_submenu)
        .separator()
        .text("refresh", "🔄 Refresh All Servers")
        .item(&export_submenu)
        .separator()
        .text("open", "⚙️ Open McpMux")
        .item(&PredefinedMenuItem::separator(app)?)
        .text("quit", "❌ Quit")
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
        "create_space" => {
            open_main_window_at(app, "/spaces/new");
        }

        // Export actions
        "export_cursor" => {
            handle_export(app, "cursor");
        }
        "export_vscode" => {
            handle_export(app, "vscode");
        }
        "export_claude" => {
            handle_export(app, "claude");
        }

        // General actions
        "refresh" => {
            handle_refresh_servers(app);
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

    // Emit event to frontend
    let _ = app.emit("tray:switch-space", space_id);
}

/// Open main window at a specific route
fn open_main_window_at<R: Runtime>(app: &AppHandle<R>, route: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        // Emit navigation event
        let _ = app.emit("tray:navigate", route);
    }
}

/// Handle export request
fn handle_export<R: Runtime>(app: &AppHandle<R>, client_type: &str) {
    info!("Export config requested for: {}", client_type);

    // Emit event to frontend to handle export
    let _ = app.emit("tray:export-config", client_type);
}

/// Handle refresh all servers
fn handle_refresh_servers<R: Runtime>(app: &AppHandle<R>) {
    info!("Refresh all servers requested");

    // Emit event to frontend
    let _ = app.emit("tray:refresh-servers", ());
}

/// Update tray menu with current spaces
#[allow(dead_code)]
pub async fn update_tray_spaces<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
) -> tauri::Result<()> {
    let spaces = state.space_service.list().await.unwrap_or_default();
    let active_space = state.space_service.get_active().await.ok().flatten();

    // Get tray handle
    if let Some(tray) = app.tray_by_id("mcpmux-tray") {
        // Rebuild space submenu
        let mut space_menu = SubmenuBuilder::new(app, "Active Space");

        for space in spaces {
            let icon = space.icon.clone().unwrap_or_else(|| "🌐".to_string());
            let is_active = active_space.as_ref().map(|a| a.id == space.id).unwrap_or(false);
            let check = if is_active { "✓ " } else { "  " };
            let label = format!("{}{} {}", check, icon, space.name);
            let id = format!("space_{}", space.id);
            space_menu = space_menu.text(id, label);
        }

        space_menu = space_menu.separator().text("create_space", "➕ Create Space...");

        let space_submenu = space_menu.build()?;

        // Rebuild full menu
        let export_submenu = SubmenuBuilder::new(app, "📋 Export Config")
            .text("export_cursor", "Cursor")
            .text("export_vscode", "VS Code")
            .text("export_claude", "Claude Desktop")
            .build()?;

        let menu = MenuBuilder::new(app)
            .item(
                &MenuItemBuilder::with_id("status", "McpMux 🟢")
                    .enabled(false)
                    .build(app)?,
            )
            .separator()
            .item(&space_submenu)
            .separator()
            .text("refresh", "🔄 Refresh All Servers")
            .item(&export_submenu)
            .separator()
            .text("open", "⚙️ Open McpMux")
            .item(&PredefinedMenuItem::separator(app)?)
            .text("quit", "❌ Quit")
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
