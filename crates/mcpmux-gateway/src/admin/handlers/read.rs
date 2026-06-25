//! Read-only admin REST handlers delegating to command bridge functions.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

use crate::admin::command_bridge::read as bridge;
use crate::admin::handlers::error::ApiError;
use crate::admin::router::AdminState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceQuery {
    pub space_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeQuery {
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateRootQuery {
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveFeaturesQuery {
    pub workspace_root: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconPathQuery {
    pub icon_ref: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerLogsQuery {
    pub limit: Option<usize>,
    pub level_filter: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerFeatureQuery {
    pub space_id: String,
    pub include_unavailable: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerFeatureByServerQuery {
    pub space_id: String,
    pub server_id: String,
    pub include_unavailable: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerFeatureByTypeQuery {
    pub space_id: String,
    pub server_id: String,
    pub feature_type: String,
    pub include_unavailable: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneAvailabilityQuery {
    pub space_id: String,
    pub source_server_id: String,
    pub suffix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneSuggestQuery {
    pub space_id: String,
    pub source_server_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneDependentsQuery {
    pub space_id: String,
    pub source_server_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigExportPreviewQuery {
    pub client_type: String,
    pub space_id: String,
    #[serde(default)]
    pub mask_credentials: bool,
}

fn ok(value: Value) -> Json<Value> {
    Json(value)
}

pub async fn get_gateway_status(
    State(state): State<AdminState>,
    Query(query): Query<SpaceQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_gateway_status(&state.bridge, query.space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn probe_gateway_start(
    State(state): State<AdminState>,
    Query(query): Query<ProbeQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::probe_gateway_start(&state.bridge, query.port)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn take_pending_port_conflict(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::take_pending_port_conflict(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_gateway_port_settings(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_gateway_port_settings(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn reset_gateway_port(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::reset_gateway_port(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_connected_servers(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_connected_servers(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_pool_stats(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_pool_stats(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_server_statuses(
    State(state): State<AdminState>,
    Query(query): Query<SpaceQuery>,
) -> Result<Json<Value>, ApiError> {
    let space_id = query
        .space_id
        .ok_or_else(|| ApiError::bad_request("spaceId query parameter is required"))?;
    match bridge::get_server_statuses(&state.bridge, space_id.clone()).await {
        Ok(value) => Ok(ok(value)),
        Err(error) => {
            warn!("[Admin] get_server_statuses failed for space {space_id}: {error:#}");
            Err(ApiError::from_bridge(error))
        }
    }
}

pub async fn list_spaces(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    match bridge::list_spaces(&state.bridge).await {
        Ok(value) => Ok(ok(value)),
        Err(error) => {
            warn!("[Admin] list_spaces failed: {error:#}");
            Err(ApiError::from_bridge(error))
        }
    }
}

pub async fn get_space(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_space(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn read_space_config(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::read_space_config(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_installed_servers(
    State(state): State<AdminState>,
    Query(query): Query<SpaceQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_installed_servers(&state.bridge, query.space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn discover_servers(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::discover_servers(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_server_definition(
    State(state): State<AdminState>,
    Path(server_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_server_definition(&state.bridge, server_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_registry_ui_config(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_registry_ui_config(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_registry_home_config(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_registry_home_config(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn is_registry_offline(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::is_registry_offline(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_clients(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::list_clients(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_machines(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::list_machines(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_local_machine_id(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_local_machine_id(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_hostname() -> Result<Json<Value>, ApiError> {
    bridge::get_hostname().map(ok).map_err(ApiError::from_bridge)
}

pub async fn get_client(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_client(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_feature_sets(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::list_feature_sets(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_feature_sets_by_space(
    State(state): State<AdminState>,
    Path(space_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_feature_sets_by_space(&state.bridge, space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_feature_set(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_feature_set(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_feature_set_with_members(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_feature_set_with_members(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_workspace_bindings(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_workspace_bindings(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_workspace_bindings_for_space(
    State(state): State<AdminState>,
    Path(space_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_workspace_bindings_for_space(&state.bridge, space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_reported_workspace_roots(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_reported_workspace_roots(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn validate_workspace_root(
    Query(query): Query<ValidateRootQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::validate_workspace_root(query.path)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_workspace_effective_features(
    State(state): State<AdminState>,
    Query(query): Query<EffectiveFeaturesQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_workspace_effective_features(&state.bridge, query.workspace_root)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_workspace_appearances(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_workspace_appearances(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn resolve_workspace_icon_path(
    State(state): State<AdminState>,
    Query(query): Query<IconPathQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::resolve_workspace_icon_path(&state.bridge, query.icon_ref)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

/// Stream a workspace icon PNG for web admin (`local:workspace-icons/…` refs).
pub async fn serve_workspace_icon(
    State(state): State<AdminState>,
    Query(query): Query<IconPathQuery>,
) -> Response {
    let Some(path) = bridge::workspace_icon_path(&state.bridge.data_dir, &query.icon_ref) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "private, max-age=3600"),
            ],
            bytes,
        )
            .into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(err) => {
            warn!(
                path = %path.display(),
                error = %err,
                "[Admin] failed to read workspace icon"
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn get_startup_settings(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_startup_settings(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_server_update_settings(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_server_update_settings(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_meta_tools_enabled(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_meta_tools_enabled(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_version(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_version(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_bundle_version(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_bundle_version(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_build_info(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_build_info(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_logs_path(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_logs_path(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_server_logs(
    State(state): State<AdminState>,
    Path(server_id): Path<String>,
    Query(query): Query<ServerLogsQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_server_logs(&state.bridge, server_id, query.limit, query.level_filter)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_server_log_file(
    State(state): State<AdminState>,
    Path(server_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_server_log_file(&state.bridge, server_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_log_retention_days(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_log_retention_days(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_oauth_clients(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_oauth_clients(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_oauth_client_grants(
    State(state): State<AdminState>,
    Path((client_id, space_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_oauth_client_grants(&state.bridge, client_id, space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_meta_tool_grants(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_meta_tool_grants(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_server_features(
    State(state): State<AdminState>,
    Query(query): Query<ServerFeatureQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_server_features(&state.bridge, query.space_id, query.include_unavailable)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_server_features_by_server(
    State(state): State<AdminState>,
    Query(query): Query<ServerFeatureByServerQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_server_features_by_server(
        &state.bridge,
        query.space_id,
        query.server_id,
        query.include_unavailable,
    )
    .await
    .map(ok)
    .map_err(ApiError::from_bridge)
}

pub async fn list_server_features_by_type(
    State(state): State<AdminState>,
    Query(query): Query<ServerFeatureByTypeQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_server_features_by_type(
        &state.bridge,
        query.space_id,
        query.server_id,
        query.feature_type,
        query.include_unavailable,
    )
    .await
    .map(ok)
    .map_err(ApiError::from_bridge)
}

pub async fn get_server_feature(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_server_feature(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn is_clone_id_available(
    State(state): State<AdminState>,
    Query(query): Query<CloneAvailabilityQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::is_clone_id_available(
        &state.bridge,
        query.space_id,
        query.source_server_id,
        query.suffix,
    )
    .await
    .map(ok)
    .map_err(ApiError::from_bridge)
}

pub async fn suggest_clone_suffix(
    State(state): State<AdminState>,
    Query(query): Query<CloneSuggestQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::suggest_clone_suffix(&state.bridge, query.space_id, query.source_server_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_clone_dependents(
    State(state): State<AdminState>,
    Query(query): Query<CloneDependentsQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_clone_dependents(&state.bridge, query.space_id, query.source_server_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_registry_categories(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_registry_categories(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_space_base_dirs(
    State(state): State<AdminState>,
    Path(space_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::list_space_base_dirs(&state.bridge, space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn list_builtin_servers(
    State(state): State<AdminState>,
    Query(query): Query<SpaceQuery>,
) -> Result<Json<Value>, ApiError> {
    let space_id = query
        .space_id
        .ok_or_else(|| ApiError::bad_request("spaceId is required"))?;
    bridge::list_builtin_servers(&state.bridge, space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_meta_tools_require_approval(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_meta_tools_require_approval(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_auto_install_updates(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_auto_install_updates(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_update_channel(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_update_channel(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn get_workspace_mapping_prompt_enabled(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::get_workspace_mapping_prompt_enabled(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn preview_config_export(
    State(state): State<AdminState>,
    Query(query): Query<ConfigExportPreviewQuery>,
) -> Result<Json<Value>, ApiError> {
    bridge::preview_config_export(
        &state.bridge,
        query.client_type,
        query.space_id,
        query.mask_credentials,
    )
    .await
    .map(ok)
    .map_err(ApiError::from_bridge)
}

pub async fn get_config_paths(State(_state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::get_config_paths()
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}
