//! Command-mirror JSON-RPC for the management API.
//!
//! `POST /admin/api/rpc/<command>` dispatches by the SAME command names +
//! argument shapes the desktop uses over Tauri `invoke`, returning the SAME
//! result shapes — so the desktop React UI, served headless and running its
//! HTTP transport, drives this endpoint unchanged.
//!
//! Commands that manage McpMux state (Spaces, FeatureSets, clients, bindings,
//! gateway posture) are implemented against the gateway's repositories. Commands
//! that are inherently desktop/OS-bound (dialogs, tray, deep links, updater,
//! autostart, on-disk client-config editing) — or that drive the gateway's own
//! lifecycle, which the serve process owns — return a structured
//! `desktop_only` error, which the UI's capability layer renders as
//! "not available on the web". Every known command name is recognised.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde_json::{json, Value};
use uuid::Uuid;

use mcpmux_core::{FeatureSet, Space, WorkspaceBinding};

use super::handlers::AppState;

/// Error from a dispatched command.
struct RpcError {
    status: StatusCode,
    message: String,
}
impl RpcError {
    fn bad(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
    /// A recognised command that the headless server does not implement (it is
    /// desktop/OS-bound or lifecycle-owning). 501 so the UI can distinguish it
    /// from a genuine failure.
    fn desktop_only(cmd: &str) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            message: format!("command '{cmd}' is only available in the desktop app"),
        }
    }
}

type RpcResult = Result<Value, RpcError>;

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key)
}
fn arg_str(args: &Value, key: &str) -> Result<String, RpcError> {
    arg(args, key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| RpcError::bad(format!("missing string arg '{key}'")))
}

/// `POST /admin/api/rpc/{command}` — dispatch and return `Json(result)`.
pub async fn rpc(
    State(app_state): State<AppState>,
    Path(command): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);
    match dispatch(&app_state, &command, args).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => (e.status, Json(json!({ "error": e.message }))).into_response(),
    }
}

