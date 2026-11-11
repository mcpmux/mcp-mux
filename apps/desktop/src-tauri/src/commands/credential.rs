//! Credential management commands
//!
//! IPC commands for managing central credential profiles and secrets.
//! Enables re-using credentials across Spaces, single-point rotation,
//! and preservation of secrets across server uninstalls.

use std::collections::HashMap;
use tauri::State;
use uuid::Uuid;

use mcpmux_core::service::CredentialProfileService;
use mcpmux_core::{CredentialProfile, CredentialProfileSummary};

use crate::state::AppState;

fn make_service(state: &AppState) -> CredentialProfileService {
    CredentialProfileService::new(
        state.credential_profile_repository.clone(),
        state.installed_server_repository.clone(),
    )
}

/// List all credential profiles as summaries (with usage count and configured keys).
#[tauri::command]
pub async fn list_credential_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<CredentialProfileSummary>, String> {
    let service = make_service(&state);
    service
        .list_summaries()
        .await
        .map_err(|e| format!("Failed to list credential profiles: {e}"))
}

/// List credential profiles for a specific MCP server definition ID.
#[tauri::command]
pub async fn get_credential_profiles_for_server(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<CredentialProfileSummary>, String> {
    let service = make_service(&state);
    service
        .list_summaries_for_server(&server_id)
        .await
        .map_err(|e| format!("Failed to list credential profiles for server {server_id}: {e}"))
}

/// Get a full credential profile by ID (including decrypted values).
#[tauri::command]
pub async fn get_credential_profile(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<CredentialProfile, String> {
    let service = make_service(&state);
    service
        .get(&id)
        .await
        .map_err(|e| format!("Failed to get credential profile {id}: {e}"))?
        .ok_or_else(|| format!("Credential profile not found: {id}"))
}

/// Create a new credential profile.
#[tauri::command]
pub async fn create_credential_profile(
    state: State<'_, AppState>,
    name: String,
    server_id: String,
    server_name: Option<String>,
    values: HashMap<String, String>,
) -> Result<CredentialProfile, String> {
    let service = make_service(&state);
    service
        .create(name, server_id, server_name, values)
        .await
        .map_err(|e| format!("Failed to create credential profile: {e}"))
}

/// Update an existing credential profile and propagate secrets to linked server installations.
#[tauri::command]
pub async fn update_credential_profile(
    state: State<'_, AppState>,
    id: Uuid,
    name: String,
    values: HashMap<String, String>,
) -> Result<CredentialProfile, String> {
    let service = make_service(&state);
    service
        .update(&id, name, values)
        .await
        .map_err(|e| format!("Failed to update credential profile: {e}"))
}

/// Delete a credential profile by ID.
#[tauri::command]
pub async fn delete_credential_profile(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    let service = make_service(&state);
    service
        .delete(&id)
        .await
        .map_err(|e| format!("Failed to delete credential profile: {e}"))
}
