//! Write admin REST handlers delegating to command bridge functions.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::admin::command_bridge::read as read_bridge;
use crate::admin::command_bridge::space::UpdateSpaceInput;
use crate::admin::command_bridge::write as bridge;
use crate::admin::command_bridge::write::{
    AddMemberBody, BuiltinServerEnabledBody, BuiltinToolEnabledBody, CloneServerBody,
    CreateClientBody, CreateFeatureSetBody, CreateMachineBody, CreateSpaceBody, DisconnectServerBody,
    GatewayPortBody, GatewayPublicUrlBody, GatewayStartBody, InstallServerBody, LogRetentionBody,
    MetaToolApprovalBody, MetaToolRevokeBody, MetaToolsEnabledBody, MetaToolsRequireApprovalBody,
    OAuthClientUpdateBody, OAuthGrantBody, SaveServerInputsBody, SaveSpaceConfigBody,
    ServerConnectionBody, ServerUpdateSettingsBody, SetLocalMachineIdBody, SetMembersBody,
    SetServerDisplayNameBody, SetServerEnabledBody, SetServerOAuthConnectedBody, SpaceBaseDirBody,
    StartupSettingsBody, UninstallServerBody, UpdateChannelBody, UpdateFeatureSetBody,
    UpdateMachineBody, UploadIconBody, WorkspaceAppearanceBody, WorkspaceBindingBody,
    WorkspaceMappingPromptBody,
};
use crate::admin::handlers::error::ApiError;
use crate::admin::router::AdminState;

