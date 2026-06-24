//! Admin Axum router — health, static SPA, API routes.

use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use mcpmux_core::ApplicationServices;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tracing::warn;

use super::config::AdminConfig;
use super::event_hub::AdminEventHub;
use super::handlers::{events, health, oauth, read, spa, write};
use super::middleware::{cf_access_middleware, csrf_middleware, get_csrf_token, CfAccessValidator};
use crate::admin::bridge_context::AdminBridgeCtx;

/// Shared state for admin HTTP handlers.
#[derive(Clone)]
pub struct AdminState {
    /// Application services (same instance as Tauri commands).
    pub services: Arc<ApplicationServices>,
    /// Admin server configuration.
    pub config: AdminConfig,
    /// MCP gateway running flag (updated by desktop when gateway starts/stops).
    pub gateway_running: Arc<AtomicBool>,
    /// Directory containing built frontend assets (`index.html`, etc.).
    pub frontend_dist: PathBuf,
    /// CF Access JWT validator when `trust_cf_access` is enabled.
    pub cf_validator: Option<Arc<CfAccessValidator>>,
    /// Shared read bridge context used by REST handlers.
    pub bridge: Arc<AdminBridgeCtx>,
    /// Merged EventBus + direct UI event fan-in for SSE.
    pub event_hub: Arc<AdminEventHub>,
    /// CSRF token for mutating requests.
    pub csrf_token: Arc<Mutex<String>>,
}