async fn dispatch(app_state: &AppState, cmd: &str, args: Value) -> RpcResult {
    let deps = &app_state.services.dependencies;
    match cmd {
        // ---- Spaces ----
        "list_spaces" => {
            let spaces = deps.space_repo.list().await.map_err(err)?;
            to_val(&spaces)
        }
        "get_space" => {
            let id = parse_uuid(&arg_str(&args, "id")?)?;
            let space = deps.space_repo.get(&id).await.map_err(err)?;
            to_val(&space)
        }
        "create_space" => {
            let name = arg_str(&args, "name")?;
            if name.trim().is_empty() {
                return Err(RpcError::bad("name is required"));
            }
            let mut space = Space::new(name.trim());
            if let Some(icon) = arg(&args, "icon")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                space = space.with_icon(icon);
            }
            deps.space_repo.create(&space).await.map_err(err)?;
            emit_space_created(app_state, &space).await;
            to_val(&space)
        }
        "delete_space" => {
            let id = parse_uuid(&arg_str(&args, "id")?)?;
            deps.space_repo.delete(&id).await.map_err(err)?;
            Ok(Value::Null)
        }

        // ---- FeatureSets ----
        "list_feature_sets" => {
            let sets = deps.feature_set_repo.list().await.map_err(err)?;
            to_val(&sets)
        }
        "list_feature_sets_by_space" => {
            let space_id = arg_str(&args, "spaceId").or_else(|_| arg_str(&args, "space_id"))?;
            let sets = deps
                .feature_set_repo
                .list_by_space(&space_id)
                .await
                .map_err(err)?;
            to_val(&sets)
        }
        "get_feature_set" => {
            let id = arg_str(&args, "id")?;
            let fs = deps.feature_set_repo.get(&id).await.map_err(err)?;
            to_val(&fs)
        }
        "get_feature_set_members" => {
            let id = arg_str(&args, "id").or_else(|_| arg_str(&args, "featureSetId"))?;
            let members = deps
                .feature_set_repo
                .get_feature_members(&id)
                .await
                .map_err(err)?;
            to_val(&members)
        }
        "get_feature_set_with_members" => {
            let id = arg_str(&args, "id")?;
            let fs = deps
                .feature_set_repo
                .get_with_members(&id)
                .await
                .map_err(err)?;
            to_val(&fs)
        }
        "create_feature_set" => {
            // Desktop sends { input: { name, space_id, description?, icon? } }.
            let input = arg(&args, "input").cloned().unwrap_or(args.clone());
            let name = arg_str(&input, "name")?;
            let space_id = arg_str(&input, "space_id")?;
            if name.trim().is_empty() {
                return Err(RpcError::bad("name is required"));
            }
            let space_uuid = parse_uuid(&space_id)?;
            let mut fs = FeatureSet::new_custom(name.trim(), space_id.trim());
            fs.description = arg(&input, "description")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            fs.icon = arg(&input, "icon")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            deps.feature_set_repo.create(&fs).await.map_err(err)?;
            emit_fs_created(app_state, space_uuid, &fs).await;
            to_val(&fs)
        }
        "update_feature_set" => {
            let id = arg_str(&args, "id")?;
            let mut fs = deps
                .feature_set_repo
                .get(&id)
                .await
                .map_err(err)?
                .ok_or_else(|| RpcError::bad("feature set not found"))?;
            let input = arg(&args, "input").cloned().unwrap_or(args.clone());
            if let Some(name) = arg(&input, "name").and_then(Value::as_str) {
                fs.name = name.to_string();
            }
            if let Some(desc) = arg(&input, "description").and_then(Value::as_str) {
                fs.description = Some(desc.to_string());
            }
            if let Some(icon) = arg(&input, "icon").and_then(Value::as_str) {
                fs.icon = Some(icon.to_string());
            }
            deps.feature_set_repo.update(&fs).await.map_err(err)?;
            to_val(&fs)
        }
        "delete_feature_set" => {
            let id = arg_str(&args, "id")?;
            deps.feature_set_repo.delete(&id).await.map_err(err)?;
            Ok(Value::Null)
        }

        // ---- Inbound clients ----
        "get_oauth_clients" | "list_clients" => {
            let clients = deps.inbound_client_repo.list_clients().await.map_err(err)?;
            to_val(&clients)
        }
        "get_client" => {
            let id = arg_str(&args, "id").or_else(|_| arg_str(&args, "clientId"))?;
            let client = deps
                .inbound_client_repo
                .get_client(&id)
                .await
                .map_err(err)?;
            to_val(&client)
        }
        "delete_oauth_client" | "delete_client" => {
            let id = arg_str(&args, "id").or_else(|_| arg_str(&args, "clientId"))?;
            deps.inbound_client_repo
                .delete_client(&id)
                .await
                .map_err(err)?;
            emit_client_changed(app_state, &id).await;
            Ok(Value::Null)
        }
        "update_oauth_client" => {
            let id = arg_str(&args, "clientId").or_else(|_| arg_str(&args, "id"))?;
            if let Some(alias) = arg(&args, "alias").and_then(Value::as_str) {
                deps.inbound_client_repo
                    .update_client_alias(&id, Some(alias.to_string()))
                    .await
                    .map_err(err)?;
            }
            emit_client_changed(app_state, &id).await;
            Ok(Value::Null)
        }
        "get_oauth_client_grants" => Ok(json!([])), // v2: routing is binding-driven, no per-client grants

        // ---- Workspace bindings (mappings) ----
        "list_workspace_bindings" => {
            let bindings = deps.workspace_binding_repo.list().await.map_err(err)?;
            to_val(&bindings)
        }
        "list_workspace_bindings_for_space" => {
            let space_id =
                parse_uuid(&arg_str(&args, "spaceId").or_else(|_| arg_str(&args, "space_id"))?)?;
            let bindings = deps
                .workspace_binding_repo
                .list_for_space(&space_id)
                .await
                .map_err(err)?;
            to_val(&bindings)
        }
        "create_workspace_binding" => {
            let input = arg(&args, "input").cloned().unwrap_or(args.clone());
            let binding = binding_from_input(&input)?;
            deps.workspace_binding_repo
                .create(&binding)
                .await
                .map_err(err)?;
            emit_binding_changed(app_state, binding.space_id, &binding.workspace_root).await;
            to_val(&binding)
        }
        "update_workspace_binding" => {
            let id = parse_uuid(&arg_str(&args, "id")?)?;
            let input = arg(&args, "input").cloned().unwrap_or(args.clone());
            let mut binding = binding_from_input(&input)?;
            binding.id = id;
            deps.workspace_binding_repo
                .update(&binding)
                .await
                .map_err(err)?;
            emit_binding_changed(app_state, binding.space_id, &binding.workspace_root).await;
            to_val(&binding)
        }
        "delete_workspace_binding" => {
            let id = parse_uuid(&arg_str(&args, "id")?)?;
            let existing = deps.workspace_binding_repo.get(&id).await.ok().flatten();
            deps.workspace_binding_repo.delete(&id).await.map_err(err)?;
            if let Some(b) = existing {
                emit_binding_changed(app_state, b.space_id, &b.workspace_root).await;
            }
            Ok(Value::Null)
        }
        "validate_workspace_root" => {
            // Headless: accept the value as-is (trimmed). Full path normalization
            // is a desktop concern (it inspects the local filesystem).
            let path = arg_str(&args, "path").or_else(|_| arg_str(&args, "root"))?;
            Ok(Value::String(path.trim().to_string()))
        }
        "list_reported_workspace_roots" => {
            let roots = app_state.services.session_roots.list_all_roots();
            to_val(&roots)
        }
        "clear_unmapped_reported_roots" => {
            // Forget every reported root that has no binding.
            let bindings = deps.workspace_binding_repo.list().await.map_err(err)?;
            let mapped: std::collections::HashSet<String> = bindings
                .iter()
                .map(|b| b.workspace_root.to_lowercase())
                .collect();
            let forgotten = app_state
                .services
                .session_roots
                .forget_unmapped_roots(|root| mapped.contains(&root.to_lowercase()));
            Ok(json!(forgotten.len()))
        }

        // ---- Servers ----
        "list_installed_servers" => {
            let space_id = match arg(&args, "spaceId").and_then(Value::as_str) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => default_space_id(app_state).await?.to_string(),
            };
            let servers = deps
                .installed_server_repo
                .list_for_space(&space_id)
                .await
                .map_err(err)?;
            to_val(&servers)
        }

        // ---- Gateway posture / settings (reads reflect the running config) ----
        "get_gateway_status" => {
            let state = app_state.gateway_state.read().await;
            Ok(json!({
                "running": true,
                "url": app_state.base_url,
                "active_sessions": state.sessions.len(),
                "connected_backends": 0,
            }))
        }
        "get_gateway_network_access" => {
            Ok(json!(app_state.gateway_state.read().await.network_bind))
        }
        "get_gateway_auth_disabled" => {
            Ok(json!(app_state.gateway_state.read().await.auth_disabled()))
        }
        "get_gateway_host_allowlist" => Ok(json!({
            "additionalHosts": [],
            "allowAnyHost": false,
        })),
        "get_gateway_public_url_settings" => Ok(json!({
            "configuredPublicBaseUrl": null,
            "activePublicBaseUrl": app_state.base_url,
            "localBaseUrl": app_state.base_url,
        })),
        "get_gateway_port_settings" => Ok(json!({
            "configuredPort": null,
            "defaultPort": mcpmux_core::DEFAULT_GATEWAY_PORT,
            "activePort": null,
        })),
        "get_meta_tools_require_approval" => Ok(json!(true)),
        "get_workspace_mapping_prompt_enabled" => Ok(json!(true)),
        "get_log_retention_days" => Ok(json!(30)),
        "get_startup_settings" => Ok(json!({
            "autoLaunch": false, "startMinimized": false, "closeToTray": false
        })),
        "get_version" => Ok(json!(env!("CARGO_PKG_VERSION"))),

        // ---- Benign no-ops the UI calls unconditionally on startup ----
        // Desktop-only in effect, but returning a harmless value (vs. 501) keeps
        // the web admin's boot free of caught-but-noisy errors.
        "flush_pending_deep_link" | "take_pending_port_conflict" => Ok(Value::Null),
        "refresh_oauth_tokens_on_startup" => Ok(json!({
            "servers_checked": 0, "tokens_refreshed": 0, "refresh_failed": 0
        })),

        // ---- Everything else: desktop/OS-bound or gateway-lifecycle-owning ----
        other if is_known_desktop_command(other) => Err(RpcError::desktop_only(other)),
        other => Err(RpcError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown command '{other}'"),
        }),
    }
}

