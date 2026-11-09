//! Credential management commands
//!
//! IPC commands for managing credentials and reusing inputs across spaces (Issue #227).

use std::collections::HashMap;
use tauri::State;

use mcpmux_core::domain::PriorCredentialSummary;
use mcpmux_core::service::{
    find_prior_credential as find_prior, get_prior_credential_values as get_prior,
};

use crate::state::AppState;

/// Find if the same server has been installed/configured with non-empty credentials in another Space.
///
/// Returns the most recently updated prior configuration (without the secret values).
#[tauri::command]
pub async fn find_prior_credential(
    state: State<'_, AppState>,
    space_id: String,
    server_id: String,
    input_id: Option<String>,
) -> Result<Option<PriorCredentialSummary>, String> {
    find_prior(
        &*state.installed_server_repository,
        Some(&*state.credential_repository),
        state.space_service.repository().as_ref(),
        &space_id,
        &server_id,
        input_id.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Fetch decrypted input/credential values from the source Space for the given server.
/// This is invoked ONLY when the user clicks "[ Use ]" on the reuse nudge.
#[tauri::command]
pub async fn get_prior_credential_values(
    state: State<'_, AppState>,
    source_space_id: String,
    server_id: String,
) -> Result<HashMap<String, String>, String> {
    get_prior(
        &*state.installed_server_repository,
        Some(&*state.credential_repository),
        &source_space_id,
        &server_id,
    )
    .await
    .map_err(|e| e.to_string())
}
