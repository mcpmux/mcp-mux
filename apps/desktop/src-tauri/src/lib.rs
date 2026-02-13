//! McpMux Desktop Application
//!
//! Centralized MCP Server Management Desktop Application

use mcpmux_core::branding;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

mod commands;
mod services;
mod state;
mod tray;

// Re-export deep link handler
use commands::oauth::handle_deep_link;

use commands::gateway::GatewayAppState;
use commands::server_manager::ServerManagerState;
use state::AppState;

/// Application identifier - read from tauri.conf.json at build time
/// Single source of truth: tauri.conf.json -> build.rs -> env!()
const APP_IDENTIFIER: &str = env!("TAURI_APP_IDENTIFIER");

/// Get the app local data directory (same as Tauri's app_local_data_dir)
///
/// Uses Local (not Roaming) because our data is machine-specific:
/// - Database contains machine-specific server paths
/// - Logs are machine-specific
/// - Large files shouldn't roam in enterprise environments
///
/// Uses APP_IDENTIFIER from tauri.conf.json (via build.rs) for consistency.
/// - Windows: %LOCALAPPDATA%/<identifier>/
/// - macOS: ~/Library/Application Support/<identifier>/
/// - Linux: ~/.local/share/<identifier>/
fn get_app_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(APP_IDENTIFIER)
}

/// Get the logs directory path (under app data directory)
fn get_logs_dir() -> std::path::PathBuf {
    get_app_data_dir().join("logs")
}

/// Initialize tracing for the application with console and file logging
///
/// - Console: colored, compact format
/// - File: daily rotation in ~/.local/share/mcpmux/logs/ (Linux)
///   or %LOCALAPPDATA%/mcpmux/logs/ (Windows)
fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // Load .env file if present (for development)
    // Try multiple locations: current dir, parent dir (for tauri dev), src-tauri parent
    dotenvy::dotenv().ok();
    dotenvy::from_filename("../.env").ok(); // apps/desktop/.env when run from src-tauri

    let logs_dir = get_logs_dir();

    // Create logs directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("Warning: Failed to create logs directory: {}", e);
    }

    // File appender with daily rotation
    // Creates files like: mcpmux.2026-01-22.log
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(branding::LOG_PREFIX)
        .filename_suffix("log")
        .build(&logs_dir)
        .expect("Failed to create log file appender");
    let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);

    // Environment filter for log levels
    // RUST_LOG takes precedence, with sensible defaults for our crates
    // Note: Rust crate names use underscores in tracing (e.g., mcpmux-core → mcpmux_core)
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // Default filter when RUST_LOG is not set
        EnvFilter::new("info")
            .add_directive("mcpmux_core=debug".parse().unwrap())
            .add_directive("mcpmux_gateway=debug".parse().unwrap())
            .add_directive("mcpmux_storage=debug".parse().unwrap())
            .add_directive("mcpmux_mcp=debug".parse().unwrap())
            .add_directive("mcpmux_lib=debug".parse().unwrap())
            .add_directive("tauri=info".parse().unwrap())
            .add_directive("tao=warn".parse().unwrap())
            .add_directive("wry=warn".parse().unwrap())
    });

    // Console layer: colored, compact
    let console_layer = fmt::layer()
        .with_ansi(true)
        .compact()
        .with_thread_names(false)
        .with_line_number(false)
        .with_file(false)
        .with_target(true);

    // File layer: no colors, include more detail
    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .with_target(true);

    // Combine layers
    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    // Return guard - must be kept alive for the duration of the program
    guard
}

/// Get app version
#[tauri::command]
fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Get the path to the logs directory
#[tauri::command]
fn get_logs_path() -> String {
    get_logs_dir().to_string_lossy().to_string()
}