/// Build the admin router with health, API stubs, and SPA static fallback.
pub fn build_admin_router(state: AdminState) -> Router {
    state.event_hub.start(state.services.clone());

    let mut router = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/csrf-token", get(get_csrf_token))
        .route("/api/v1/events", get(events::sse_events))
        .route("/api/v1/gateway/status", get(read::get_gateway_status))
        .route(
            "/api/v1/gateway/probe-start",
            get(read::probe_gateway_start),
        )
        .route(
            "/api/v1/gateway/pending-port-conflict",
            get(read::take_pending_port_conflict),
        )
        .route(
            "/api/v1/gateway/port-settings",
            get(read::get_gateway_port_settings),
        )
        .route("/api/v1/gateway/reset-port", get(read::reset_gateway_port))
        .route(
            "/api/v1/gateway/connected-servers",
            get(read::list_connected_servers),
        )
        .route("/api/v1/gateway/pool-stats", get(read::get_pool_stats))
        .route("/api/v1/gateway/start", post(write::start_gateway))
        .route("/api/v1/gateway/stop", post(write::stop_gateway))
        .route("/api/v1/gateway/restart", post(write::restart_gateway))
        .route("/api/v1/gateway/disconnect", post(write::disconnect_server))
        .route(
            "/api/v1/gateway/connect-all",
            post(write::connect_all_enabled_servers),
        )
        .route(
            "/api/v1/gateway/refresh-oauth-tokens",
            post(write::refresh_oauth_tokens_on_startup),
        )
        .route("/api/v1/gateway/port", put(write::set_gateway_port))
        .route(
            "/api/v1/gateway/public-url",
            put(write::set_gateway_public_url),
        )
        .route(
            "/api/v1/spaces",
            get(read::list_spaces).post(write::create_space),
        )
        .route(
            "/api/v1/spaces/{id}",
            get(read::get_space)
                .put(write::update_space)
                .delete(write::delete_space),
        )
        .route(
            "/api/v1/spaces/{id}/config",
            get(read::read_space_config).put(write::save_space_config),
        )
        .route(
            "/api/v1/spaces/{space_id}/config/servers/{server_id}",
            delete(write::remove_server_from_config),
        )
        .route(
            "/api/v1/spaces/{space_id}/base-dirs",
            get(read::list_space_base_dirs).post(write::add_space_base_dir),
        )
        .route(
            "/api/v1/spaces/base-dirs/{id}",
            delete(write::remove_space_base_dir),
        )
        .route(
            "/api/v1/servers/installed",
            get(read::list_installed_servers),
        )
        .route("/api/v1/servers/install", post(write::install_server))
        .route("/api/v1/servers/{id}", delete(write::uninstall_server))
        .route(
            "/api/v1/servers/{id}/inputs",
            put(write::save_server_inputs),
        )
        .route(
            "/api/v1/servers/{id}/display-name",
            put(write::set_server_display_name),
        )
        .route(
            "/api/v1/servers/{id}/oauth-connected",
            put(write::set_server_oauth_connected),
        )
        .route(
            "/api/v1/servers/{id}/enabled",
            put(write::set_server_enabled),
        )
        .route(
            "/api/v1/servers/connections",
            get(read::get_server_statuses),
        )
        .route(
            "/api/v1/servers/connections/enable",
            post(write::enable_server_v2),
        )
        .route(
            "/api/v1/servers/connections/disable",
            post(write::disable_server_v2),
        )
        .route(
            "/api/v1/servers/connections/start-auth",
            post(write::start_auth_v2),
        )
        .route(
            "/api/v1/servers/connections/cancel-auth",
            post(write::cancel_auth_v2),
        )
        .route(
            "/api/v1/servers/connections/retry",
            post(write::retry_connection),
        )
        .route(
            "/api/v1/servers/connections/update-package",
            post(write::update_server_package),
        )
        .route(
            "/api/v1/servers/connections/logout",
            post(write::logout_server),
        )
        .route(
            "/api/v1/servers/updates/check-all",
            post(write::check_all_server_versions),
        )
        .route(
            "/api/v1/servers/{server_id}/updates/check",
            post(write::check_server_version),
        )
        .route("/api/v1/servers/clones", post(write::clone_server))
        .route("/api/v1/registry/discover", get(read::discover_servers))
        .route(
            "/api/v1/registry/definition/{server_id}",
            get(read::get_server_definition),
        )
        .route(
            "/api/v1/registry/ui-config",
            get(read::get_registry_ui_config),
        )
        .route(
            "/api/v1/registry/home-config",
            get(read::get_registry_home_config),
        )
        .route("/api/v1/registry/offline", get(read::is_registry_offline))
        .route(
            "/api/v1/registry/categories",
            get(read::list_registry_categories),
        )
        .route("/api/v1/registry/refresh", post(write::refresh_registry))
        .route(
            "/api/v1/clients",
            get(read::list_clients).post(write::create_client),
        )
        .route(
            "/api/v1/clients/{id}",
            get(read::get_client).delete(write::delete_client),
        )
        .route(
            "/api/v1/clients/init-presets",
            post(write::init_preset_clients),
        )
        .route(
            "/api/v1/feature-sets",
            get(read::list_feature_sets).post(write::create_feature_set),
        )
        .route(
            "/api/v1/feature-sets/by-space/{space_id}",
            get(read::list_feature_sets_by_space),
        )
        .route(
            "/api/v1/feature-sets/{id}",
            get(read::get_feature_set)
                .put(write::update_feature_set)
                .delete(write::delete_feature_set),
        )
        .route(
            "/api/v1/feature-sets/{id}/with-members",
            get(read::get_feature_set_with_members),
        )
        .route(
            "/api/v1/feature-sets/{id}/members",
            post(write::add_feature_set_member).put(write::set_feature_set_members),
        )
        .route(
            "/api/v1/feature-sets/{id}/members/{member_id}",
            delete(write::remove_feature_set_member),
        )
        .route(
            "/api/v1/workspaces/bindings",
            get(read::list_workspace_bindings).post(write::create_workspace_binding),
        )
        .route(
            "/api/v1/workspaces/bindings/space/{space_id}",
            get(read::list_workspace_bindings_for_space),
        )
        .route(
            "/api/v1/workspaces/bindings/{id}",
            put(write::update_workspace_binding).delete(write::delete_workspace_binding),
        )
        .route(
            "/api/v1/workspaces/reported-roots",
            get(read::list_reported_workspace_roots),
        )
        .route(
            "/api/v1/workspaces/reported-roots/clear-unmapped",
            post(write::clear_unmapped_reported_roots),
        )
        .route(
            "/api/v1/workspaces/validate-root",
            get(read::validate_workspace_root),
        )
        .route(
            "/api/v1/workspaces/effective-features",
            get(read::get_workspace_effective_features),
        )
        .route(
            "/api/v1/workspaces/appearances",
            get(read::list_workspace_appearances)
                .post(write::upload_workspace_icon)
                .put(write::upsert_workspace_appearance)
                .delete(write::delete_workspace_appearance),
        )
        .route(
            "/api/v1/workspaces/icon-path",
            get(read::resolve_workspace_icon_path),
        )
        .route("/api/v1/workspaces/icon", get(read::serve_workspace_icon))
        .route(
            "/api/v1/settings/startup",
            get(read::get_startup_settings).put(write::update_startup_settings),
        )
        .route(
            "/api/v1/settings/server-updates",
            get(read::get_server_update_settings).put(write::update_server_update_settings),
        )
        .route(
            "/api/v1/settings/meta-tools-enabled",
            get(read::get_meta_tools_enabled).put(write::set_meta_tools_enabled),
        )
        .route(
            "/api/v1/settings/meta-tools-require-approval",
            get(read::get_meta_tools_require_approval).put(write::set_meta_tools_require_approval),
        )
        .route(
            "/api/v1/settings/workspace-mapping-prompt",
            get(read::get_workspace_mapping_prompt_enabled)
                .put(write::set_workspace_mapping_prompt_enabled),
        )
        .route(
            "/api/v1/settings/auto-install-updates",
            get(read::get_auto_install_updates),
        )
        .route(
            "/api/v1/settings/update-channel",
            get(read::get_update_channel).put(write::set_update_channel),
        )
        .route("/api/v1/app/version", get(read::get_version))
        .route("/api/v1/app/bundle-version", get(read::get_bundle_version))
        .route("/api/v1/app/build-info", get(read::get_build_info))
        .route("/api/v1/app/logs-path", get(read::get_logs_path))
        .route(
            "/api/v1/logs/server/{server_id}",
            get(read::get_server_logs).delete(write::clear_server_logs),
        )
        .route(
            "/api/v1/logs/server/{server_id}/file",
            get(read::get_server_log_file),
        )
        .route(
            "/api/v1/logs/retention-days",
            get(read::get_log_retention_days).put(write::set_log_retention_days),
        )
        .route("/api/v1/oauth/clients", get(read::get_oauth_clients))
        .route(
            "/api/v1/oauth/clients/{client_id}",
            put(write::update_oauth_client).delete(write::delete_oauth_client),
        )
        .route(
            "/api/v1/oauth/clients/{client_id}/grants",
            post(write::grant_oauth_client_feature_set),
        )
        .route(
            "/api/v1/oauth/clients/{client_id}/grants/revoke",
            post(write::revoke_oauth_client_feature_set),
        )
        .route(
            "/api/v1/oauth/clients/{client_id}/grants/{space_id}",
            get(read::get_oauth_client_grants),
        )
        .route(
            "/api/v1/oauth/consent/pending",
            get(oauth::get_pending_consent),
        )
        .route(
            "/api/v1/oauth/consent/approve",
            post(oauth::approve_oauth_consent),
        )
        .route(
            "/api/v1/oauth/consent/reject",
            post(oauth::reject_oauth_consent),
        )
        .route(
            "/api/v1/meta-tools/grants",
            get(read::list_meta_tool_grants),
        )
        .route(
            "/api/v1/meta-tools/approval",
            post(write::respond_to_meta_tool_approval),
        )
        .route(
            "/api/v1/meta-tools/grants/revoke",
            post(write::revoke_meta_tool_grant),
        )
        .route("/api/v1/server-features", get(read::list_server_features))
        .route(
            "/api/v1/server-features/by-server",
            get(read::list_server_features_by_server),
        )
        .route(
            "/api/v1/server-features/by-type",
            get(read::list_server_features_by_type),
        )
        .route(
            "/api/v1/server-features/{id}",
            get(read::get_server_feature),
        )
        .route("/api/v1/builtins", get(read::list_builtin_servers))
        .route(
            "/api/v1/builtins/server-enabled",
            put(write::set_builtin_server_enabled),
        )
        .route(
            "/api/v1/builtins/tool-enabled",
            put(write::set_builtin_tool_enabled),
        )
        .route(
            "/api/v1/servers/clones/available",
            get(read::is_clone_id_available),
        )
        .route(
            "/api/v1/servers/clones/suggest",
            get(read::suggest_clone_suffix),
        )
        .route(
            "/api/v1/servers/clones/dependents",
            get(read::list_clone_dependents),
        )
        .route(
            "/api/v1/config-export/preview",
            get(read::preview_config_export),
        )
        .route("/api/v1/config-export/paths", get(read::get_config_paths))
        .route(
            "/api/v1/config-export/check",
            post(write::check_config_exists),
        )
        .route(
            "/api/v1/config-export/backup",
            post(write::backup_existing_config),
        )
        .route(
            "/api/v1/config-export/export",
            post(write::export_config_to_file),
        );

    #[cfg(any(test, feature = "test-utils"))]
    {
        router = router.route(
            "/api/v1/test/events/publish",
            post(events::publish_test_event),
        );
    }

    if state.frontend_dist.join("index.html").is_file() {
        let index = state.frontend_dist.join("index.html");
        let static_files =
            ServeDir::new(&state.frontend_dist).not_found_service(ServeFile::new(index));
        router = router.fallback_service(static_files);
    } else {
        warn!(
            "[Admin] frontend dist missing index.html at {:?} — serving build hint page",
            state.frontend_dist
        );
        router = router.fallback(get(spa::missing_spa_build));
    }

    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            csrf_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cf_access_middleware,
        ))
        .with_state(state)
}
