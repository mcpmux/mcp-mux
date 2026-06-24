//! Write admin bridge endpoints for Phase 6 parity.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::Utc;
use mcpmux_core::{
    validate_workspace_root as validate_workspace_root_path, AppSettingsService, Client,
    FeatureSet, FeatureSetMember, MemberMode, MemberType, ServerSource, UpdatePolicy,
    WorkspaceAppearance, WorkspaceBinding, WorkspaceRootValidation,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::admin::bridge_context::AdminBridgeCtx;
use crate::admin::command_bridge::read::{
    as_json, space_ctx, to_client_response, to_feature_set_response,
    to_workspace_appearance_response, to_workspace_binding_response,
};
use crate::admin::command_bridge::space::{self, UpdateSpaceInput};

const LOCAL_ICON_PREFIX: &str = "local:workspace-icons/";
const WORKSPACE_ICON_DIR: &str = "workspace-icons";
const DEFAULT_UPDATE_POLICY_KEY: &str = "servers.default_update_policy";

#[derive(Debug, Deserialize)]
pub struct CreateSpaceBody {
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveSpaceConfigBody {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateFeatureSetBody {
    pub name: String,
    pub space_id: String,
    pub description: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFeatureSetBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberBody {
    pub member_type: String,
    pub member_id: String,
    pub mode: Option<String>,
    pub surfaced: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SetMembersBody {
    pub members: Vec<AddMemberBody>,
}

#[derive(Debug, Deserialize)]
pub struct CreateClientBody {
    pub name: String,
    pub client_type: String,
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceBindingBody {
    pub workspace_root: String,
    pub label: Option<String>,
    pub icon: Option<String>,
    pub space_id: String,
    pub feature_set_ids: Vec<String>,
    pub client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceAppearanceBody {
    pub workspace_root: String,
    pub icon: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSettingsBody {
    pub auto_launch: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerUpdateSettingsBody {
    pub default_update_policy: String,
}

#[derive(Debug, Deserialize)]
pub struct UninstallServerBody {
    pub space_id: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallServerBody {
    pub id: String,
    pub space_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveServerInputsBody {
    pub input_values: HashMap<String, String>,
    pub space_id: String,
    pub env_overrides: Option<HashMap<String, String>>,
    pub args_append: Option<Vec<String>>,
    pub extra_headers: Option<HashMap<String, String>>,
    pub default_params: Option<HashMap<String, Value>>,
    pub default_params_strategy: Option<String>,
    pub display_name_override: Option<String>,
    pub update_policy: Option<String>,
    pub pinned_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetServerDisplayNameBody {
    pub space_id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetServerOAuthConnectedBody {
    pub space_id: String,
    pub connected: bool,
}

#[derive(Debug, Deserialize)]
pub struct ServerConnectionBody {
    pub space_id: String,
    pub server_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DisconnectServerBody {
    pub space_id: String,
    pub server_id: String,
    pub logout: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GatewayStartBody {
    pub port: Option<u16>,
    pub allow_dynamic_fallback: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GatewayPortBody {
    pub port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPublicUrlBody {
    pub public_url: String,
}

#[derive(Debug, Deserialize)]
pub struct CloneServerBody {
    pub space_id: String,
    pub source_server_id: String,
    pub suffix: String,
    pub alias: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UploadIconBody {
    pub source_path: String,
}

#[derive(Debug, Deserialize)]
pub struct MetaToolApprovalBody {
    pub request_id: String,
    pub client_id: String,
    pub tool_name: String,
    pub decision: String,
}

#[derive(Debug, Deserialize)]
pub struct MetaToolRevokeBody {
    pub client_id: String,
    pub tool_name: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuthClientUpdateBody {
    pub client_alias: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthGrantBody {
    pub space_id: String,
    pub feature_set_id: String,
}

#[derive(Debug, Deserialize)]
pub struct LogRetentionBody {
    pub days: u32,
}

#[derive(Debug, Deserialize)]
pub struct MetaToolsEnabledBody {
    pub enabled: bool,
}

fn normalize_label(label: &Option<String>) -> Option<String> {
    label
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn normalize_workspace_root(raw: &str) -> Result<String> {
    match validate_workspace_root_path(raw) {
        WorkspaceRootValidation::Empty => Err(anyhow!("workspace_root cannot be empty")),
        WorkspaceRootValidation::Ok { normalized } => Ok(normalized),
        WorkspaceRootValidation::Invalid { reason } => Err(anyhow!(reason)),
    }
}

fn validate_feature_set_ids(ids: &[String]) -> Result<Vec<String>> {
    let cleaned: Vec<String> = ids
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        return Err(anyhow!("at least one feature_set_id is required"));
    }
    let mut seen = std::collections::HashSet::new();
    Ok(cleaned
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect())
}

fn local_ref_to_file_name(icon_ref: &str) -> Option<&str> {
    let file_name = icon_ref.strip_prefix(LOCAL_ICON_PREFIX)?;
    if file_name.contains('/') || file_name.contains('\\') {
        return None;
    }
    if Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        != Some("png")
    {
        return None;
    }
    Some(file_name)
}

async fn maybe_remove_orphaned_icon(ctx: &AdminBridgeCtx, icon_ref: Option<&str>) -> Result<()> {
    let Some(icon_ref) = icon_ref else {
        return Ok(());
    };
    let Some(file_name) = local_ref_to_file_name(icon_ref) else {
        return Ok(());
    };

    let appearances = ctx.workspace_appearance_repository.list().await?;
    if appearances.iter().any(|a| a.icon == icon_ref) {
        return Ok(());
    }

    let file_path = ctx.data_dir.join(WORKSPACE_ICON_DIR).join(file_name);
    match tokio::fs::remove_file(&file_path).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(anyhow!("failed to remove orphaned icon file: {err}")),
    }
    Ok(())
}

fn parse_member_type(value: &str) -> MemberType {
    if value == "feature_set" {
        MemberType::FeatureSet
    } else {
        MemberType::Feature
    }
}

fn parse_member_mode(value: Option<&str>) -> MemberMode {
    value
        .and_then(MemberMode::parse)
        .unwrap_or(MemberMode::Include)
}

async fn get_feature_set_with_members(ctx: &AdminBridgeCtx, id: &str) -> Result<FeatureSet> {
    ctx.feature_set_repository
        .get_with_members(id)
        .await?
        .ok_or_else(|| anyhow!("Feature set not found"))
}

async fn save_feature_set(ctx: &AdminBridgeCtx, mut feature_set: FeatureSet) -> Result<Value> {
    feature_set.updated_at = Utc::now();
    ctx.feature_set_repository.update(&feature_set).await?;
    Ok(to_feature_set_response(feature_set))
}

// --- Spaces ---

pub async fn create_space(ctx: &AdminBridgeCtx, body: CreateSpaceBody) -> Result<Value> {
    let space = space::create_space(&space_ctx(ctx), body.name, body.icon).await?;
    as_json(space)
}

pub async fn update_space(
    ctx: &AdminBridgeCtx,
    id: String,
    input: UpdateSpaceInput,
) -> Result<Value> {
    let id = Uuid::parse_str(&id)?;
    let space = space::update_space(&space_ctx(ctx), id, input).await?;
    as_json(space)
}

pub async fn delete_space(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let id = Uuid::parse_str(&id)?;
    space::delete_space(&space_ctx(ctx), id).await?;
    Ok(json!({ "ok": true }))
}

pub async fn save_space_config(
    ctx: &AdminBridgeCtx,
    space_id: String,
    body: SaveSpaceConfigBody,
) -> Result<Value> {
    space::save_space_config(&space_ctx(ctx), &space_id, &body.content).await?;
    Ok(json!({ "ok": true }))
}

pub async fn remove_server_from_config(
    ctx: &AdminBridgeCtx,
    space_id: String,
    server_id: String,
) -> Result<Value> {
    let removed = space::remove_server_from_config(&space_ctx(ctx), &space_id, &server_id).await?;
    as_json(removed)
}

// --- Feature sets ---

pub async fn create_feature_set(ctx: &AdminBridgeCtx, body: CreateFeatureSetBody) -> Result<Value> {
    let set = ctx
        .services
        .permission()
        .create_feature_set(&body.space_id, &body.name, body.description, body.icon)
        .await?;
    Ok(to_feature_set_response(set))
}

pub async fn update_feature_set(
    ctx: &AdminBridgeCtx,
    id: String,
    body: UpdateFeatureSetBody,
) -> Result<Value> {
    let set = ctx
        .services
        .permission()
        .update_feature_set(id.as_str(), body.name, body.description, body.icon)
        .await?;
    Ok(to_feature_set_response(set))
}

pub async fn delete_feature_set(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    ctx.services.permission().delete_feature_set(&id).await?;
    Ok(json!({ "ok": true }))
}

pub async fn add_feature_set_member(
    ctx: &AdminBridgeCtx,
    feature_set_id: String,
    body: AddMemberBody,
) -> Result<Value> {
    let mut feature_set = get_feature_set_with_members(ctx, &feature_set_id).await?;
    let fs_type = feature_set.feature_set_type.as_str();
    if fs_type != "starter" && fs_type != "default" && fs_type != "custom" {
        return Err(anyhow!(
            "Cannot modify members of '{fs_type}' type feature set"
        ));
    }

    let member_type = parse_member_type(&body.member_type);
    let mode = parse_member_mode(body.mode.as_deref());

    if feature_set
        .members
        .iter()
        .any(|m| m.member_type == member_type && m.member_id == body.member_id)
    {
        return Err(anyhow!("Member already exists in this feature set"));
    }
    if member_type == MemberType::FeatureSet && body.member_id == feature_set_id {
        return Err(anyhow!("Cannot add a feature set to itself"));
    }
    if member_type == MemberType::FeatureSet {
        if let Some(target) = ctx.feature_set_repository.get(&body.member_id).await? {
            let target_type = target.feature_set_type.as_str();
            if target_type == "all" || target_type == "default" {
                return Err(anyhow!(
                    "Cannot include '{target_type}' type feature sets in other feature sets"
                ));
            }
        }
    }

    feature_set.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: feature_set_id.clone(),
        member_type,
        member_id: body.member_id,
        mode,
        surfaced: body.surfaced.unwrap_or(false),
    });
    save_feature_set(ctx, feature_set).await
}

pub async fn remove_feature_set_member(
    ctx: &AdminBridgeCtx,
    feature_set_id: String,
    member_id: String,
) -> Result<Value> {
    let mut feature_set = get_feature_set_with_members(ctx, &feature_set_id).await?;
    if feature_set.is_builtin {
        return Err(anyhow!("Cannot modify builtin feature set"));
    }
    feature_set.members.retain(|m| m.id != member_id);
    save_feature_set(ctx, feature_set).await
}

pub async fn set_feature_set_members(
    ctx: &AdminBridgeCtx,
    feature_set_id: String,
    body: SetMembersBody,
) -> Result<Value> {
    let mut feature_set = get_feature_set_with_members(ctx, &feature_set_id).await?;
    let fs_type = feature_set.feature_set_type.as_str();
    if fs_type != "starter" && fs_type != "default" && fs_type != "custom" {
        return Err(anyhow!(
            "Cannot modify members of '{fs_type}' type feature set"
        ));
    }

    feature_set.members = body
        .members
        .into_iter()
        .filter(|m| !(m.member_type == "feature_set" && m.member_id == feature_set_id))
        .map(|input| FeatureSetMember {
            id: Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.clone(),
            member_type: parse_member_type(&input.member_type),
            member_id: input.member_id,
            mode: parse_member_mode(input.mode.as_deref()),
            surfaced: input.surfaced.unwrap_or(false),
        })
        .collect();
    save_feature_set(ctx, feature_set).await
}

// --- Clients ---

pub async fn create_client(ctx: &AdminBridgeCtx, body: CreateClientBody) -> Result<Value> {
    let client = ctx
        .services
        .client()
        .create(&body.name, &body.client_type)
        .await?;
    Ok(to_client_response(client))
}

pub async fn delete_client(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let id = Uuid::parse_str(&id)?;
    ctx.services.client().delete(id).await?;
    Ok(json!({ "ok": true }))
}

pub async fn init_preset_clients(ctx: &AdminBridgeCtx) -> Result<Value> {
    let existing = ctx.services.client().list().await?;
    if !existing.iter().any(|c| c.client_type == "cursor") {
        let cursor = Client::cursor();
        ctx.services
            .client()
            .create(&cursor.name, &cursor.client_type)
            .await?;
    }
    if !existing.iter().any(|c| c.client_type == "vscode") {
        let vscode = Client::vscode();
        ctx.services
            .client()
            .create(&vscode.name, &vscode.client_type)
            .await?;
    }
    if !existing.iter().any(|c| c.client_type == "claude") {
        let claude = Client::claude_desktop();
        ctx.services
            .client()
            .create(&claude.name, &claude.client_type)
            .await?;
    }
    Ok(json!({ "ok": true }))
}

// --- Workspace bindings ---

pub async fn create_workspace_binding(
    ctx: &AdminBridgeCtx,
    body: WorkspaceBindingBody,
) -> Result<Value> {
    let space_id = Uuid::parse_str(&body.space_id)?;
    let feature_set_ids = validate_feature_set_ids(&body.feature_set_ids)?;
    let normalized = normalize_workspace_root(&body.workspace_root)?;

    let mut binding = WorkspaceBinding::new_multi(normalized.clone(), space_id, feature_set_ids);
    binding.label = normalize_label(&body.label);
    binding.client_id = body.client_id.clone();

    ctx.workspace_binding_repository.create(&binding).await?;

    Ok(to_workspace_binding_response(binding))
}

pub async fn update_workspace_binding(
    ctx: &AdminBridgeCtx,
    id: String,
    body: WorkspaceBindingBody,
) -> Result<Value> {
    let id_uuid = Uuid::parse_str(&id)?;
    let space_id = Uuid::parse_str(&body.space_id)?;
    let feature_set_ids = validate_feature_set_ids(&body.feature_set_ids)?;
    let normalized = normalize_workspace_root(&body.workspace_root)?;

    let existing = ctx
        .workspace_binding_repository
        .get(&id_uuid)
        .await?
        .ok_or_else(|| anyhow!("binding not found: {id}"))?;

    let label = if body.label.is_some() {
        normalize_label(&body.label)
    } else {
        existing.label.clone()
    };

    let updated = WorkspaceBinding {
        id: existing.id,
        workspace_root: normalized,
        client_id: body.client_id.or(existing.client_id),
        label,
        space_id,
        feature_set_ids,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };

    ctx.workspace_binding_repository.update(&updated).await?;

    Ok(to_workspace_binding_response(updated))
}

pub async fn delete_workspace_binding(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let id_uuid = Uuid::parse_str(&id)?;
    ctx.workspace_binding_repository.delete(&id_uuid).await?;
    Ok(json!({ "ok": true }))
}

// --- Workspace appearances ---

pub async fn upsert_workspace_appearance(
    ctx: &AdminBridgeCtx,
    body: WorkspaceAppearanceBody,
) -> Result<Value> {
    let workspace_root = normalize_workspace_root(&body.workspace_root)?;
    let icon = body.icon.trim();
    if icon.is_empty() {
        return Err(anyhow!("icon cannot be empty"));
    }

    let previous_icon = ctx
        .workspace_appearance_repository
        .get(&workspace_root)
        .await?
        .map(|a| a.icon);

    let appearance = WorkspaceAppearance::new(workspace_root, icon.to_string());
    ctx.workspace_appearance_repository
        .upsert(&appearance)
        .await?;

    if let Some(previous_icon) = previous_icon {
        if previous_icon != appearance.icon {
            maybe_remove_orphaned_icon(ctx, Some(previous_icon.as_str())).await?;
        }
    }

    Ok(to_workspace_appearance_response(appearance))
}

pub async fn delete_workspace_appearance(
    ctx: &AdminBridgeCtx,
    workspace_root: String,
) -> Result<Value> {
    let normalized = normalize_workspace_root(&workspace_root)?;
    let previous = ctx.workspace_appearance_repository.get(&normalized).await?;
    ctx.workspace_appearance_repository
        .delete(&normalized)
        .await?;
    if let Some(previous) = previous {
        maybe_remove_orphaned_icon(ctx, Some(previous.icon.as_str())).await?;
    }
    Ok(json!({ "ok": true }))
}

pub async fn upload_workspace_icon(_ctx: &AdminBridgeCtx, _body: UploadIconBody) -> Result<Value> {
    // ponytail: workspace icon upload requires `image` crate, lands in Phase 7
    Err(anyhow!("Workspace icon upload not yet available"))
}

// --- Settings ---

pub async fn update_startup_settings(
    ctx: &AdminBridgeCtx,
    body: StartupSettingsBody,
) -> Result<Value> {
    ctx.settings_repository
        .set("startup.autostart_configured", "true")
        .await?;
    ctx.settings_repository
        .set("startup.start_minimized", &body.start_minimized.to_string())
        .await?;
    ctx.settings_repository
        .set("ui.close_to_tray", &body.close_to_tray.to_string())
        .await?;
    let _ = body.auto_launch;
    Ok(json!({ "ok": true }))
}

pub async fn update_server_update_settings(
    ctx: &AdminBridgeCtx,
    body: ServerUpdateSettingsBody,
) -> Result<Value> {
    let policy = UpdatePolicy::from_db_str(&body.default_update_policy);
    ctx.settings_repository
        .set(DEFAULT_UPDATE_POLICY_KEY, policy.as_db_str())
        .await?;
    Ok(json!({ "ok": true }))
}

pub async fn set_meta_tools_enabled(ctx: &AdminBridgeCtx, enabled: bool) -> Result<Value> {
    ctx.settings_repository
        .set(
            "gateway.meta_tools_enabled",
            if enabled { "true" } else { "false" },
        )
        .await?;
    Ok(json!({ "ok": true }))
}

// --- Logs ---

pub async fn clear_server_logs(ctx: &AdminBridgeCtx, server_id: String) -> Result<Value> {
    let default_space = ctx
        .space_service
        .get_default()
        .await?
        .ok_or_else(|| anyhow!("No default space found"))?;
    ctx.server_log_manager
        .clear_logs(&default_space.id.to_string(), &server_id)
        .await?;
    Ok(json!({ "ok": true }))
}

pub async fn set_log_retention_days(ctx: &AdminBridgeCtx, body: LogRetentionBody) -> Result<Value> {
    let settings = AppSettingsService::new(ctx.settings_repository.clone());
    settings.set_log_retention_days(body.days).await?;
    if body.days > 0 {
        let _ = ctx
            .server_log_manager
            .cleanup_logs_older_than(body.days)
            .await;
    }
    Ok(json!({ "ok": true }))
}

// --- Registry / servers ---

pub async fn refresh_registry(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.server_discovery.refresh().await?;
    let servers = ctx.server_discovery.list().await;
    let mut count = 0_u32;
    for server in servers {
        if let ServerSource::UserSpace { space_id, .. } = &server.source {
            if ctx
                .services
                .server()
                .get(space_id, &server.id)
                .await?
                .is_some()
            {
                continue;
            }
            let space_uuid = Uuid::parse_str(space_id)?;
            ctx.services
                .server()
                .install(space_uuid, &server.id, &server, HashMap::new())
                .await?;
            count += 1;
        }
    }
    as_json(count)
}

pub async fn install_server(ctx: &AdminBridgeCtx, body: InstallServerBody) -> Result<Value> {
    ctx.server_discovery.refresh_if_needed().await?;
    let definition = ctx
        .server_discovery
        .get(&body.id)
        .await
        .ok_or_else(|| anyhow!("Server definition not found"))?;
    let space_uuid = Uuid::parse_str(&body.space_id)?;
    let installed = ctx
        .services
        .server()
        .install(space_uuid, &body.id, &definition, HashMap::new())
        .await?;
    as_json(installed)
}

pub async fn uninstall_server(ctx: &AdminBridgeCtx, id: String, space_id: String) -> Result<Value> {
    let space_uuid = Uuid::parse_str(&space_id)?;
    ctx.services.server().uninstall(space_uuid, &id).await?;
    Ok(json!({ "ok": true }))
}

pub async fn save_server_inputs(
    ctx: &AdminBridgeCtx,
    id: String,
    body: SaveServerInputsBody,
) -> Result<Value> {
    let space_uuid = Uuid::parse_str(&body.space_id)?;
    let installed = ctx
        .services
        .server()
        .update_config(
            space_uuid,
            &id,
            body.input_values,
            body.env_overrides,
            body.args_append,
            body.extra_headers,
            None,
            None,
        )
        .await?;
    as_json(installed)
}

pub async fn set_server_display_name(
    _ctx: &AdminBridgeCtx,
    _id: String,
    _body: SetServerDisplayNameBody,
) -> Result<Value> {
    // ponytail: set_display_name_override lands in Phase 6
    Err(anyhow!("Server display name override not yet available"))
}

pub async fn set_server_oauth_connected(
    ctx: &AdminBridgeCtx,
    id: String,
    body: SetServerOAuthConnectedBody,
) -> Result<Value> {
    let space_uuid = Uuid::parse_str(&body.space_id)?;
    ctx.services
        .server()
        .set_oauth_connected(space_uuid, &id, body.connected)
        .await?;
    Ok(json!({ "ok": true }))
}

pub async fn clone_server(_ctx: &AdminBridgeCtx, _body: CloneServerBody) -> Result<Value> {
    // ponytail: clone_server lands in Phase 6
    Err(anyhow!("Server cloning not yet available"))
}

// --- Gateway writes (delegated) ---

pub async fn start_gateway(ctx: &AdminBridgeCtx, body: GatewayStartBody) -> Result<Value> {
    ctx.gateway_writes
        .start_gateway(body.port, body.allow_dynamic_fallback)
        .await
}

pub async fn stop_gateway(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_writes.stop_gateway().await
}

pub async fn restart_gateway(ctx: &AdminBridgeCtx, body: GatewayStartBody) -> Result<Value> {
    ctx.gateway_writes
        .restart_gateway(body.port, body.allow_dynamic_fallback)
        .await
}

pub async fn disconnect_server(ctx: &AdminBridgeCtx, body: DisconnectServerBody) -> Result<Value> {
    ctx.gateway_writes
        .disconnect_server(body.server_id, body.space_id, body.logout)
        .await
}

pub async fn connect_all_enabled_servers(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_writes.connect_all_enabled_servers().await
}

pub async fn refresh_oauth_tokens_on_startup(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_writes.refresh_oauth_tokens_on_startup().await
}

pub async fn set_gateway_port(ctx: &AdminBridgeCtx, body: GatewayPortBody) -> Result<Value> {
    ctx.gateway_writes.set_gateway_port(body.port).await
}

pub async fn set_gateway_public_url(
    _ctx: &AdminBridgeCtx,
    _body: GatewayPublicUrlBody,
) -> Result<Value> {
    // ponytail: public URL persistence lands in Phase 5 (AppSettingsService extension)
    Err(anyhow!(
        "Gateway public URL configuration not yet available"
    ))
}

pub async fn enable_server_v2(ctx: &AdminBridgeCtx, body: ServerConnectionBody) -> Result<Value> {
    ctx.gateway_writes
        .enable_server_v2(body.space_id, body.server_id)
        .await
}

pub async fn disable_server_v2(ctx: &AdminBridgeCtx, body: ServerConnectionBody) -> Result<Value> {
    ctx.gateway_writes
        .disable_server_v2(body.space_id, body.server_id)
        .await
}

pub async fn start_auth_v2(ctx: &AdminBridgeCtx, body: ServerConnectionBody) -> Result<Value> {
    ctx.gateway_writes
        .start_auth_v2(body.space_id, body.server_id)
        .await
}

pub async fn cancel_auth_v2(ctx: &AdminBridgeCtx, body: ServerConnectionBody) -> Result<Value> {
    ctx.gateway_writes
        .cancel_auth_v2(body.space_id, body.server_id)
        .await
}

pub async fn retry_connection(ctx: &AdminBridgeCtx, body: ServerConnectionBody) -> Result<Value> {
    ctx.gateway_writes
        .retry_connection(body.space_id, body.server_id)
        .await
}

pub async fn update_server_package(
    ctx: &AdminBridgeCtx,
    body: ServerConnectionBody,
) -> Result<Value> {
    ctx.gateway_writes
        .update_server_package(body.space_id, body.server_id)
        .await
}

pub async fn logout_server(ctx: &AdminBridgeCtx, body: ServerConnectionBody) -> Result<Value> {
    ctx.gateway_writes
        .logout_server(body.space_id, body.server_id)
        .await
}

pub async fn respond_to_meta_tool_approval(
    ctx: &AdminBridgeCtx,
    body: MetaToolApprovalBody,
) -> Result<Value> {
    ctx.gateway_writes
        .respond_to_meta_tool_approval(
            body.request_id,
            body.client_id,
            body.tool_name,
            body.decision,
        )
        .await
}

pub async fn revoke_meta_tool_grant(
    ctx: &AdminBridgeCtx,
    body: MetaToolRevokeBody,
) -> Result<Value> {
    ctx.gateway_writes
        .revoke_meta_tool_grant(body.client_id, body.tool_name)
        .await
}

pub async fn update_oauth_client(
    ctx: &AdminBridgeCtx,
    client_id: String,
    body: OAuthClientUpdateBody,
) -> Result<Value> {
    ctx.gateway_writes
        .update_oauth_client(client_id, body.client_alias)
        .await
}

pub async fn delete_oauth_client(ctx: &AdminBridgeCtx, client_id: String) -> Result<Value> {
    ctx.gateway_writes.delete_oauth_client(client_id).await
}

pub async fn grant_oauth_client_feature_set(
    ctx: &AdminBridgeCtx,
    client_id: String,
    body: OAuthGrantBody,
) -> Result<Value> {
    ctx.gateway_writes
        .grant_oauth_client_feature_set(client_id, body.space_id, body.feature_set_id)
        .await
}

pub async fn revoke_oauth_client_feature_set(
    ctx: &AdminBridgeCtx,
    client_id: String,
    body: OAuthGrantBody,
) -> Result<Value> {
    ctx.gateway_writes
        .revoke_oauth_client_feature_set(client_id, body.space_id, body.feature_set_id)
        .await
}

/// Probe npm/PyPI for a single installed server package update.
pub async fn check_server_version(
    _ctx: &AdminBridgeCtx,
    _body: ServerConnectionBody,
) -> Result<Value> {
    // ponytail: version probing lands in Phase 5
    Err(anyhow!("Server version checking not yet available"))
}

/// Probe all notify/auto package-managed servers for available updates.
pub async fn check_all_server_versions(_ctx: &AdminBridgeCtx) -> Result<Value> {
    // ponytail: version probing lands in Phase 5
    Err(anyhow!("Server version checking not yet available"))
}
