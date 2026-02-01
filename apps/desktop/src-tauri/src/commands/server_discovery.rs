use crate::AppState;
use mcpmux_core::{HomeConfig, InstalledServer, ServerDefinition, ServerSource, UiConfig};
use tauri::State;

/// Discover all available servers (from API + User Spaces)
#[tauri::command]
pub async fn discover_servers(state: State<'_, AppState>) -> Result<Vec<ServerDefinition>, String> {
    tracing::info!("[discover_servers] Refreshing server list from all sources");

    // Refresh from API if needed (5 min cache)
    state
        .server_discovery
        .refresh_if_needed()
        .await
        .map_err(|e| format!("Failed to refresh: {}", e))?;

    // Get all merged servers
    let servers = state.server_discovery.list().await;

    tracing::info!(
        "[discover_servers] Returning {} server definitions",
        servers.len()
    );
    Ok(servers)
}

/// Force refresh from all sources (ignores cache)
/// Returns number of newly auto-installed user-configured servers
#[tauri::command]
pub async fn refresh_registry(state: State<'_, AppState>) -> Result<u32, String> {
    tracing::info!("[refresh_registry] Force refreshing from all sources");

    state
        .server_discovery
        .refresh()
        .await
        .map_err(|e| format!("Failed to refresh: {}", e))?;

    // Auto-install user-configured servers
    let servers = state.server_discovery.list().await;
    let count = auto_install_user_servers(&state, &servers).await?;

    tracing::info!(
        "[refresh_registry] Refresh complete, {} new servers auto-installed",
        count
    );
    Ok(count)
}

/// Auto-install servers from user space configs (they should appear in My Servers automatically)
/// Returns the count of newly installed servers
async fn auto_install_user_servers(
    state: &AppState,
    servers: &[ServerDefinition],
) -> Result<u32, String> {
    let mut count = 0;

    for server in servers {
        if let ServerSource::UserSpace { space_id, .. } = &server.source {
            // Check if already installed (by normalized ID)
            let existing = state
                .installed_server_repository
                .get_by_server_id(space_id, &server.id)
                .await
                .map_err(|e| format!("Failed to check installed: {}", e))?;

            if existing.is_none() {
                // Auto-install the user-configured server
                tracing::info!(
                    "[auto_install] Installing user-configured server: {} (name: {}) in space {}",
                    server.id,
                    server.name,
                    space_id
                );

                // Cache the definition for offline support
                let installed = InstalledServer::new(space_id, &server.id).with_definition(server);
                state
                    .installed_server_repository
                    .install(&installed)
                    .await
                    .map_err(|e| format!("Failed to auto-install: {}", e))?;

                count += 1;
            }
        }
    }

    Ok(count)
}

/// Search servers by query
#[tauri::command]
pub async fn search_servers(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<ServerDefinition>, String> {
    tracing::debug!("[search_servers] Searching for: {}", query);

    // Refresh if needed first
    state
        .server_discovery
        .refresh_if_needed()
        .await
        .map_err(|e| format!("Failed to refresh: {}", e))?;

    // Search in local cache
    let results = state.server_discovery.search(&query).await;

    tracing::debug!("[search_servers] Found {} results", results.len());
    Ok(results)
}

/// Get a single server definition by ID
#[tauri::command]
pub async fn get_server_definition(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Option<ServerDefinition>, String> {
    tracing::debug!("[get_server_definition] Getting server: {}", server_id);

    // Refresh if needed first
    state
        .server_discovery
        .refresh_if_needed()
        .await
        .map_err(|e| format!("Failed to refresh: {}", e))?;

    // Get from cache
    let server = state.server_discovery.get(&server_id).await;

    Ok(server)
}

/// Get UI configuration from registry bundle (filters, sort options, etc.)
#[tauri::command]
pub async fn get_registry_ui_config(state: State<'_, AppState>) -> Result<UiConfig, String> {
    tracing::debug!("[get_registry_ui_config] Getting UI config");

    // Refresh if needed first
    state
        .server_discovery
        .refresh_if_needed()
        .await
        .map_err(|e| format!("Failed to refresh: {}", e))?;

    // Get from cache
    let config = state.server_discovery.ui_config().await;

    tracing::debug!(
        "[get_registry_ui_config] Returning {} filters, {} sort options",
        config.filters.len(),
        config.sort_options.len()
    );
    Ok(config)
}

/// Get home configuration from registry bundle (featured server IDs)
#[tauri::command]
pub async fn get_registry_home_config(
    state: State<'_, AppState>,
) -> Result<Option<HomeConfig>, String> {
    tracing::debug!("[get_registry_home_config] Getting home config");

    // Refresh if needed first
    state
        .server_discovery
        .refresh_if_needed()
        .await
        .map_err(|e| format!("Failed to refresh: {}", e))?;

    // Get from cache
    let config = state.server_discovery.home_config().await;

    Ok(config)
}

/// Check if registry is running in offline mode (using disk cache)
#[tauri::command]
pub async fn is_registry_offline(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.server_discovery.is_offline().await)
}