// --- helpers -------------------------------------------------------------

fn err<E: std::fmt::Display>(e: E) -> RpcError {
    RpcError::internal(e.to_string())
}
fn to_val<T: serde::Serialize>(v: &T) -> RpcResult {
    serde_json::to_value(v).map_err(err)
}
fn parse_uuid(s: &str) -> Result<Uuid, RpcError> {
    Uuid::parse_str(s.trim()).map_err(|_| RpcError::bad(format!("'{s}' is not a UUID")))
}

fn binding_from_input(input: &Value) -> Result<WorkspaceBinding, RpcError> {
    let root = arg_str(input, "workspace_root")?;
    let space_id = parse_uuid(&arg_str(input, "space_id")?)?;
    let fs_ids: Vec<String> = input
        .get("feature_set_ids")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let is_id = input.get("binding_type").and_then(Value::as_str) == Some("id");
    Ok(if is_id {
        WorkspaceBinding::new_id(root.trim(), space_id, fs_ids)
    } else {
        WorkspaceBinding::new_multi(root.trim(), space_id, fs_ids)
    })
}

async fn default_space_id(app_state: &AppState) -> Result<Uuid, RpcError> {
    app_state
        .services
        .dependencies
        .space_repo
        .get_default()
        .await
        .map_err(err)?
        .map(|s| s.id)
        .ok_or_else(|| RpcError::bad("no default Space"))
}