/// Open the logs directory in the system file explorer
#[tauri::command]
async fn open_logs_folder() -> Result<(), String> {
    let logs_dir = get_logs_dir();

    // Create directory if it doesn't exist
    if !logs_dir.exists() {
        std::fs::create_dir_all(&logs_dir)
            .map_err(|e| format!("Failed to create logs directory: {}", e))?;
    }

    // Open in system file explorer
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&logs_dir)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&logs_dir)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&logs_dir)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Keep the guard alive for the entire program - dropping it stops file logging
    let _log_guard = init_tracing();

    let logs_dir = get_logs_dir();
    info!(
        "Starting {} Desktop v{}",
        branding::DISPLAY_NAME,
        env!("CARGO_PKG_VERSION")
    );
    info!("Logs directory: {}", logs_dir.display());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]), // Start minimized to tray
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            // This callback is called when a second instance is launched
            info!("Second instance detected, focusing existing window");
            info!("Args: {:?}, CWD: {:?}", args, cwd);

            // Check if any arg is a deep link URL
            for arg in &args {
                if branding::is_deep_link(arg) {
                    info!("Deep link received via second instance: {}", arg);
                    handle_deep_link(app, arg);
                }
            }

            // Try to focus the main window
            if let Some(window) = app.get_webview_window("main") {
                // Show window if hidden
                if let Err(e) = window.show() {
                    warn!("Failed to show window: {}", e);
                }
                // Unminimize if minimized
                if let Err(e) = window.unminimize() {
                    warn!("Failed to unminimize window: {}", e);
                }
                // Focus the window
                if let Err(e) = window.set_focus() {
                    warn!("Failed to focus window: {}", e);
                }
            } else {
                warn!("Main window not found");
            }
        }))
        .setup(|app| {
            info!("Initializing application state...");

            // Get data directory (Local, not Roaming - machine-specific data)
            let data_dir = app
                .path()
                .app_local_data_dir()
                .expect("Failed to get app local data directory");
            let app_data_dir = data_dir.clone();

            // Create and manage application state
            let state = AppState::new(data_dir).map_err(|e| {
                error!("Failed to initialize application state: {}", e);
                e.to_string()
            })?;

            app.manage(state);

            // Create event bus and ServerAppService
            let app_state: tauri::State<'_, AppState> = app.state();
            let event_bus = mcpmux_core::create_shared_event_bus();
            let event_sender = event_bus.sender();

            let server_app_service = mcpmux_core::ServerAppService::new(
                app_state.installed_server_repository.clone(),
                Some(app_state.feature_set_repository.clone()),
                Some(app_state.server_feature_repository_core.clone()),
                Some(app_state.credential_repository.clone()),
                event_sender,
            );

            let managed_app_service = Arc::new(RwLock::new(Some(server_app_service)));
            app.manage(managed_app_service);

            // Create gateway state and auto-start gateway
            let gateway_state = Arc::new(RwLock::new(GatewayAppState::default()));

            // Create server manager state (will be initialized when gateway starts)
            let server_manager_state = Arc::new(RwLock::new(ServerManagerState::default()));

            // Get repositories for pool services (clone before moving into spawn)
            let db_for_gateway = app_state.database();
            let installed_server_repo = app_state.installed_server_repository.clone();
            let credential_repo = app_state.credential_repository.clone();
            let backend_oauth_repo = app_state.backend_oauth_repository.clone();
            let feature_set_repo = app_state.feature_set_repository.clone();
            let feature_repo = app_state.server_feature_repository_core.clone();
            let server_discovery = app_state.server_discovery.clone();
            let server_log_manager = app_state.server_log_manager.clone();
            let port_service = app_state.gateway_port_service.clone();
            let settings_repo = app_state.settings_repository.clone();

            // Auto-start gateway on app launch
            let gw_state_clone = gateway_state.clone();
            let sm_state_clone = server_manager_state.clone();
            let app_handle_for_sm = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Check if auto-start is enabled
                if !port_service.get_auto_start().await {
                    info!("[Gateway] Auto-start disabled, skipping");
                    return;
                }

                // Resolve port using the service (Single Responsibility)
                let final_port = match port_service.resolve_and_allocate().await {
                    Ok(port) => port,
                    Err(e) => {
                        warn!("[Gateway] Failed to allocate port: {}", e);
                        return;
                    }
                };

                let url = format!("http://localhost:{}", final_port);
                info!("Auto-starting gateway on {}", url);

                // Load JWT signing secret (DPAPI on Windows, keychain elsewhere)
                let jwt_secret = match mcpmux_storage::create_jwt_secret_provider(&app_data_dir) {
                    Ok(provider) => match provider.get_or_create_secret() {
                        Ok(secret) => {
                            info!("[Gateway] JWT signing secret loaded");
                            Some(secret)
                        }
                        Err(e) => {
                            warn!("[Gateway] Failed to load JWT secret: {}. Token signing disabled.", e);
                            None
                        }
                    },
                    Err(e) => {
                        warn!("[Gateway] Failed to create JWT secret provider: {}. Token signing disabled.", e);
                        None
                    }
                };

                // Build gateway dependencies using DI builder pattern
                let mut deps_builder = mcpmux_gateway::DependenciesBuilder::new()
                    .with_installed_server_repo(installed_server_repo)
                    .with_credential_repo(credential_repo)
                    .with_backend_oauth_repo(backend_oauth_repo)
                    .with_feature_repo(feature_repo)
                    .with_feature_set_repo(feature_set_repo)
                    .with_server_discovery(server_discovery)
                    .with_log_manager(server_log_manager)
                    .with_database(db_for_gateway)
                    .with_state_dir(app_data_dir.clone())
                    .with_settings_repo(settings_repo);

                if let Some(secret) = jwt_secret {
                    deps_builder = deps_builder.with_jwt_secret(secret);
                }

                let dependencies = match deps_builder.build() {
                    Ok(deps) => deps,
                    Err(e) => {
                        warn!("[Gateway] Failed to build dependencies: {}", e);
                        return;
                    }
                };

                // Create gateway config
                let config = mcpmux_gateway::GatewayConfig {
                    host: "127.0.0.1".to_string(),  // Bind address must be IP
                    port: final_port,
                    enable_cors: true,
                };

                // Create self-contained gateway server with DI
                // Gateway auto-initializes all services and auto-connects enabled servers
                let server = mcpmux_gateway::GatewayServer::new(config, dependencies);
                let gw_inner_state = server.state();

                // Get services from gateway
                let pool_service = server.pool_service();
                let feature_service = server.feature_service();
                let server_manager_arc = server.server_manager();
                let event_emitter = server.event_emitter();
                let grant_service = server.grant_service();

                // Start domain event bridge
                crate::commands::gateway::start_domain_event_bridge(&app_handle_for_sm, gw_inner_state.clone());

                // Subscribe to OAuth completion events
                let oauth_completion_rx = pool_service.oauth_manager().subscribe();

                info!("[Gateway] Services initialized via DI");

                // Store ServerManager and PoolService in state
                {
                    let mut sm_state = sm_state_clone.write().await;
                    sm_state.manager = Some(server_manager_arc.clone());
                    sm_state.pool_service = Some(pool_service.clone());
                }
                info!("[Gateway] ServerManager initialized with event bridge");

                // Start OAuth completion handler - reconnects servers after OAuth completes
                // IMPORTANT: Each reconnection is spawned as a separate task to allow parallel connections
                let sm_for_oauth = server_manager_arc.clone();
                let pool_for_oauth = pool_service.clone();
                tokio::spawn(async move {
                    use mcpmux_gateway::{ServerKey, ConnectionResult};
                    let mut rx = oauth_completion_rx;

                    info!("[OAuth Handler] Started listening for OAuth completions");

                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                info!(
                                    "[OAuth Handler] Received completion for {}: success={}",
                                    event.server_id, event.success
                                );

                                if event.success {
                                    // OAuth succeeded - spawn reconnection in separate task for parallelism
                                    let sm = sm_for_oauth.clone();
                                    let pool = pool_for_oauth.clone();
                                    let server_id = event.server_id.clone();
                                    let space_id = event.space_id;

                                    tokio::spawn(async move {
                                        let key = ServerKey::new(space_id, &server_id);

                                        info!("[OAuth Handler] Attempting reconnection for {}", server_id);
                                        sm.set_connecting(&key).await;

                                        match pool.reconnect_instance(space_id, &server_id).await {
                                            ConnectionResult::Connected { features, .. } => {
                                                info!("[OAuth Handler] Reconnection successful for {}", server_id);
                                                sm.set_connected(&key, features).await;
                                            }
                                            ConnectionResult::OAuthRequired { .. } => {
                                                warn!("[OAuth Handler] Still requires OAuth after completion: {}", server_id);
                                                sm.set_auth_required(&key, Some("OAuth still required".to_string())).await;
                                            }
                                            ConnectionResult::Failed { error } => {
                                                error!("[OAuth Handler] Reconnection failed for {}: {}", server_id, error);
                                                sm.set_error(&key, error).await;
                                            }
                                        }
                                    });
                                } else {
                                    // OAuth failed - handle synchronously (fast operation)
                                    let key = ServerKey::new(event.space_id, &event.server_id);
                                    let error_msg = event.error.unwrap_or_else(|| "OAuth failed".to_string());
                                    warn!("[OAuth Handler] OAuth failed for {}: {}", event.server_id, error_msg);
                                    sm_for_oauth.set_auth_required(&key, Some(error_msg)).await;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                warn!("[OAuth Handler] Lagged {} messages", n);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                info!("[OAuth Handler] Channel closed, stopping");
                                break;
                            }
                        }
                    }
                });
                info!("[Gateway] OAuth completion handler started");

                // Start periodic refresh loop (every 60s for connected servers)
                let _refresh_handle = server_manager_arc.clone().start_periodic_refresh();
                info!("[Gateway] Periodic refresh service started");

                // Note: Auto-connect happens in the frontend via useEffect calling connect_all_enabled_servers
                // This keeps the backend service clean and follows React best practices

                let handle = server.spawn();

                let mut state = gw_state_clone.write().await;
                state.running = true;
                state.url = Some(url.clone());
                state.handle = Some(handle);
                state.gateway_state = Some(gw_inner_state);
                state.pool_service = Some(pool_service);
                state.feature_service = Some(feature_service);
                state.event_emitter = Some(event_emitter);
                state.grant_service = Some(grant_service);

                info!(
                    "Gateway auto-started successfully on {} - GrantService initialized: {}",
                    url,
                    state.grant_service.is_some()
                );
            });

            app.manage(gateway_state);
            app.manage(server_manager_state);

            // Start file watcher for user space config files (hot-reload)
            {
                let app_state: tauri::State<'_, AppState> = app.state();
                let spaces_dir = app_state.spaces_dir().to_path_buf();
                let installed_repo = app_state.installed_server_repository.clone();
                let app_handle_for_watcher = app.handle().clone();

                // Use the well-known default space UUID
                // This is created by the initial migration (001_initial.sql)
                let default_space_id = "00000000-0000-0000-0000-000000000001".to_string();

                tauri::async_runtime::spawn(async move {

                    // Create file watcher with UI event emitter
                    match services::SpaceFileWatcher::new(
                        spaces_dir.clone(),
                        Arc::new(mcpmux_core::application::UserSpaceSyncService::new(installed_repo)),
                        default_space_id,
                        Some(move |space_id: &str, result: &mcpmux_core::application::SyncResult| {
                            // Emit event to refresh UI
                            if result.has_changes() {
                                if let Err(e) = app_handle_for_watcher.emit("space-servers-updated", serde_json::json!({
                                    "space_id": space_id,
                                    "added": result.added,
                                    "updated": result.updated,
                                    "removed": result.removed,
                                })) {
                                    warn!("[FileWatcher] Failed to emit event: {}", e);
                                }
                            }
                        }),
                    ) {
                        Ok(_watcher) => {
                            info!("[FileWatcher] Started watching: {:?}", spaces_dir);
                            // Keep watcher alive - it will run until app exits
                            // The watcher is kept in the spawned task's scope
                            std::future::pending::<()>().await;
                        }
                        Err(e) => {
                            warn!("[FileWatcher] Failed to start: {}", e);
                        }
                    }
                });
            }

            // Start periodic log cleanup task
            {
                let log_manager = app_state.server_log_manager.clone();
                let settings_repo_for_cleanup = app_state.settings_repository.clone();

                tauri::async_runtime::spawn(async move {
                    use mcpmux_core::AppSettingsService;

                    let settings = AppSettingsService::new(settings_repo_for_cleanup);

                    // Run cleanup once at startup
                    let retention_days = settings.get_log_retention_days().await;
                    if retention_days > 0 {
                        info!(
                            "[LogCleanup] Running startup cleanup (retention: {} days)",
                            retention_days
                        );
                        match log_manager.cleanup_logs_older_than(retention_days).await {
                            Ok(n) if n > 0 => {
                                info!("[LogCleanup] Startup cleanup removed {} file(s)", n)
                            }
                            Ok(_) => debug!("[LogCleanup] No old log files to clean up"),
                            Err(e) => warn!("[LogCleanup] Startup cleanup failed: {}", e),
                        }
                    }

                    // Then run every 24 hours
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
                    interval.tick().await; // skip the first immediate tick (already ran above)

                    loop {
                        interval.tick().await;
                        let days = settings.get_log_retention_days().await;
                        if days > 0 {
                            match log_manager.cleanup_logs_older_than(days).await {
                                Ok(n) if n > 0 => {
                                    info!("[LogCleanup] Periodic cleanup removed {} file(s)", n)
                                }
                                Ok(_) => {}
                                Err(e) => warn!("[LogCleanup] Periodic cleanup failed: {}", e),
                            }
                        }
                    }
                });
            }

            // Setup system tray
            tray::setup_tray(app.handle())?;

            // Setup window close event handler for close-to-tray behavior
            if let Some(main_window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let settings_repo = app_state.settings_repository.clone();

                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        // Check if close-to-tray is enabled
                        let app_handle_clone = app_handle.clone();
                        let settings_clone = settings_repo.clone();

                        tauri::async_runtime::spawn(async move {
                            match settings_clone.get("ui.close_to_tray").await {
                                Ok(Some(value)) if value == "true" => {
                                    // Close to tray - hide window instead of closing
                                    info!("[Window] Close requested, hiding to tray");
                                    if let Some(window) = app_handle_clone.get_webview_window("main") {
                                        let _ = window.hide();
                                    }
                                }
                                Ok(Some(value)) if value == "false" => {
                                    // Actually close the app
                                    info!("[Window] Close requested, exiting app");
                                    app_handle_clone.exit(0);
                                }
                                _ => {
                                    // Default behavior: close to tray
                                    info!("[Window] Close requested (default), hiding to tray");
                                    if let Some(window) = app_handle_clone.get_webview_window("main") {
                                        let _ = window.hide();
                                    }
                                }
                            }
                        });

                        // Always prevent default close to handle it asynchronously
                        api.prevent_close();
                    }
                });

                // Check if app should start hidden (auto-launch with --hidden flag)
                if commands::should_start_hidden() {
                    info!("[Window] Starting hidden (--hidden flag present)");
                    let _ = main_window.hide();
                }
            }

            // Enable auto-start on first launch if not already configured.
            // The OS-level autostart is only set if not previously enabled/disabled by the user.
            // This ensures fresh installs get autostart without requiring manual Settings toggle.
            {
                let autostart_manager: tauri::State<'_, tauri_plugin_autostart::AutoLaunchManager> = app.state();
                match autostart_manager.is_enabled() {
                    Ok(false) => {
                        // Check if user has ever explicitly configured autostart
                        let app_state: tauri::State<'_, AppState> = app.state();
                        let was_configured = tauri::async_runtime::block_on(async {
                            app_state.settings_repository
                                .get("startup.autostart_configured")
                                .await
                                .ok()
                                .flatten()
                                .is_some()
                        });

                        if !was_configured {
                            // First launch: enable autostart and mark as configured
                            if let Err(e) = autostart_manager.enable() {
                                warn!("[Autostart] Failed to enable on first launch: {}", e);
                            } else {
                                info!("[Autostart] Enabled on first launch");
                            }
                            tauri::async_runtime::block_on(async {
                                let _ = app_state.settings_repository
                                    .set("startup.autostart_configured", "true")
                                    .await;
                            });
                        }
                    }
                    Ok(true) => {
                        info!("[Autostart] Already enabled");
                    }
                    Err(e) => {
                        warn!("[Autostart] Failed to check status: {}", e);
                    }
                }
            }

            // Register deep link protocol in OS (Windows registry / Linux xdg-mime)
            // NSIS writes to HKCU, MSI writes to HKLM — both register during install.
            // This register_all() call is a safety net for dev mode and edge cases
            // (e.g. AppImage on Linux, portable installs).
            #[cfg(any(windows, target_os = "linux"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if let Err(e) = app.deep_link().register_all() {
                    warn!("[DeepLink] Failed to register protocol schemes: {}", e);
                } else {
                    info!("[DeepLink] Protocol schemes registered successfully");
                }
            }

            // Register deep link handler for when app receives URLs
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let app_handle = app.handle().clone();

                // Register the deep link handler
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        info!("[DeepLink] Received URL: {}", url);
                        handle_deep_link(&app_handle, url.as_str());
                    }
                });
            }

            info!("Application started successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_version,
            // Space commands
            commands::list_spaces,
            commands::get_space,
            commands::create_space,
            commands::delete_space,
            commands::get_active_space,
            commands::set_active_space,
            commands::open_space_config_file,
            commands::read_space_config,
            commands::save_space_config,
            commands::remove_server_from_config,
            commands::refresh_tray_menu,
            // Server Discovery commands (v2)
            commands::discover_servers,
            commands::get_server_definition,
            commands::get_registry_ui_config,
            commands::get_registry_home_config,
            commands::is_registry_offline,
            commands::refresh_registry,
            commands::search_servers,
            // Installed Server commands
            commands::install_server,
            commands::uninstall_server,
            commands::list_installed_servers,
            commands::set_server_enabled,
            commands::set_server_oauth_connected,
            commands::save_server_inputs,
            // FeatureSet commands
            commands::list_feature_sets,
            commands::list_feature_sets_by_space,
            commands::get_feature_set,
            commands::get_feature_set_with_members,
            commands::create_feature_set,
            commands::update_feature_set,
            commands::delete_feature_set,
            commands::get_builtin_feature_sets,
            commands::ensure_server_all_feature_set,
            commands::add_feature_set_member,
            commands::remove_feature_set_member,
            commands::set_feature_set_members,
            // Individual feature member commands
            commands::add_feature_to_set,
            commands::remove_feature_from_set,
            commands::get_feature_set_members,
            // Client custom feature sets
            commands::find_or_create_client_custom_feature_set,
            // Server feature commands
            commands::list_server_features,
            commands::list_server_features_by_server,
            commands::list_server_features_by_type,
            commands::get_server_feature,
            // Client commands
            commands::list_clients,
            commands::get_client,
            commands::create_client,
            commands::delete_client,
            commands::update_client_grants,
            commands::update_client_mode,
            commands::init_preset_clients,
            commands::get_client_grants,
            commands::get_all_client_grants,
            commands::grant_feature_set_to_client,
            commands::revoke_feature_set_from_client,
            // Config export commands
            commands::preview_config_export,
            commands::export_config_to_file,
            commands::get_config_paths,
            commands::check_config_exists,
            commands::backup_existing_config,
            // Gateway commands
            commands::get_gateway_status,
            commands::start_gateway,
            commands::stop_gateway,
            commands::restart_gateway,
            commands::generate_gateway_config,
            commands::connect_server,
            commands::disconnect_server,
            commands::list_connected_servers,
            commands::connect_all_enabled_servers,
            commands::get_pool_stats,
            commands::refresh_oauth_tokens_on_startup,
            // OAuth commands
            commands::approve_oauth_consent,
            commands::get_pending_consent,
            commands::get_oauth_clients,
            commands::approve_oauth_client,
            commands::update_oauth_client,
            commands::delete_oauth_client,
            commands::get_oauth_client_grants,
            commands::grant_oauth_client_feature_set,
            commands::revoke_oauth_client_feature_set,
            commands::get_oauth_client_resolved_features,
            commands::open_url,
            // Server Manager commands (event-driven v2)
            commands::get_server_statuses,
            commands::enable_server_v2,
            commands::disable_server_v2,
            commands::start_auth_v2,
            commands::cancel_auth_v2,
            commands::retry_connection,
            commands::logout_server,
            commands::disconnect_server_v2,
            // Log commands
            commands::get_server_logs,
            commands::clear_server_logs,
            commands::get_server_log_file,
            commands::get_log_retention_days,
            commands::set_log_retention_days,
            // App log commands
            get_logs_path,
            open_logs_folder,
            // Startup settings commands
            commands::get_startup_settings,
            commands::update_startup_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running McpMux application");
}
