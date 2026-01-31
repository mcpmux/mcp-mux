//! Tauri commands for managing individual feature members in feature sets
//!
//! Allows fine-grained control: add individual tools/prompts/resources to feature sets

use mcpmux_core::{FeatureSetMember, MemberMode};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;
use uuid::Uuid as StdUuid;

use crate::state::AppState;
use crate::commands::gateway::GatewayAppState;

/// Add an individual feature (tool/prompt/resource) to a feature set
#[tauri::command]
pub async fn add_feature_to_set(
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    feature_set_id: String,
    feature_id: String,
    mode: String,
) -> Result<(), String> {
    let app_state = &*state;
    
    let mode = match mode.as_str() {
        "include" => MemberMode::Include,
        "exclude" => MemberMode::Exclude,
        _ => return Err("Invalid mode. Use 'include' or 'exclude'".to_string()),
    };

    app_state
        .feature_set_repository
        .add_feature_member(&feature_set_id, &feature_id, mode)
        .await
        .map_err(|e| format!("Failed to add feature to set: {}", e))?;
    
    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        
        // Get feature set to access space_id
        if let Ok(Some(fs)) = app_state.feature_set_repository.get(&feature_set_id).await {
            if let Some(space_id_str) = fs.space_id {
                if let Ok(space_uuid) = StdUuid::parse_str(&space_id_str) {
                    gw.emit_domain_event(mcpmux_core::DomainEvent::FeatureSetMembersChanged {
                        space_id: space_uuid,
                        feature_set_id: feature_set_id.clone(),
                        added_count: 1,
                        removed_count: 0,
                    });
                }
            }
        }
    }

    Ok(())
}

/// Remove an individual feature from a feature set
#[tauri::command]
pub async fn remove_feature_from_set(
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
    feature_set_id: String,
    feature_id: String,
) -> Result<(), String> {
    let app_state = &*state;

    app_state
        .feature_set_repository
        .remove_feature_member(&feature_set_id, &feature_id)
        .await
        .map_err(|e| format!("Failed to remove feature from set: {}", e))?;
    
    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        
        // Get feature set to access space_id
        if let Ok(Some(fs)) = app_state.feature_set_repository.get(&feature_set_id).await {
            if let Some(space_id_str) = fs.space_id {
                if let Ok(space_uuid) = StdUuid::parse_str(&space_id_str) {
                    gw.emit_domain_event(mcpmux_core::DomainEvent::FeatureSetMembersChanged {
                        space_id: space_uuid,
                        feature_set_id: feature_set_id.clone(),
                        added_count: 0,
                        removed_count: 1,
                    });
                }
            }
        }
    }

    Ok(())
}

/// Get all individual feature members of a feature set
#[tauri::command]
pub async fn get_feature_set_members(
    state: State<'_, AppState>,
    feature_set_id: String,
) -> Result<Vec<FeatureSetMember>, String> {
    let app_state = &*state;

    let members = app_state
        .feature_set_repository
        .get_feature_members(&feature_set_id)
        .await
        .map_err(|e| format!("Failed to get feature members: {}", e))?;

    Ok(members)
}

