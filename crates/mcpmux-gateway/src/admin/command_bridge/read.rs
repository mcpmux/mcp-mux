//! Read-only admin bridge endpoints for Phase 4 parity.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use chrono::Utc;
use mcpmux_core::{
    validate_workspace_root as validate_workspace_root_path, AppSettingsService, ConfigExporter,
    ConfigFormat, FeatureSet, FeatureSetMember, FeatureType, LogLevel, MemberMode, MemberType,
    ResolvedServer, ResolvedTransport, TransportConfig, WorkspaceRootValidation,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::admin::bridge_context::AdminBridgeCtx;
use crate::admin::command_bridge::space::{self, SpaceBridgeCtx};

const LOCAL_ICON_PREFIX: &str = "local:workspace-icons/";
const WORKSPACE_ICON_DIR: &str = "workspace-icons";

pub(crate) fn as_json<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(Into::into)
}

pub(crate) fn to_client_response(client: mcpmux_core::Client) -> Value {
    json!({
        "id": client.id.to_string(),
        "name": client.name,
        "client_type": client.client_type,
        "last_seen": client.last_seen.map(|dt| dt.to_rfc3339()),
    })
}

pub(crate) fn to_feature_set_member_response(member: &FeatureSetMember) -> Value {
    json!({
        "id": member.id,
        "feature_set_id": member.feature_set_id,
        "member_type": member.member_type.as_str(),
        "member_id": member.member_id,
        "mode": member.mode.as_str(),
        "surfaced": member.surfaced,
    })
}

pub(crate) fn to_feature_set_response(feature_set: FeatureSet) -> Value {
    json!({
        "id": feature_set.id,
        "name": feature_set.name,
        "description": feature_set.description,
        "icon": feature_set.icon,
        "space_id": feature_set.space_id,
        "feature_set_type": feature_set.feature_set_type.as_str(),
        "server_id": feature_set.server_id,
        "is_builtin": feature_set.is_builtin,
        "is_deleted": feature_set.is_deleted,
        "members": feature_set
            .members
            .iter()
            .map(to_feature_set_member_response)
            .collect::<Vec<_>>(),
    })
}

pub(crate) fn to_workspace_binding_response(binding: mcpmux_core::WorkspaceBinding) -> Value {
    json!({
        "id": binding.id.to_string(),
        "workspace_root": binding.workspace_root,
        "client_id": binding.client_id,
        "machine_id": binding.machine_id.map(|id| id.to_string()),
        "label": binding.label,
        "icon": binding.icon,
        "space_id": binding.space_id.to_string(),
        "feature_set_ids": binding.feature_set_ids,
        "created_at": binding.created_at.to_rfc3339(),
        "updated_at": binding.updated_at.to_rfc3339(),
    })
}

pub(crate) fn to_machine_response(machine: mcpmux_core::Machine) -> Value {
    json!({
        "id": machine.id.to_string(),
        "name": machine.name,
        "icon": machine.icon,
        "hostname": machine.hostname,
        "created_at": machine.created_at.to_rfc3339(),
        "updated_at": machine.updated_at.to_rfc3339(),
    })
}

pub(crate) fn to_workspace_appearance_response(
    appearance: mcpmux_core::WorkspaceAppearance,
) -> Value {
    json!({
        "workspace_root": appearance.workspace_root,
        "icon": appearance.icon,
        "updated_at": appearance.updated_at.to_rfc3339(),
    })
}

fn to_server_feature_response(feature: mcpmux_core::ServerFeature) -> Value {
    json!({
        "id": feature.id.to_string(),
        "space_id": feature.space_id,
        "server_id": feature.server_id,
        "feature_type": feature.feature_type.as_str(),
        "feature_name": feature.feature_name,
        "display_name": feature.display_name,
        "description": feature.description,
        "input_schema": feature.raw_json,
        "discovered_at": feature.discovered_at.to_rfc3339(),
        "last_seen_at": feature.last_seen_at.to_rfc3339(),
        "is_available": feature.is_available,
    })
}

