//! Commands for managing client-specific custom feature sets

use crate::state::AppState;
use mcpmux_core::{FeatureSet, FeatureSetType};
use tauri::State;

/// Find or create a custom feature set for a specific client in a space
/// This ensures only one custom feature set exists per client per space
#[tauri::command]
pub async fn find_or_create_client_custom_feature_set(
    state: State<'_, AppState>,
    client_name: String,
    space_id: String,
) -> Result<FeatureSet, String> {
    let custom_set_name = format!("{} - Custom", client_name);

    // First, try to find existing custom feature set
    let existing_sets = state
        .feature_set_repository
        .list_by_space(&space_id)
        .await
        .map_err(|e| format!("Failed to list feature sets: {}", e))?;

    // Look for existing custom feature set with this name
    if let Some(existing) = existing_sets.iter().find(|fs| {
        fs.name == custom_set_name
            && fs.feature_set_type == FeatureSetType::Custom
            && !fs.is_deleted
    }) {
        // Load members
        return state
            .feature_set_repository
            .get_with_members(&existing.id)
            .await
            .map_err(|e| format!("Failed to load feature set: {}", e))?
            .ok_or_else(|| "Feature set not found".to_string());
    }

    // No existing set found, create a new one
    let new_set = FeatureSet::new_custom(&custom_set_name, &space_id)
        .with_description(format!("Custom features for {}", client_name))
        .with_icon("⚙️");

    state
        .feature_set_repository
        .create(&new_set)
        .await
        .map_err(|e| format!("Failed to create custom feature set: {}", e))?;

    Ok(new_set)
}