async fn emit_space_created(app_state: &AppState, space: &Space) {
    app_state.gateway_state.read().await.emit_domain_event(
        mcpmux_core::DomainEvent::SpaceCreated {
            space_id: space.id,
            name: space.name.clone(),
            icon: space.icon.clone(),
        },
    );
}
async fn emit_fs_created(app_state: &AppState, space_id: Uuid, fs: &FeatureSet) {
    app_state.gateway_state.read().await.emit_domain_event(
        mcpmux_core::DomainEvent::FeatureSetCreated {
            space_id,
            feature_set_id: fs.id.clone(),
            name: fs.name.clone(),
            feature_set_type: Some("custom".to_string()),
        },
    );
}
async fn emit_binding_changed(app_state: &AppState, space_id: Uuid, root: &str) {
    app_state.gateway_state.read().await.emit_domain_event(
        mcpmux_core::DomainEvent::WorkspaceBindingChanged {
            space_id,
            workspace_root: root.to_string(),
        },
    );
}
async fn emit_client_changed(app_state: &AppState, client_id: &str) {
    app_state.gateway_state.read().await.emit_domain_event(
        mcpmux_core::DomainEvent::ClientRegistered {
            client_id: client_id.to_string(),
            client_name: String::new(),
            registration_type: None,
        },
    );
}