fn ok(value: Value) -> Json<Value> {
    Json(value)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigExportClientTypeBody {
    client_type: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportConfigRequestBody {
    client_type: String,
    space_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConfigExportToFileBody {
    request: ExportConfigRequestBody,
    path: String,
}

pub async fn create_space(
    State(state): State<AdminState>,
    Json(body): Json<CreateSpaceBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::create_space(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_space(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSpaceInput>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_space(&state.bridge, id, input)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_space(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_space(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn save_space_config(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SaveSpaceConfigBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::save_space_config(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn remove_server_from_config(
    State(state): State<AdminState>,
    Path((space_id, server_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    bridge::remove_server_from_config(&state.bridge, space_id, server_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn start_gateway(
    State(state): State<AdminState>,
    Json(body): Json<GatewayStartBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::start_gateway(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn stop_gateway(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::stop_gateway(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn restart_gateway(
    State(state): State<AdminState>,
    Json(body): Json<GatewayStartBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::restart_gateway(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn disconnect_server(
    State(state): State<AdminState>,
    Json(body): Json<DisconnectServerBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::disconnect_server(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn connect_all_enabled_servers(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::connect_all_enabled_servers(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn refresh_oauth_tokens_on_startup(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::refresh_oauth_tokens_on_startup(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_gateway_port(
    State(state): State<AdminState>,
    Json(body): Json<GatewayPortBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_gateway_port(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_gateway_public_url(
    State(state): State<AdminState>,
    Json(body): Json<GatewayPublicUrlBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_gateway_public_url(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn install_server(
    State(state): State<AdminState>,
    Json(body): Json<InstallServerBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::install_server(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn uninstall_server(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<UninstallServerBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::uninstall_server(&state.bridge, id, body.space_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn save_server_inputs(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SaveServerInputsBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::save_server_inputs(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_server_display_name(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SetServerDisplayNameBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_server_display_name(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_server_oauth_connected(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SetServerOAuthConnectedBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_server_oauth_connected(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn enable_server_v2(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::enable_server_v2(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn disable_server_v2(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::disable_server_v2(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn start_auth_v2(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::start_auth_v2(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn cancel_auth_v2(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::cancel_auth_v2(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn retry_connection(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::retry_connection(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_server_package(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_server_package(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn logout_server(
    State(state): State<AdminState>,
    Json(body): Json<ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::logout_server(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn clone_server(
    State(state): State<AdminState>,
    Json(body): Json<CloneServerBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::clone_server(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn create_feature_set(
    State(state): State<AdminState>,
    Json(body): Json<CreateFeatureSetBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::create_feature_set(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_feature_set(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateFeatureSetBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_feature_set(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_feature_set(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_feature_set(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn add_feature_set_member(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<AddMemberBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::add_feature_set_member(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn remove_feature_set_member(
    State(state): State<AdminState>,
    Path((id, member_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    bridge::remove_feature_set_member(&state.bridge, id, member_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_feature_set_members(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SetMembersBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_feature_set_members(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn create_client(
    State(state): State<AdminState>,
    Json(body): Json<CreateClientBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::create_client(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_client(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_client(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn create_machine(
    State(state): State<AdminState>,
    Json(body): Json<CreateMachineBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::create_machine(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_machine(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateMachineBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_machine(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_machine(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_machine(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_local_machine_id(
    State(state): State<AdminState>,
    Json(body): Json<SetLocalMachineIdBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_local_machine_id(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn init_preset_clients(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::init_preset_clients(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn create_workspace_binding(
    State(state): State<AdminState>,
    Json(body): Json<WorkspaceBindingBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::create_workspace_binding(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_workspace_binding(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<WorkspaceBindingBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_workspace_binding(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_workspace_binding(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_workspace_binding(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn upsert_workspace_appearance(
    State(state): State<AdminState>,
    Json(body): Json<WorkspaceAppearanceBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::upsert_workspace_appearance(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_workspace_appearance(
    State(state): State<AdminState>,
    Json(body): Json<WorkspaceAppearanceBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_workspace_appearance(&state.bridge, body.workspace_root)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn upload_workspace_icon(
    State(state): State<AdminState>,
    Json(body): Json<UploadIconBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::upload_workspace_icon(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_startup_settings(
    State(state): State<AdminState>,
    Json(body): Json<StartupSettingsBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_startup_settings(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_server_update_settings(
    State(state): State<AdminState>,
    Json(body): Json<ServerUpdateSettingsBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_server_update_settings(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_meta_tools_enabled(
    State(state): State<AdminState>,
    Json(body): Json<MetaToolsEnabledBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_meta_tools_enabled(&state.bridge, body.enabled)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn clear_server_logs(
    State(state): State<AdminState>,
    Path(server_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::clear_server_logs(&state.bridge, server_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_log_retention_days(
    State(state): State<AdminState>,
    Json(body): Json<LogRetentionBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_log_retention_days(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn refresh_registry(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    bridge::refresh_registry(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn respond_to_meta_tool_approval(
    State(state): State<AdminState>,
    Json(body): Json<MetaToolApprovalBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::respond_to_meta_tool_approval(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn revoke_meta_tool_grant(
    State(state): State<AdminState>,
    Json(body): Json<MetaToolRevokeBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::revoke_meta_tool_grant(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn update_oauth_client(
    State(state): State<AdminState>,
    Path(client_id): Path<String>,
    Json(body): Json<OAuthClientUpdateBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::update_oauth_client(&state.bridge, client_id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn delete_oauth_client(
    State(state): State<AdminState>,
    Path(client_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::delete_oauth_client(&state.bridge, client_id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn grant_oauth_client_feature_set(
    State(state): State<AdminState>,
    Path(client_id): Path<String>,
    Json(body): Json<OAuthGrantBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::grant_oauth_client_feature_set(&state.bridge, client_id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn revoke_oauth_client_feature_set(
    State(state): State<AdminState>,
    Path(client_id): Path<String>,
    Json(body): Json<OAuthGrantBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::revoke_oauth_client_feature_set(&state.bridge, client_id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn check_server_version(
    State(state): State<AdminState>,
    Path(server_id): Path<String>,
    Json(body): Json<bridge::ServerConnectionBody>,
) -> Result<Json<Value>, ApiError> {
    let body = bridge::ServerConnectionBody {
        space_id: body.space_id,
        server_id,
    };
    bridge::check_server_version(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn check_all_server_versions(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::check_all_server_versions(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn add_space_base_dir(
    State(state): State<AdminState>,
    Path(space_id): Path<String>,
    Json(body): Json<SpaceBaseDirBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::add_space_base_dir(&state.bridge, space_id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn remove_space_base_dir(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    bridge::remove_space_base_dir(&state.bridge, id)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_server_enabled(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SetServerEnabledBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_server_enabled(&state.bridge, id, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_builtin_server_enabled(
    State(state): State<AdminState>,
    Json(body): Json<BuiltinServerEnabledBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_builtin_server_enabled(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_builtin_tool_enabled(
    State(state): State<AdminState>,
    Json(body): Json<BuiltinToolEnabledBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_builtin_tool_enabled(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn clear_unmapped_reported_roots(
    State(state): State<AdminState>,
) -> Result<Json<Value>, ApiError> {
    bridge::clear_unmapped_reported_roots(&state.bridge)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_meta_tools_require_approval(
    State(state): State<AdminState>,
    Json(body): Json<MetaToolsRequireApprovalBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_meta_tools_require_approval(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_workspace_mapping_prompt_enabled(
    State(state): State<AdminState>,
    Json(body): Json<WorkspaceMappingPromptBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_workspace_mapping_prompt_enabled(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn set_update_channel(
    State(state): State<AdminState>,
    Json(body): Json<UpdateChannelBody>,
) -> Result<Json<Value>, ApiError> {
    bridge::set_update_channel(&state.bridge, body)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn check_config_exists(
    State(_state): State<AdminState>,
    Json(body): Json<ConfigExportClientTypeBody>,
) -> Result<Json<Value>, ApiError> {
    read_bridge::check_config_exists(body.client_type)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn backup_existing_config(
    State(_state): State<AdminState>,
    Json(body): Json<ConfigExportClientTypeBody>,
) -> Result<Json<Value>, ApiError> {
    read_bridge::backup_existing_config(body.client_type)
        .await
        .map(ok)
        .map_err(ApiError::from_bridge)
}

pub async fn export_config_to_file(
    State(state): State<AdminState>,
    Json(body): Json<ConfigExportToFileBody>,
) -> Result<Json<Value>, ApiError> {
    read_bridge::export_config_to_file(
        &state.bridge,
        body.request.client_type,
        body.request.space_id,
        body.path,
    )
    .await
    .map(ok)
    .map_err(ApiError::from_bridge)
}