fn collect_member_ids(
    feature_set: &FeatureSet,
    lookup: &HashMap<String, FeatureSet>,
    allowed: &mut HashSet<String>,
    excluded: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) {
    if !visited.insert(feature_set.id.clone()) {
        return;
    }
    for member in &feature_set.members {
        match member.member_type {
            MemberType::Feature => match member.mode {
                MemberMode::Include => {
                    allowed.insert(member.member_id.clone());
                }
                MemberMode::Exclude => {
                    excluded.insert(member.member_id.clone());
                }
            },
            MemberType::FeatureSet => {
                if let Some(nested) = lookup.get(&member.member_id) {
                    collect_member_ids(nested, lookup, allowed, excluded, visited);
                }
            }
        }
    }
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

fn icon_ref_to_path(data_dir: &Path, icon_ref: &str) -> Option<PathBuf> {
    let file_name = local_ref_to_file_name(icon_ref)?;
    Some(data_dir.join(WORKSPACE_ICON_DIR).join(file_name))
}

/// Resolve a validated `local:workspace-icons/…` ref to an on-disk path.
pub fn workspace_icon_path(data_dir: &Path, icon_ref: &str) -> Option<PathBuf> {
    icon_ref_to_path(data_dir, icon_ref)
}

pub(crate) fn space_ctx<'a>(ctx: &'a AdminBridgeCtx) -> SpaceBridgeCtx<'a> {
    SpaceBridgeCtx {
        services: &ctx.services,
        spaces_dir: &ctx.spaces_dir,
    }
}

pub async fn list_spaces(ctx: &AdminBridgeCtx) -> Result<Value> {
    as_json(space::list_spaces(&space_ctx(ctx)).await?)
}

pub async fn get_space(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let id = Uuid::parse_str(&id)?;
    as_json(space::get_space(&space_ctx(ctx), id).await?)
}

pub async fn read_space_config(ctx: &AdminBridgeCtx, space_id: String) -> Result<Value> {
    as_json(space::read_space_config(&space_ctx(ctx), &space_id).await?)
}

pub async fn get_gateway_status(ctx: &AdminBridgeCtx, space_id: Option<String>) -> Result<Value> {
    ctx.gateway_runtime.get_gateway_status(space_id).await
}

pub async fn probe_gateway_start(ctx: &AdminBridgeCtx, port: Option<u16>) -> Result<Value> {
    ctx.gateway_runtime.probe_gateway_start(port).await
}

pub async fn take_pending_port_conflict(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.take_pending_port_conflict().await
}

pub async fn get_gateway_port_settings(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.get_gateway_port_settings().await
}

pub async fn reset_gateway_port(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.reset_gateway_port().await
}

pub async fn list_connected_servers(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.list_connected_servers().await
}

pub async fn get_pool_stats(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.get_pool_stats().await
}

pub async fn get_server_statuses(ctx: &AdminBridgeCtx, space_id: String) -> Result<Value> {
    ctx.gateway_runtime.get_server_statuses(space_id).await
}

pub async fn list_installed_servers(
    ctx: &AdminBridgeCtx,
    space_id: Option<String>,
) -> Result<Value> {
    let servers = if let Some(space_id) = space_id {
        ctx.services.server().list_for_space(&space_id).await?
    } else {
        ctx.services.server().list().await?
    };
    as_json(servers)
}

pub async fn discover_servers(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.server_discovery.refresh_if_needed().await?;
    as_json(ctx.server_discovery.list().await)
}

pub async fn get_server_definition(ctx: &AdminBridgeCtx, server_id: String) -> Result<Value> {
    ctx.server_discovery.refresh_if_needed().await?;
    as_json(ctx.server_discovery.get(&server_id).await)
}

pub async fn get_registry_ui_config(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.server_discovery.refresh_if_needed().await?;
    as_json(ctx.server_discovery.ui_config().await)
}

pub async fn get_registry_home_config(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.server_discovery.refresh_if_needed().await?;
    as_json(ctx.server_discovery.home_config().await)
}

pub async fn is_registry_offline(ctx: &AdminBridgeCtx) -> Result<Value> {
    as_json(ctx.server_discovery.is_offline().await)
}

