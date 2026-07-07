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
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde_json::{json, Value};
use uuid::Uuid;

use mcpmux_core::{AppSettingsService, FeatureSet, WorkspaceBinding};

use super::handlers::AppState;
use super::management::{
    binding_key_for, create_space_with_builtins, delete_space_guarded, ensure_binding_key_free,
    redacted_server_json,
};

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
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);
    // The request Host feeds commands that hand out URLs (device pairing): a
    // LAN admin must receive a claim URL other devices can reach, not
    // `localhost`.
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    match dispatch(&app_state, &command, args, host.as_deref()).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => (e.status, Json(json!({ "error": e.message }))).into_response(),
    }
}

async fn dispatch(
    app_state: &AppState,
    cmd: &str,
    args: Value,
    host_header: Option<&str>,
) -> RpcResult {
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
            let icon = arg(&args, "icon")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            // Desktop-parity path: seeds builtin FeatureSets, first Space
            // becomes the default, emits SpaceCreated.
            let space = create_space_with_builtins(app_state, name.trim(), icon, None)
                .await
                .map_err(RpcError::internal)?;
            to_val(&space)
        }
        "delete_space" => {
            let id = parse_uuid(&arg_str(&args, "id")?)?;
            // Guarded like the desktop: the default Space (the resolver's
            // fallback for unmapped sessions) can never be deleted.
            delete_space_guarded(app_state, &id)
                .await
                .map_err(RpcError::bad)?;
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
            // Desktop-command guard: builtin sets (Starter, …) are immutable.
            if fs.is_builtin {
                return Err(RpcError::bad("Cannot modify builtin feature set"));
            }
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
            fs.updated_at = chrono::Utc::now();
            deps.feature_set_repo.update(&fs).await.map_err(err)?;
            // Same fan-out the desktop performs so peers + admin views refresh.
            let space_id = fs.space_id.clone().unwrap_or_else(|| "default".into());
            if let Err(e) = app_state
                .services
                .grant_service
                .notify_feature_set_modified(&space_id, &id)
                .await
            {
                tracing::warn!("[management] feature-set change fan-out failed: {e}");
            }
            to_val(&fs)
        }
        "delete_feature_set" => {
            let id = arg_str(&args, "id")?;
            // Fetch first so the deletion can be announced (desktop parity).
            let existing = deps.feature_set_repo.get(&id).await.map_err(err)?;
            deps.feature_set_repo.delete(&id).await.map_err(err)?;
            if let Some(space_uuid) = existing
                .and_then(|fs| fs.space_id)
                .and_then(|s| Uuid::parse_str(&s).ok())
            {
                app_state.gateway_state.read().await.emit_domain_event(
                    mcpmux_core::DomainEvent::FeatureSetDeleted {
                        space_id: space_uuid,
                        feature_set_id: id,
                    },
                );
            }
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
            // Best-effort desktop parity: drop the auto-mapped clientId
            // id-binding so a deleted client doesn't leave an orphan mapping.
            if let Ok(Some(b)) = deps.workspace_binding_repo.find_by_id_key(&id).await {
                if let Err(e) = deps.workspace_binding_repo.delete(&b.id).await {
                    tracing::warn!("[management] clientId mapping cleanup for {id} failed: {e}");
                }
            }
            app_state
                .gateway_state
                .read()
                .await
                .emit_domain_event(mcpmux_core::DomainEvent::ClientDeleted { client_id: id });
            Ok(Value::Null)
        }
        "update_oauth_client" => {
            // Desktop invoke shape: { clientId, settings: { client_alias } }.
            let id = arg_str(&args, "clientId").or_else(|_| arg_str(&args, "id"))?;
            let alias = arg(&args, "settings")
                .and_then(|s| s.get("client_alias"))
                .or_else(|| arg(&args, "alias"))
                .and_then(Value::as_str)
                .map(str::to_string);
            deps.inbound_client_repo
                .update_client_alias(&id, alias)
                .await
                .map_err(err)?;
            app_state.gateway_state.read().await.emit_domain_event(
                mcpmux_core::DomainEvent::ClientUpdated {
                    client_id: id.clone(),
                },
            );
            let updated = deps
                .inbound_client_repo
                .get_client(&id)
                .await
                .map_err(err)?
                .ok_or_else(|| RpcError::bad("client not found after update"))?;
            Ok(client_info_json(&updated))
        }
        "get_oauth_client_grants" => {
            let client_id = arg_str(&args, "clientId").or_else(|_| arg_str(&args, "client_id"))?;
            let space_id = arg_str(&args, "spaceId").or_else(|_| arg_str(&args, "space_id"))?;
            let grants = app_state
                .services
                .grant_service
                .get_grants_for_space(&client_id, &space_id)
                .await
                .map_err(err)?;
            to_val(&grants)
        }

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
            // Readable duplicate error instead of an opaque UNIQUE violation.
            ensure_binding_key_free(app_state, &binding.workspace_root, None)
                .await
                .map_err(RpcError::bad)?;
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
            ensure_binding_key_free(app_state, &binding.workspace_root, Some(id))
                .await
                .map_err(RpcError::bad)?;
            let existing = deps
                .workspace_binding_repo
                .get(&id)
                .await
                .map_err(err)?
                .ok_or_else(|| RpcError::bad("binding not found"))?;
            let old_space_id = existing.space_id;
            binding.id = id;
            binding.created_at = existing.created_at;
            deps.workspace_binding_repo
                .update(&binding)
                .await
                .map_err(err)?;
            // Notify the new target space; if the target moved, also the old
            // one so peers that resolved there lose the stale route.
            emit_binding_changed(app_state, binding.space_id, &binding.workspace_root).await;
            if old_space_id != binding.space_id {
                emit_binding_changed(app_state, old_space_id, &binding.workspace_root).await;
            }
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
            // Same rules as the desktop command: normalization is pure string
            // logic (no filesystem access), so headless can and must apply it —
            // the resolver only matches normalized roots.
            let path = arg_str(&args, "path").or_else(|_| arg_str(&args, "root"))?;
            match mcpmux_core::validate_workspace_root(&path) {
                mcpmux_core::WorkspaceRootValidation::Ok { normalized } => {
                    Ok(Value::String(normalized))
                }
                mcpmux_core::WorkspaceRootValidation::Empty => Err(RpcError::bad("")),
                mcpmux_core::WorkspaceRootValidation::Invalid { reason } => {
                    Err(RpcError::bad(reason))
                }
            }
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
            // Never ship decrypted credentials over the management API.
            Ok(Value::Array(
                servers.iter().map(redacted_server_json).collect(),
            ))
        }

        // ---- Gateway posture / settings (reads reflect the running config) ----
        "get_gateway_status" => {
            let active_sessions = app_state.gateway_state.read().await.sessions.len();
            let manager = &app_state.services.server_manager;
            let connected_backends = match arg(&args, "spaceId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                Some(sid) => manager.connected_count_for_space(&parse_uuid(sid)?).await,
                None => manager.connected_count().await,
            };
            Ok(json!({
                "running": true,
                "url": app_state.base_url,
                "active_sessions": active_sessions,
                "connected_backends": connected_backends,
            }))
        }
        "get_gateway_network_access" => {
            Ok(json!(app_state.gateway_state.read().await.network_bind))
        }
        "get_gateway_auth_disabled" => {
            Ok(json!(app_state.gateway_state.read().await.auth_disabled()))
        }
        "get_gateway_host_allowlist" => {
            let state = app_state.gateway_state.read().await;
            Ok(json!({
                "additionalHosts": state.additional_allowed_hosts,
                "allowAnyHost": state.allow_any_host,
            }))
        }
        "get_gateway_public_url_settings" => {
            let state = app_state.gateway_state.read().await;
            let active = state
                .public_base_url
                .clone()
                .unwrap_or_else(|| app_state.base_url.clone());
            Ok(json!({
                "configuredPublicBaseUrl": state.public_base_url,
                "activePublicBaseUrl": active,
                "localBaseUrl": app_state.base_url,
            }))
        }
        "get_gateway_port_settings" => {
            let port = app_state.gateway_state.read().await.bound_port;
            Ok(json!({
                "configuredPort": port,
                "defaultPort": mcpmux_core::DEFAULT_GATEWAY_PORT,
                "activePort": port,
            }))
        }
        "get_meta_tools_require_approval" => {
            // Same key + default as the desktop command (missing = require).
            let stored = match &deps.settings_repo {
                Some(repo) => repo.get("meta_tools.require_approval").await.map_err(err)?,
                None => None,
            };
            Ok(json!(stored.map(|v| v != "false").unwrap_or(true)))
        }
        "get_workspace_mapping_prompt_enabled" => {
            let stored = match &deps.settings_repo {
                Some(repo) => repo
                    .get("workspaces.mapping_prompt_enabled")
                    .await
                    .map_err(err)?,
                None => None,
            };
            Ok(json!(stored.map(|v| v != "false").unwrap_or(true)))
        }
        "get_log_retention_days" => match &deps.settings_repo {
            Some(repo) => {
                let settings = AppSettingsService::new(repo.clone());
                Ok(json!(settings.get_log_retention_days().await))
            }
            None => Ok(json!(30)),
        },
        "get_startup_settings" => Ok(json!({
            "autoLaunch": false, "startMinimized": false, "closeToTray": false
        })),
        "get_version" => Ok(json!(env!("CARGO_PKG_VERSION"))),

        // ---- Client API keys + device pairing (headless credential issuance) ----
        // Without these a pure headless deploy could never mint a credential,
        // making the auth-required /mcp endpoint unreachable end-to-end.
        "register_api_key_client" => {
            let name = arg_str(&args, "name")?;
            let trimmed = name.trim().to_string();
            if trimmed.is_empty() {
                return Err(RpcError::bad("Client name is required"));
            }
            let locked_space_id = arg(&args, "lockedSpaceId")
                .or_else(|| arg(&args, "locked_space_id"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let now = chrono::Utc::now().to_rfc3339();
            let client_id = format!("mcp_{}", &Uuid::new_v4().simple().to_string()[..8]);
            let client = mcpmux_storage::InboundClient {
                client_id: client_id.clone(),
                registration_type: mcpmux_storage::RegistrationType::Preregistered,
                client_name: trimmed.clone(),
                client_alias: None,
                redirect_uris: vec![],
                grant_types: vec![],
                response_types: vec![],
                token_endpoint_auth_method: "none".to_string(),
                scope: None,
                approved: true,
                logo_uri: None,
                client_uri: None,
                software_id: None,
                software_version: None,
                metadata_url: None,
                metadata_cached_at: None,
                metadata_cache_ttl: None,
                last_seen: None,
                created_at: now.clone(),
                updated_at: now,
                reports_roots: false,
                roots_capability_known: false,
            };
            deps.inbound_client_repo
                .save_client(&client)
                .await
                .map_err(err)?;
            if let Some(ref space) = locked_space_id {
                deps.inbound_client_repo
                    .set_locked_space(&client_id, Some(space))
                    .await
                    .map_err(err)?;
            }
            let (key_id, plaintext, key_prefix) = generate_api_key();
            deps.inbound_client_repo
                .create_api_key(&key_id, &client_id, &plaintext, &key_prefix, None, None)
                .await
                .map_err(err)?;
            // Best-effort desktop parity: auto-map the client to the (locked
            // or default) Space's Starter so it routes sensibly out of the box.
            if let Err(e) =
                auto_map_api_key_client(app_state, &client_id, locked_space_id.as_deref()).await
            {
                tracing::warn!("[management] auto-map for {client_id} failed (non-fatal): {e}");
            }
            Ok(json!({
                "clientId": client_id,
                "clientName": trimmed,
                "lockedSpaceId": locked_space_id,
                "apiKey": plaintext,
                "keyPrefix": key_prefix,
            }))
        }
        "create_client_api_key" => {
            let client_id = arg_str(&args, "clientId").or_else(|_| arg_str(&args, "client_id"))?;
            let label = arg(&args, "label")
                .and_then(Value::as_str)
                .map(str::to_string);
            let client = deps
                .inbound_client_repo
                .get_client(&client_id)
                .await
                .map_err(err)?
                .ok_or_else(|| RpcError::bad("Client not found"))?;
            let (key_id, plaintext, key_prefix) = generate_api_key();
            deps.inbound_client_repo
                .create_api_key(
                    &key_id,
                    &client_id,
                    &plaintext,
                    &key_prefix,
                    label.as_deref(),
                    None,
                )
                .await
                .map_err(err)?;
            let locked_space_id = deps
                .inbound_client_repo
                .get_locked_space(&client_id)
                .await
                .map_err(err)?;
            Ok(json!({
                "clientId": client_id,
                "clientName": client.client_name,
                "lockedSpaceId": locked_space_id,
                "apiKey": plaintext,
                "keyPrefix": key_prefix,
            }))
        }
        "list_client_api_keys" => {
            let client_id = arg_str(&args, "clientId").or_else(|_| arg_str(&args, "client_id"))?;
            let keys = deps
                .inbound_client_repo
                .list_api_keys(&client_id)
                .await
                .map_err(err)?;
            Ok(Value::Array(
                keys.into_iter()
                    .map(|k| {
                        json!({
                            "keyId": k.key_id,
                            "keyPrefix": k.key_prefix,
                            "label": k.label,
                            "revoked": k.revoked,
                            "lastUsedAt": k.last_used_at,
                            "createdAt": k.created_at,
                        })
                    })
                    .collect(),
            ))
        }
        "revoke_client_api_key" => {
            let key_id = arg_str(&args, "keyId").or_else(|_| arg_str(&args, "key_id"))?;
            deps.inbound_client_repo
                .revoke_api_key(&key_id)
                .await
                .map_err(err)?;
            Ok(Value::Null)
        }
        "mint_pairing_token" => {
            let ttl = super::pairing::DEFAULT_PAIRING_TTL;
            let (public_base_url, network_bind, token) = {
                let state = app_state.gateway_state.read().await;
                (
                    state.public_base_url.clone(),
                    state.network_bind,
                    state.pairing_tokens().mint(ttl),
                )
            };
            // The admin reached us on some host — that host is what other
            // devices on the same network can reach (the desktop app uses its
            // LAN IP instead; a headless server doesn't know its own).
            let base = super::handlers::effective_base_url(
                public_base_url.as_deref(),
                network_bind,
                host_header,
                &app_state.base_url,
            );
            Ok(json!({
                "token": token,
                "claimUrl": format!("{base}/pair?token={token}"),
                "lanBaseUrl": base,
                "expiresInSecs": ttl.as_secs(),
            }))
        }

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
    // Desktop-command parity: path roots are normalized + validated (the
    // resolver only matches normalized roots); id keys must be non-empty.
    let key = binding_key_for(&root, is_id).map_err(RpcError::bad)?;
    Ok(if is_id {
        WorkspaceBinding::new_id(key, space_id, fs_ids)
    } else {
        WorkspaceBinding::new_multi(key, space_id, fs_ids)
    })
}

/// Generate a strong API key: `mcpk_` + 256 bits of v4-UUID randomness.
/// Returns `(key_id, plaintext, key_prefix)`; only the hash is ever stored.
/// Mirror of the desktop command's generator.
fn generate_api_key() -> (String, String, String) {
    let key_id = Uuid::new_v4().to_string();
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let plaintext = format!("mcpk_{secret}");
    let key_prefix: String = plaintext.chars().take(13).collect(); // "mcpk_" + 8 chars
    (key_id, plaintext, key_prefix)
}

/// Auto-create a clientId-keyed `id` mapping pointing at the (locked or
/// default) Space's Starter FeatureSet (desktop parity), so a fresh API-key
/// client routes somewhere sensible and the mapping is editable in the UI.
async fn auto_map_api_key_client(
    app_state: &AppState,
    client_id: &str,
    locked_space_id: Option<&str>,
) -> Result<(), String> {
    let deps = &app_state.services.dependencies;
    let space_id = match locked_space_id {
        Some(s) => Uuid::parse_str(s).map_err(|e| e.to_string())?,
        None => {
            deps.space_repo
                .get_default()
                .await
                .map_err(|e| e.to_string())?
                .ok_or("no default Space configured")?
                .id
        }
    };
    let starter = deps
        .feature_set_repo
        .get_starter_for_space(&space_id.to_string())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Space has no Starter FeatureSet")?;
    let binding = WorkspaceBinding::new_id(client_id.to_string(), space_id, vec![starter.id]);
    deps.workspace_binding_repo
        .create(&binding)
        .await
        .map_err(|e| e.to_string())
}

/// The `OAuthClientInfo` shape the desktop command returns (snake_case).
fn client_info_json(c: &mcpmux_storage::InboundClient) -> Value {
    json!({
        "client_id": c.client_id,
        "registration_type": c.registration_type.as_str(),
        "client_name": c.client_name,
        "client_alias": c.client_alias,
        "redirect_uris": c.redirect_uris,
        "scope": c.scope,
        "approved": c.approved,
        "logo_uri": c.logo_uri,
        "client_uri": c.client_uri,
        "software_id": c.software_id,
        "software_version": c.software_version,
        "metadata_url": c.metadata_url,
        "metadata_cached_at": c.metadata_cached_at,
        "metadata_cache_ttl": c.metadata_cache_ttl,
        "last_seen": c.last_seen,
        "created_at": c.created_at,
        "reports_roots": c.reports_roots,
        "roots_capability_known": c.roots_capability_known,
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
        // Client presets + grant writes (API keys + pairing are served
        // headless above — a headless deploy must be able to issue credentials)
        "create_client",
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
