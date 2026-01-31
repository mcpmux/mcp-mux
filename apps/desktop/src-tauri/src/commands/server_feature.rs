//! Server feature discovery commands
//!
//! IPC commands for querying discovered MCP features (tools, prompts, resources).

use mcpmux_storage::{FeatureType, ServerFeature, ServerFeatureRepository};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Response for server feature listing
#[derive(Debug, Serialize)]
pub struct ServerFeatureResponse {
    pub id: String,
    pub space_id: String,
    pub server_id: String,
    pub feature_type: String,
    pub feature_name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub discovered_at: String,
    pub last_seen_at: String,
    pub is_available: bool,
}

impl From<ServerFeature> for ServerFeatureResponse {
    fn from(f: ServerFeature) -> Self {
        Self {
            id: f.id,
            space_id: f.space_id,
            server_id: f.server_id,
            feature_type: f.feature_type.as_str().to_string(),
            feature_name: f.feature_name,
            display_name: f.display_name,
            description: f.description,
            input_schema: f.raw_json, // Use raw_json now
            discovered_at: f.discovered_at.to_rfc3339(),
            last_seen_at: f.last_seen_at.to_rfc3339(),
            is_available: f.is_available,
        }
    }
}

/// List all features for a space (only available features by default).
#[tauri::command]
pub async fn list_server_features(
    space_id: String,
    include_unavailable: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<ServerFeatureResponse>, String> {
    let features = state
        .server_feature_repository
        .list_by_space(&space_id)
        .await
        .map_err(|e| e.to_string())?;

    // Filter to only available features unless explicitly requested
    let filtered = if include_unavailable.unwrap_or(false) {
        features
    } else {
        features.into_iter().filter(|f| f.is_available).collect()
    };

    Ok(filtered.into_iter().map(Into::into).collect())
}

/// List features for a specific server in a space (only available by default).
#[tauri::command]
pub async fn list_server_features_by_server(
    space_id: String,
    server_id: String,
    include_unavailable: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<ServerFeatureResponse>, String> {
    let features = state
        .server_feature_repository
        .list_by_server(&space_id, &server_id)
        .await
        .map_err(|e| e.to_string())?;

    let filtered = if include_unavailable.unwrap_or(false) {
        features
    } else {
        features.into_iter().filter(|f| f.is_available).collect()
    };

    Ok(filtered.into_iter().map(Into::into).collect())
}

/// List features by type for a server (only available by default).
#[tauri::command]
pub async fn list_server_features_by_type(
    space_id: String,
    server_id: String,
    feature_type: String,
    include_unavailable: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<ServerFeatureResponse>, String> {
    let ft = FeatureType::from_str(&feature_type)
        .ok_or_else(|| format!("Invalid feature type: {}", feature_type))?;

    let features = state
        .server_feature_repository
        .list_by_type(&space_id, &server_id, ft)
        .await
        .map_err(|e| e.to_string())?;

    let filtered = if include_unavailable.unwrap_or(false) {
        features
    } else {
        features.into_iter().filter(|f| f.is_available).collect()
    };

    Ok(filtered.into_iter().map(Into::into).collect())
}

/// Get a specific feature by ID.
#[tauri::command]
pub async fn get_server_feature(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<ServerFeatureResponse>, String> {
    let feature = state
        .server_feature_repository
        .get(&id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(feature.map(Into::into))
}