pub async fn list_clients(ctx: &AdminBridgeCtx) -> Result<Value> {
    let clients = ctx.services.client().list().await?;
    Ok(Value::Array(
        clients
            .into_iter()
            .map(to_client_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_machines(ctx: &AdminBridgeCtx) -> Result<Value> {
    let machines = ctx.machine_repository.list().await?;
    Ok(Value::Array(
        machines
            .into_iter()
            .map(to_machine_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn get_local_machine_id(ctx: &AdminBridgeCtx) -> Result<Value> {
    let settings = AppSettingsService::new(ctx.settings_repository.clone());
    Ok(settings
        .get_local_machine_id()
        .await
        .map(|id| json!(id.to_string()))
        .unwrap_or(Value::Null))
}

pub fn get_hostname() -> Result<Value> {
    let name = hostname::get()?.to_string_lossy().into_owned();
    Ok(json!(name))
}

pub async fn get_client(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let id = Uuid::parse_str(&id)?;
    let client = ctx.services.client().get(id).await?;
    Ok(client.map(to_client_response).unwrap_or(Value::Null))
}

pub async fn list_feature_sets(ctx: &AdminBridgeCtx) -> Result<Value> {
    let sets = ctx.services.permission().list_feature_sets().await?;
    Ok(Value::Array(
        sets.into_iter()
            .map(to_feature_set_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_feature_sets_by_space(ctx: &AdminBridgeCtx, space_id: String) -> Result<Value> {
    let sets = ctx
        .services
        .permission()
        .list_feature_sets_for_space(&space_id)
        .await?;
    Ok(Value::Array(
        sets.into_iter()
            .map(to_feature_set_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn get_feature_set(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let set = ctx.services.permission().get_feature_set(&id).await?;
    Ok(set.map(to_feature_set_response).unwrap_or(Value::Null))
}

pub async fn get_feature_set_with_members(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    get_feature_set(ctx, id).await
}

pub async fn list_workspace_bindings(ctx: &AdminBridgeCtx) -> Result<Value> {
    let bindings = ctx.workspace_binding_repository.list().await?;
    Ok(Value::Array(
        bindings
            .into_iter()
            .map(to_workspace_binding_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_workspace_bindings_for_space(
    ctx: &AdminBridgeCtx,
    space_id: String,
) -> Result<Value> {
    let space_id = Uuid::parse_str(&space_id)?;
    let bindings = ctx
        .workspace_binding_repository
        .list_for_space(&space_id)
        .await?;
    Ok(Value::Array(
        bindings
            .into_iter()
            .map(to_workspace_binding_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_reported_workspace_roots(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.list_reported_workspace_roots().await
}

pub async fn validate_workspace_root(path: String) -> Result<Value> {
    match validate_workspace_root_path(&path) {
        WorkspaceRootValidation::Empty => Err(anyhow!("")),
        WorkspaceRootValidation::Ok { normalized } => as_json(normalized),
        WorkspaceRootValidation::Invalid { reason } => Err(anyhow!(reason)),
    }
}

pub async fn get_workspace_effective_features(
    ctx: &AdminBridgeCtx,
    workspace_root: String,
) -> Result<Value> {
    let normalized = match validate_workspace_root_path(&workspace_root) {
        WorkspaceRootValidation::Empty => return Err(anyhow!("workspace_root cannot be empty")),
        WorkspaceRootValidation::Ok { normalized } => normalized,
        WorkspaceRootValidation::Invalid { reason } => return Err(anyhow!(reason)),
    };

    let default_space = ctx
        .space_service
        .get_default()
        .await?
        .ok_or_else(|| anyhow!("No default Space configured"))?;

    let binding = ctx
        .workspace_binding_repository
        .find_longest_prefix_match(&default_space.id, None, std::slice::from_ref(&normalized))
        .await?;

    let (source, binding_id, space_id, feature_set_ids) = match binding {
        Some(binding) => (
            "binding".to_string(),
            Some(binding.id.to_string()),
            binding.space_id,
            binding.feature_set_ids,
        ),
        None => {
            let sets = ctx
                .services
                .permission()
                .list_feature_sets_for_space(&default_space.id.to_string())
                .await?;
            let fallback = sets
                .into_iter()
                .find(|set| set.feature_set_type.as_str() == "starter")
                .ok_or_else(|| anyhow!("Default Space has no Starter FeatureSet"))?;
            (
                "unbound".to_string(),
                None,
                default_space.id,
                vec![fallback.id],
            )
        }
    };

    let space = ctx
        .space_service
        .get(&space_id)
        .await?
        .ok_or_else(|| anyhow!("Resolved Space no longer exists"))?;

    let mut resolved_sets: Vec<FeatureSet> = Vec::with_capacity(feature_set_ids.len());
    for id in &feature_set_ids {
        let set = ctx
            .services
            .permission()
            .get_feature_set(id)
            .await?
            .ok_or_else(|| anyhow!("Resolved FeatureSet {id} not found"))?;
        resolved_sets.push(set);
    }

    let mut lookup: HashMap<String, FeatureSet> = HashMap::new();
    for set in ctx
        .services
        .permission()
        .list_feature_sets_for_space(&space_id.to_string())
        .await?
    {
        lookup.insert(set.id.clone(), set);
    }
    for set in &resolved_sets {
        lookup.insert(set.id.clone(), set.clone());
    }

    let mut allowed = HashSet::<String>::new();
    let mut excluded = HashSet::<String>::new();
    let mut visited = HashSet::<String>::new();
    for set in &resolved_sets {
        collect_member_ids(set, &lookup, &mut allowed, &mut excluded, &mut visited);
    }
    excluded.retain(|id| !allowed.contains(id));

    let all_features = ctx
        .server_feature_repository
        .list_for_space(&space_id.to_string())
        .await?;
    let mut server_totals = HashMap::<String, Value>::new();
    for feature in &all_features {
        let entry = server_totals
            .entry(feature.server_id.clone())
            .or_insert_with(|| json!({ "tools": 0, "prompts": 0, "resources": 0 }));
        let key = match feature.feature_type {
            FeatureType::Tool => "tools",
            FeatureType::Prompt => "prompts",
            FeatureType::Resource => "resources",
        };
        let current = entry[key].as_u64().unwrap_or(0);
        entry[key] = json!(current + 1);
    }

    let filtered = all_features
        .into_iter()
        .filter(|feature| {
            let id = feature.id.to_string();
            allowed.contains(&id) && !excluded.contains(&id)
        })
        .collect::<Vec<_>>();

    let to_effective = |feature: mcpmux_core::ServerFeature| {
        json!({
            "id": feature.id.to_string(),
            "feature_name": feature.feature_name,
            "display_name": feature.display_name,
            "description": feature.description,
            "server_id": feature.server_id,
            "server_alias": feature.server_alias,
            "server_status": "unknown",
            "available": feature.is_available,
        })
    };

    let mut tools = vec![];
    let mut prompts = vec![];
    let mut resources = vec![];
    for feature in filtered {
        match feature.feature_type {
            FeatureType::Tool => tools.push(to_effective(feature)),
            FeatureType::Prompt => prompts.push(to_effective(feature)),
            FeatureType::Resource => resources.push(to_effective(feature)),
        }
    }

    Ok(json!({
        "workspace_root": normalized,
        "source": source,
        "binding_id": binding_id,
        "space_id": space_id.to_string(),
        "space_name": space.name,
        "feature_sets": resolved_sets
            .into_iter()
            .map(|set| json!({
                "id": set.id,
                "name": set.name,
                "feature_set_type": set.feature_set_type.as_str(),
            }))
            .collect::<Vec<_>>(),
        "tools": tools,
        "prompts": prompts,
        "resources": resources,
        "server_totals": server_totals,
    }))
}

pub async fn list_workspace_appearances(ctx: &AdminBridgeCtx) -> Result<Value> {
    let items = ctx.workspace_appearance_repository.list().await?;
    Ok(Value::Array(
        items
            .into_iter()
            .map(to_workspace_appearance_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn resolve_workspace_icon_path(ctx: &AdminBridgeCtx, icon_ref: String) -> Result<Value> {
    let Some(path) = icon_ref_to_path(&ctx.data_dir, &icon_ref) else {
        return Ok(Value::Null);
    };
    match tokio::fs::metadata(&path).await {
        Ok(_) => as_json(Some(path.to_string_lossy().to_string())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => as_json(None::<String>),
        Err(err) => Err(anyhow!("failed to resolve icon path: {err}")),
    }
}

pub async fn get_startup_settings(ctx: &AdminBridgeCtx) -> Result<Value> {
    let start_minimized = ctx
        .settings_repository
        .get("startup.start_minimized")
        .await
        .ok()
        .flatten()
        .map(|value| value == "true")
        .unwrap_or(true);
    let close_to_tray = ctx
        .settings_repository
        .get("ui.close_to_tray")
        .await
        .ok()
        .flatten()
        .map(|value| value == "true")
        .unwrap_or(true);
    Ok(json!({
        "autoLaunch": ctx.auto_launch_enabled.unwrap_or(false),
        "startMinimized": start_minimized,
        "closeToTray": close_to_tray,
    }))
}

pub async fn get_server_update_settings(ctx: &AdminBridgeCtx) -> Result<Value> {
    let policy = match ctx
        .settings_repository
        .get("servers.default_update_policy")
        .await
    {
        Ok(Some(value)) => value,
        _ => "notify".to_string(),
    };
    let last_checked_at = ctx
        .settings_repository
        .get("servers.last_version_probe_at")
        .await
        .ok()
        .flatten();
    Ok(json!({
        "defaultUpdatePolicy": policy,
        "lastCheckedAt": last_checked_at,
    }))
}

pub async fn get_meta_tools_enabled(ctx: &AdminBridgeCtx) -> Result<Value> {
    let enabled = match ctx
        .settings_repository
        .get("gateway.meta_tools_enabled")
        .await
    {
        Ok(Some(value)) => !matches!(value.as_str(), "false" | "0"),
        _ => true,
    };
    as_json(enabled)
}

pub async fn get_version(ctx: &AdminBridgeCtx) -> Result<Value> {
    as_json(ctx.app_version.clone())
}

pub async fn get_bundle_version(ctx: &AdminBridgeCtx) -> Result<Value> {
    as_json(ctx.bundle_version.clone())
}

pub async fn get_build_info(ctx: &AdminBridgeCtx) -> Result<Value> {
    as_json(serde_json::json!({
        "git_sha": ctx.backend_build.git_sha,
        "git_branch": ctx.backend_build.git_branch,
        "commit_time": ctx.backend_build.commit_time,
        "build_time": ctx.backend_build.build_time,
    }))
}

pub async fn get_logs_path(ctx: &AdminBridgeCtx) -> Result<Value> {
    as_json(ctx.data_dir.join("logs").to_string_lossy().to_string())
}

pub async fn get_server_logs(
    ctx: &AdminBridgeCtx,
    server_id: String,
    limit: Option<usize>,
    level_filter: Option<String>,
) -> Result<Value> {
    let default_space = ctx
        .space_service
        .get_default()
        .await?
        .ok_or_else(|| anyhow!("No default space found"))?;
    let level = level_filter.and_then(|value| LogLevel::parse(&value));
    let logs = ctx
        .server_log_manager
        .read_logs(
            &default_space.id.to_string(),
            &server_id,
            limit.unwrap_or(100),
            level,
        )
        .await?;
    let mapped = logs
        .into_iter()
        .map(|log| {
            json!({
                "timestamp": log.timestamp.to_rfc3339(),
                "level": log.level.as_str(),
                "source": log.source.as_str(),
                "message": log.message,
                "metadata": log.metadata,
            })
        })
        .collect::<Vec<_>>();
    Ok(Value::Array(mapped))
}

pub async fn get_server_log_file(ctx: &AdminBridgeCtx, server_id: String) -> Result<Value> {
    let default_space = ctx
        .space_service
        .get_default()
        .await?
        .ok_or_else(|| anyhow!("No default space found"))?;
    let path = ctx
        .server_log_manager
        .get_log_file(&default_space.id.to_string(), &server_id);
    as_json(path.to_string_lossy().to_string())
}

pub async fn get_log_retention_days(ctx: &AdminBridgeCtx) -> Result<Value> {
    let settings = AppSettingsService::new(ctx.settings_repository.clone());
    as_json(settings.get_log_retention_days().await)
}

pub async fn get_oauth_clients(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.get_oauth_clients().await
}

pub async fn get_oauth_client_grants(
    ctx: &AdminBridgeCtx,
    client_id: String,
    space_id: String,
) -> Result<Value> {
    ctx.gateway_runtime
        .get_oauth_client_grants(client_id, space_id)
        .await
}

pub async fn list_meta_tool_grants(ctx: &AdminBridgeCtx) -> Result<Value> {
    ctx.gateway_runtime.list_meta_tool_grants().await
}

pub async fn list_server_features(
    ctx: &AdminBridgeCtx,
    space_id: String,
    include_unavailable: Option<bool>,
) -> Result<Value> {
    let features = ctx
        .server_feature_repository
        .list_for_space(&space_id)
        .await?;
    let features = if include_unavailable.unwrap_or(false) {
        features
    } else {
        features
            .into_iter()
            .filter(|feature| feature.is_available)
            .collect::<Vec<_>>()
    };
    Ok(Value::Array(
        features
            .into_iter()
            .map(to_server_feature_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_server_features_by_server(
    ctx: &AdminBridgeCtx,
    space_id: String,
    server_id: String,
    include_unavailable: Option<bool>,
) -> Result<Value> {
    let features = ctx
        .server_feature_repository
        .list_for_server(&space_id, &server_id)
        .await?;
    let features = if include_unavailable.unwrap_or(false) {
        features
    } else {
        features
            .into_iter()
            .filter(|feature| feature.is_available)
            .collect::<Vec<_>>()
    };
    Ok(Value::Array(
        features
            .into_iter()
            .map(to_server_feature_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_server_features_by_type(
    ctx: &AdminBridgeCtx,
    space_id: String,
    server_id: String,
    feature_type: String,
    include_unavailable: Option<bool>,
) -> Result<Value> {
    let parsed =
        FeatureType::parse(&feature_type).ok_or_else(|| anyhow!("Invalid feature type"))?;
    let features = ctx
        .server_feature_repository
        .list_for_server(&space_id, &server_id)
        .await?;
    let features = features
        .into_iter()
        .filter(|feature| feature.feature_type == parsed)
        .collect::<Vec<_>>();
    let features = if include_unavailable.unwrap_or(false) {
        features
    } else {
        features
            .into_iter()
            .filter(|feature| feature.is_available)
            .collect::<Vec<_>>()
    };
    Ok(Value::Array(
        features
            .into_iter()
            .map(to_server_feature_response)
            .collect::<Vec<_>>(),
    ))
}

pub async fn get_server_feature(ctx: &AdminBridgeCtx, id: String) -> Result<Value> {
    let id = Uuid::parse_str(&id)?;
    let feature = ctx.server_feature_repository.get(&id).await?;
    Ok(feature
        .map(to_server_feature_response)
        .unwrap_or(Value::Null))
}

pub async fn is_clone_id_available(
    _ctx: &AdminBridgeCtx,
    _space_id: String,
    _source_server_id: String,
    _suffix: String,
) -> Result<Value> {
    // ponytail: clone_server lands in Phase 6
    Err(anyhow!("Server cloning not yet available"))
}

pub async fn suggest_clone_suffix(
    _ctx: &AdminBridgeCtx,
    _space_id: String,
    _source_server_id: String,
) -> Result<Value> {
    // ponytail: clone_server lands in Phase 6
    Err(anyhow!("Server cloning not yet available"))
}

pub async fn list_clone_dependents(
    _ctx: &AdminBridgeCtx,
    _space_id: String,
    _source_server_id: String,
) -> Result<Value> {
    // ponytail: clone_server lands in Phase 6
    Err(anyhow!("Server cloning not yet available"))
}

pub async fn now_utc() -> Result<Value> {
    as_json(Utc::now().to_rfc3339())
}

const META_TOOLS_REQUIRE_APPROVAL_KEY: &str = "meta_tools.require_approval";
const UPDATE_CHANNEL_KEY: &str = "updates.channel";
const AUTO_INSTALL_UPDATES_KEY: &str = "updates.auto_install";

/// Registry categories are bundle-derived; ADR-001 keeps categories client-side in filters.
pub async fn list_registry_categories(_ctx: &AdminBridgeCtx) -> Result<Value> {
    Ok(Value::Array(vec![]))
}

pub async fn list_space_base_dirs(ctx: &AdminBridgeCtx, space_id: String) -> Result<Value> {
    let space_uuid = Uuid::parse_str(&space_id)?;
    let rows = ctx
        .space_base_dir_repository
        .list_by_space(&space_uuid)
        .await?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "space_id": row.space_id,
                    "path": row.path,
                    "created_at": row.created_at.to_rfc3339(),
                })
            })
            .collect(),
    ))
}

pub async fn list_builtin_servers(ctx: &AdminBridgeCtx, space_id: String) -> Result<Value> {
    let repo = &ctx.space_builtin_config_repository;
    let mut out = Vec::new();
    for descriptor in mcpmux_core::builtin_servers() {
        let enabled = repo
            .server_enabled_override(&space_id, descriptor.id)
            .await?
            .unwrap_or(descriptor.default_enabled);
        let disabled = repo.disabled_tools(&space_id, descriptor.id).await?;
        let tools = descriptor
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "write": tool.write,
                    "enabled": !disabled.iter().any(|name| name == tool.name),
                })
            })
            .collect::<Vec<_>>();
        out.push(json!({
            "id": descriptor.id,
            "name": descriptor.name,
            "description": descriptor.description,
            "enabled": enabled,
            "tools": tools,
        }));
    }
    Ok(Value::Array(out))
}

pub async fn get_meta_tools_require_approval(ctx: &AdminBridgeCtx) -> Result<Value> {
    let stored = ctx
        .settings_repository
        .get(META_TOOLS_REQUIRE_APPROVAL_KEY)
        .await?;
    let required = stored.map(|value| value != "false").unwrap_or(true);
    as_json(required)
}

pub async fn get_auto_install_updates(ctx: &AdminBridgeCtx) -> Result<Value> {
    let stored = ctx
        .settings_repository
        .get(AUTO_INSTALL_UPDATES_KEY)
        .await?;
    let enabled = stored.map(|value| value == "true").unwrap_or(true);
    as_json(enabled)
}

const WORKSPACE_MAPPING_PROMPT_KEY: &str = "workspaces.mapping_prompt_enabled";

fn mapping_prompt_enabled_from(stored: Option<&str>) -> bool {
    stored.map(|s| s != "false").unwrap_or(true)
}

pub async fn get_workspace_mapping_prompt_enabled(ctx: &AdminBridgeCtx) -> Result<Value> {
    let stored = ctx
        .settings_repository
        .get(WORKSPACE_MAPPING_PROMPT_KEY)
        .await?;
    as_json(mapping_prompt_enabled_from(stored.as_deref()))
}

pub async fn get_update_channel(ctx: &AdminBridgeCtx) -> Result<Value> {
    let stored = ctx.settings_repository.get(UPDATE_CHANNEL_KEY).await?;
    let channel = stored
        .filter(|value| value == "prerelease")
        .map(|_| "prerelease".to_string())
        .unwrap_or_else(|| "stable".to_string());
    as_json(channel)
}

fn config_export_format(client_type: &str) -> Result<ConfigFormat> {
    match client_type.to_lowercase().as_str() {
        "cursor" => Ok(ConfigFormat::Cursor),
        "vscode" | "vscode-continue" | "continue" => Ok(ConfigFormat::VsCodeContinue),
        "claude" | "claude-desktop" => Ok(ConfigFormat::ClaudeDesktop),
        _ => Err(anyhow!("Unknown client type: {client_type}")),
    }
}

async fn resolve_config_export_space_id(ctx: &AdminBridgeCtx, space_id: &str) -> Result<String> {
    if space_id == "default" || space_id.is_empty() {
        let space = ctx
            .space_service
            .get_default()
            .await?
            .ok_or_else(|| anyhow!("No default space found"))?;
        Ok(space.id.to_string())
    } else {
        Ok(space_id.to_string())
    }
}

fn resolve_config_placeholders(template: &str, input_values: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in input_values {
        let placeholder = format!("${{input:{key}}}");
        result = result.replace(&placeholder, value);
    }
    result
}

async fn build_config_export_servers(
    ctx: &AdminBridgeCtx,
    space_id: &str,
    mask_credentials: bool,
) -> Result<Vec<ResolvedServer>> {
    let installed = ctx.services.server().list_for_space(space_id).await?;
    let mut resolved = Vec::new();

    for inst in installed.into_iter().filter(|server| server.enabled) {
        let Some(entry) = inst.get_definition() else {
            continue;
        };

        let transport = match &entry.transport {
            TransportConfig::Stdio {
                command, args, env, ..
            } => {
                let resolved_command = resolve_config_placeholders(command, &inst.input_values);
                let mut resolved_args: Vec<String> = args
                    .iter()
                    .map(|arg| resolve_config_placeholders(arg, &inst.input_values))
                    .collect();
                resolved_args.extend(inst.args_append.clone());

                let mut resolved_env = HashMap::new();
                for (k, v) in env {
                    resolved_env.insert(
                        k.clone(),
                        resolve_config_placeholders(v, &inst.input_values),
                    );
                }

                if mask_credentials {
                    for k in inst.input_values.keys() {
                        resolved_env.insert(k.clone(), "***MASKED***".to_string());
                    }
                } else {
                    resolved_env.extend(inst.input_values.clone());
                }
                resolved_env.extend(inst.env_overrides.clone());

                ResolvedTransport::Stdio {
                    command: resolved_command,
                    args: resolved_args,
                    env: resolved_env,
                }
            }
            TransportConfig::Http { url, headers, .. } => {
                let resolved_url = resolve_config_placeholders(url, &inst.input_values);
                let mut resolved_headers: HashMap<String, String> = headers
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            resolve_config_placeholders(v, &inst.input_values),
                        )
                    })
                    .collect();
                resolved_headers.extend(inst.extra_headers.clone());

                ResolvedTransport::Http {
                    url: resolved_url,
                    headers: resolved_headers,
                }
            }
        };

        resolved.push(ResolvedServer {
            server_id: inst.server_id.clone(),
            transport,
        });
    }

    Ok(resolved)
}

fn suggested_config_filename(format: ConfigFormat) -> &'static str {
    match format {
        ConfigFormat::Cursor => "mcp.json",
        ConfigFormat::VsCodeContinue => "continue-mcp.json",
        ConfigFormat::ClaudeDesktop => "claude_desktop_config.json",
    }
}

/// Preview generated MCP client config JSON without writing to disk.
pub async fn preview_config_export(
    ctx: &AdminBridgeCtx,
    client_type: String,
    space_id: String,
    mask_credentials: bool,
) -> Result<Value> {
    let space_id = resolve_config_export_space_id(ctx, &space_id).await?;
    let format = config_export_format(&client_type)?;
    let servers = build_config_export_servers(ctx, &space_id, mask_credentials).await?;
    let exporter = ConfigExporter::new();
    let content = exporter
        .export_json(format, &servers)
        .map_err(|err| anyhow!("Failed to export config: {err}"))?;

    Ok(json!({
        "content": content,
        "default_path": format
            .default_path()
            .map(|path| path.to_string_lossy().to_string()),
        "suggested_filename": suggested_config_filename(format),
    }))
}

/// Default config file paths per supported MCP client type.
pub async fn get_config_paths() -> Result<Value> {
    let mut paths = HashMap::new();
    paths.insert(
        "cursor".to_string(),
        ConfigFormat::Cursor
            .default_path()
            .map(|path| path.to_string_lossy().to_string()),
    );
    paths.insert(
        "vscode".to_string(),
        ConfigFormat::VsCodeContinue
            .default_path()
            .map(|path| path.to_string_lossy().to_string()),
    );
    paths.insert(
        "claude".to_string(),
        ConfigFormat::ClaudeDesktop
            .default_path()
            .map(|path| path.to_string_lossy().to_string()),
    );
    as_json(paths)
}

/// Whether a config file already exists at the default path for a client type.
pub async fn check_config_exists(client_type: String) -> Result<Value> {
    let format = config_export_format(&client_type)?;
    let exists = format
        .default_path()
        .map(|path| path.exists())
        .unwrap_or(false);
    as_json(exists)
}

/// Copy an existing default config to a `.json.bak` sibling before overwrite.
pub async fn backup_existing_config(client_type: String) -> Result<Value> {
    let format = config_export_format(&client_type)?;
    let backup_path = match format.default_path() {
        Some(path) if path.exists() => {
            let backup_path = path.with_extension("json.bak");
            std::fs::copy(&path, &backup_path)
                .map_err(|err| anyhow!("Failed to backup config: {err}"))?;
            Some(backup_path.to_string_lossy().to_string())
        }
        _ => None,
    };
    as_json(backup_path)
}

/// Write generated MCP client config JSON to the given absolute path.
pub async fn export_config_to_file(
    ctx: &AdminBridgeCtx,
    client_type: String,
    space_id: String,
    path: String,
) -> Result<Value> {
    let space_id = resolve_config_export_space_id(ctx, &space_id).await?;
    let format = config_export_format(&client_type)?;
    let servers = build_config_export_servers(ctx, &space_id, false).await?;
    let exporter = ConfigExporter::new();
    let content = exporter
        .export_json(format, &servers)
        .map_err(|err| anyhow!("Failed to export config: {err}"))?;

    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| anyhow!("Failed to create parent dir: {err}"))?;
    }
    std::fs::write(&path, &content).map_err(|err| anyhow!("Failed to write config: {err}"))?;
    as_json(path.to_string_lossy().to_string())
}