/// Recognised commands that are desktop/OS-bound or drive the gateway's own
/// lifecycle (which the serve process owns), so they are intentionally not
/// served headless. Kept explicit so a genuinely unknown command still 404s.
fn is_known_desktop_command(cmd: &str) -> bool {
    const DESKTOP_ONLY: &[&str] = &[
        // OS / windowing / tray / deep links
        "open_url",
        "open_space_config_file",
        "refresh_tray_menu",
        // Updater / autostart
        "get_auto_install_updates",
        "set_auto_install_updates",
        "get_update_channel",
        "set_update_channel",
        "update_startup_settings",
        // On-disk client-config editing (belongs to the machine the client runs on)
        "add_to_cursor",
        "add_to_vscode",
        "backup_existing_config",
        "export_config_to_file",
        "preview_config_export",
        "check_config_exists",
        "get_config_paths",
        "remove_server_from_config",
        "install_workspace_mcp_config",
        "generate_workspace_config_snippet",
        "list_workspace_install_clients",
        "read_space_config",
        "save_space_config",
        // Gateway lifecycle — owned by the serve process, not togglable via API
        "start_gateway",
        "stop_gateway",
        "restart_gateway",
        "probe_gateway_start",
        "set_gateway_port",
        "reset_gateway_port",
        "set_gateway_public_base_url",
        "reset_gateway_public_base_url",
        "set_gateway_network_access",
        "set_gateway_auth_disabled",
        "set_gateway_host_allowlist",
        "generate_gateway_config",
        // Server install / connection lifecycle + registry network ops
        "install_server",
        "uninstall_server",
        "set_server_enabled",
        "save_server_inputs",
        "connect_server",
        "disconnect_server",
        "list_connected_servers",
        "connect_all_enabled_servers",
        "retry_connection",
        "logout_server",
        "get_pool_stats",
        "get_server_statuses",
        "get_server_definition",
        "discover_servers",
        "search_servers",
        "refresh_registry",
        "is_registry_offline",
        "get_registry_home_config",
        "get_registry_ui_config",
        "list_builtin_servers",
        "set_builtin_server_enabled",
        "set_builtin_tool_enabled",
        "seed_server_features",
        "list_server_features",
        "list_server_features_by_server",
        "list_server_features_by_type",
        "get_server_feature",
        // Upstream-server OAuth flows (interactive, host-bound)
        "start_auth_v",
        "cancel_auth_v",
        "enable_server_v",
        "disable_server_v",
        "disconnect_server_v",
        "set_server_oauth_connected",
        "approve_oauth_client",
        "approve_oauth_consent",
        "get_pending_consent",
        // Meta-tool approvals (need an interactive approver)
        "get_meta_tools_require_approval_unused",
        "set_meta_tools_require_approval",
        "list_meta_tool_grants",
        "revoke_meta_tool_grant",
        "respond_to_meta_tool_approval",
        // Client API keys + pairing (pairing is minted from the trusted host)
        "create_client",
        "create_client_api_key",
        "list_client_api_keys",
        "revoke_client_api_key",
        "register_api_key_client",
        "mint_pairing_token",
        "init_preset_clients",
        "grant_oauth_client_feature_set",
        "revoke_oauth_client_feature_set",
        // Logs (file-backed on the serve host)
        "get_server_logs",
        "get_server_log_file",
        "clear_server_logs",
        "set_log_retention_days",
        // Space base dirs + feature members + effective features + misc setters
        "add_space_base_dir",
        "remove_space_base_dir",
        "list_space_base_dirs",
        "add_feature_set_member",
        "remove_feature_set_member",
        "set_feature_set_members",
        "add_feature_to_set",
        "remove_feature_from_set",
        "get_workspace_effective_features",
        "get_workspace_mapping_prompt_enabled_unused",
        "set_workspace_mapping_prompt_enabled",
        "set_meta_tools_require_approval_unused",
        "get_log_retention_days_unused",
    ];
    DESKTOP_ONLY.contains(&cmd)
}
