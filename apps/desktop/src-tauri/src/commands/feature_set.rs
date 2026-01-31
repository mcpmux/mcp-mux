//! FeatureSet management commands
//!
//! IPC commands for managing feature sets (permission bundles).

use mcpmux_core::{FeatureSet, FeatureSetMember, MemberMode, MemberType};
use serde::{Deserialize, Serialize};
use tauri::State;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid as StdUuid;

use crate::state::AppState;
use crate::commands::gateway::GatewayAppState;

/// Response for feature set member
#[derive(Debug, Serialize)]
pub struct FeatureSetMemberResponse {
    pub id: String,
    pub feature_set_id: String,
    pub member_type: String,
    pub member_id: String,
    pub mode: String,
}

impl From<&FeatureSetMember> for FeatureSetMemberResponse {
    fn from(m: &FeatureSetMember) -> Self {
        Self {
            id: m.id.clone(),
            feature_set_id: m.feature_set_id.clone(),
            member_type: m.member_type.as_str().to_string(),
            member_id: m.member_id.clone(),
            mode: m.mode.as_str().to_string(),
        }
    }
}

/// Response for feature set listing
#[derive(Debug, Serialize)]
pub struct FeatureSetResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub space_id: Option<String>,
    pub feature_set_type: String,
    pub server_id: Option<String>,
    pub is_builtin: bool,
    pub is_deleted: bool,
    pub members: Vec<FeatureSetMemberResponse>,
}

impl From<FeatureSet> for FeatureSetResponse {
    fn from(fs: FeatureSet) -> Self {
        let members = fs.members.iter().map(Into::into).collect();
        Self {
            id: fs.id,
            name: fs.name,
            description: fs.description,
            icon: fs.icon,
            space_id: fs.space_id,
            feature_set_type: fs.feature_set_type.as_str().to_string(),
            server_id: fs.server_id,
            is_builtin: fs.is_builtin,
            is_deleted: fs.is_deleted,
            members,
        }
    }
}

/// Input for creating a feature set
#[derive(Debug, Deserialize)]
pub struct CreateFeatureSetInput {
    pub name: String,
    pub space_id: String,
    pub description: Option<String>,
    pub icon: Option<String>,
}

/// Input for updating a feature set
#[derive(Debug, Deserialize)]
pub struct UpdateFeatureSetInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
}

/// Input for adding a member to a feature set
#[derive(Debug, Deserialize)]
pub struct AddMemberInput {
    pub member_type: String,  // "feature" or "feature_set"
    pub member_id: String,
    pub mode: Option<String>,  // "include" or "exclude", defaults to "include"
}

/// List all feature sets.
#[tauri::command]
pub async fn list_feature_sets(
    state: State<'_, AppState>,
) -> Result<Vec<FeatureSetResponse>, String> {
    let feature_sets = state
        .feature_set_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;

    Ok(feature_sets.into_iter().map(Into::into).collect())
}

/// List feature sets for a specific space.
/// Filters out server-all feature sets for servers that are not enabled.
#[tauri::command]
pub async fn list_feature_sets_by_space(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<FeatureSetResponse>, String> {
    let feature_sets = state
        .feature_set_repository
        .list_by_space(&space_id)
        .await
        .map_err(|e| e.to_string())?;

    // Get enabled server IDs to filter server-all feature sets
    let installed_servers = state
        .installed_server_repository
        .list_for_space(&space_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;
    
    let enabled_server_ids: std::collections::HashSet<String> = installed_servers
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.server_id)
        .collect();

    // Filter out server-all feature sets for servers that are not enabled
    let filtered = feature_sets
        .into_iter()
        .filter(|fs| {
            if fs.feature_set_type == mcpmux_core::FeatureSetType::ServerAll {
                // Only include if server is enabled
                fs.server_id.as_ref().is_some_and(|sid| enabled_server_ids.contains(sid))
            } else {
                true
            }
        })
        .map(Into::into)
        .collect();

    Ok(filtered)
}

/// Get a feature set by ID (without members).
#[tauri::command]
pub async fn get_feature_set(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<FeatureSetResponse>, String> {
    let feature_set = state
        .feature_set_repository
        .get(&id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(feature_set.map(Into::into))
}

/// Get a feature set by ID with its members.
#[tauri::command]
pub async fn get_feature_set_with_members(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<FeatureSetResponse>, String> {
    let feature_set = state
        .feature_set_repository
        .get_with_members(&id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(feature_set.map(Into::into))
}

/// Create a new custom feature set.
#[tauri::command]
pub async fn create_feature_set(
    input: CreateFeatureSetInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<FeatureSetResponse, String> {
    let mut feature_set = FeatureSet::new_custom(&input.name, &input.space_id);
    
    if let Some(desc) = input.description {
        feature_set = feature_set.with_description(desc);
    }
    
    if let Some(icon) = input.icon {
        feature_set = feature_set.with_icon(icon);
    }

    state
        .feature_set_repository
        .create(&feature_set)
        .await
        .map_err(|e| e.to_string())?;
    
    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        
        // Parse space_id as Uuid
        if let Ok(space_uuid) = StdUuid::parse_str(&input.space_id) {
            gw.emit_domain_event(mcpmux_core::DomainEvent::FeatureSetCreated {
                space_id: space_uuid,
                feature_set_id: feature_set.id.clone(),
                name: feature_set.name.clone(),
                feature_set_type: Some(feature_set.feature_set_type.as_str().to_string()),
            });
        }
    }

    Ok(feature_set.into())
}

/// Delete a feature set.
#[tauri::command]
pub async fn delete_feature_set(
    id: String,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    // Get feature set before deleting to access space_id
    let feature_set = state
        .feature_set_repository
        .get(&id)
        .await
        .map_err(|e| e.to_string())?;
    
    state
        .feature_set_repository
        .delete(&id)
        .await
        .map_err(|e| e.to_string())?;
    
    // Emit domain event if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref gw) = gw_state.gateway_state {
        let gw = gw.read().await;
        
        // Only emit if we found the feature set and it has a space_id
        if let Some(fs) = feature_set {
            if let Some(space_id_str) = fs.space_id {
                if let Ok(space_uuid) = StdUuid::parse_str(&space_id_str) {
                    gw.emit_domain_event(mcpmux_core::DomainEvent::FeatureSetDeleted {
                        space_id: space_uuid,
                        feature_set_id: id,
                    });
                }
            }
        }
    }
    
    Ok(())
}

/// Get builtin feature sets for a space.
#[tauri::command]
pub async fn get_builtin_feature_sets(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<FeatureSetResponse>, String> {
    let feature_sets = state
        .feature_set_repository
        .list_builtin(&space_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(feature_sets.into_iter().map(Into::into).collect())
}

/// Ensure server-all featureset exists for a server in a space.
#[tauri::command]
pub async fn ensure_server_all_feature_set(
    space_id: String,
    server_id: String,
    server_name: String,
    state: State<'_, AppState>,
) -> Result<FeatureSetResponse, String> {
    let feature_set = state
        .feature_set_repository
        .ensure_server_all(&space_id, &server_id, &server_name)
        .await
        .map_err(|e| e.to_string())?;

    Ok(feature_set.into())
}

/// Update a feature set (name, description, icon).
#[tauri::command]
pub async fn update_feature_set(
    id: String,
    input: UpdateFeatureSetInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<FeatureSetResponse, String> {
    let mut feature_set = state
        .feature_set_repository
        .get_with_members(&id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Feature set not found")?;

    if feature_set.is_builtin {
        return Err("Cannot modify builtin feature set".to_string());
    }

    if let Some(name) = input.name {
        feature_set.name = name;
    }
    if let Some(desc) = input.description {
        feature_set.description = Some(desc);
    }
    if let Some(icon) = input.icon {
        feature_set.icon = Some(icon);
    }
    feature_set.updated_at = Utc::now();

    state
        .feature_set_repository
        .update(&feature_set)
        .await
        .map_err(|e| e.to_string())?;

    // Notify MCP clients that feature set changed (if gateway running)
    let space_id = feature_set.space_id.as_deref().unwrap_or("default");
    let gw_state = gateway_state.read().await;
    if let Some(ref grant_service) = gw_state.grant_service {
        if let Err(e) = grant_service.notify_feature_set_modified(space_id, &id).await {
            warn!("[FeatureSet] Failed to emit notifications: {}", e);
        }
    }

    Ok(feature_set.into())
}

/// Add a member (feature or featureset) to a feature set.
#[tauri::command]
pub async fn add_feature_set_member(
    feature_set_id: String,
    input: AddMemberInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<FeatureSetResponse, String> {
    let mut feature_set = state
        .feature_set_repository
        .get_with_members(&feature_set_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Feature set not found")?;

    // Only "default" and "custom" types can have their members modified
    let fs_type = feature_set.feature_set_type.as_str();
    if fs_type != "default" && fs_type != "custom" {
        return Err(format!(
            "Cannot modify members of '{}' type feature set",
            fs_type
        ));
    }

    let member_type = match input.member_type.as_str() {
        "feature_set" => MemberType::FeatureSet,
        _ => MemberType::Feature,
    };

    let mode = input
        .mode
        .as_deref()
        .and_then(MemberMode::from_str)
        .unwrap_or(MemberMode::Include);

    // Check for duplicates
    if feature_set.members.iter().any(|m| {
        m.member_type == member_type && m.member_id == input.member_id
    }) {
        return Err("Member already exists in this feature set".to_string());
    }

    // Check for recursive reference (featureset including itself)
    if member_type == MemberType::FeatureSet && input.member_id == feature_set_id {
        return Err("Cannot add a feature set to itself".to_string());
    }

    // Prevent including "all" or "default" type feature sets in other feature sets
    if member_type == MemberType::FeatureSet {
        if let Ok(Some(target_fs)) = state.feature_set_repository.get(&input.member_id).await {
            let target_type = target_fs.feature_set_type.as_str();
            if target_type == "all" || target_type == "default" {
                return Err(format!(
                    "Cannot include '{}' type feature sets in other feature sets. Only 'custom' and 'server-all' types can be included.",
                    target_type
                ));
            }
        }
    }

    let member = FeatureSetMember {
        id: uuid::Uuid::new_v4().to_string(),
        feature_set_id: feature_set_id.clone(),
        member_type,
        member_id: input.member_id,
        mode,
    };

    feature_set.members.push(member);
    feature_set.updated_at = Utc::now();

    state
        .feature_set_repository
        .update(&feature_set)
        .await
        .map_err(|e| e.to_string())?;

    // Notify MCP clients that feature set changed (if gateway running)
    let space_id = feature_set.space_id.as_deref().unwrap_or("default");
    let gw_state = gateway_state.read().await;
    if let Some(ref grant_service) = gw_state.grant_service {
        if let Err(e) = grant_service.notify_feature_set_modified(space_id, &feature_set_id).await {
            warn!("[FeatureSet] Failed to emit notifications: {}", e);
        }
    }

    Ok(feature_set.into())
}

/// Remove a member from a feature set.
#[tauri::command]
pub async fn remove_feature_set_member(
    feature_set_id: String,
    member_id: String,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<FeatureSetResponse, String> {
    let mut feature_set = state
        .feature_set_repository
        .get_with_members(&feature_set_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Feature set not found")?;

    if feature_set.is_builtin {
        return Err("Cannot modify builtin feature set".to_string());
    }

    feature_set.members.retain(|m| m.id != member_id);
    feature_set.updated_at = Utc::now();

    state
        .feature_set_repository
        .update(&feature_set)
        .await
        .map_err(|e| e.to_string())?;

    // Notify MCP clients that feature set changed (if gateway running)
    let space_id = feature_set.space_id.as_deref().unwrap_or("default");
    let gw_state = gateway_state.read().await;
    if let Some(ref grant_service) = gw_state.grant_service {
        if let Err(e) = grant_service.notify_feature_set_modified(space_id, &feature_set_id).await {
            warn!("[FeatureSet] Failed to emit notifications: {}", e);
        }
    }

    Ok(feature_set.into())
}

/// Set all members for a feature set (replaces existing).
/// Note: Only "default" and "custom" types can have members modified.
/// "all" and "server-all" types are auto-computed and cannot be modified.
#[tauri::command]
pub async fn set_feature_set_members(
    feature_set_id: String,
    members: Vec<AddMemberInput>,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<FeatureSetResponse, String> {
    let mut feature_set = state
        .feature_set_repository
        .get_with_members(&feature_set_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Feature set not found")?;

    // Only "default" and "custom" types can have their members modified
    // "all" grants everything automatically, "server-all" is also auto-computed
    let fs_type = feature_set.feature_set_type.as_str();
    if fs_type != "default" && fs_type != "custom" {
        return Err(format!(
            "Cannot modify members of '{}' type feature set. Only 'default' and 'custom' types are configurable.",
            fs_type
        ));
    }

    // Convert inputs to members, filtering out invalid entries
    let new_members: Vec<FeatureSetMember> = members
        .into_iter()
        .filter(|m| {
            // Skip self-references
            if m.member_type == "feature_set" && m.member_id == feature_set_id {
                return false;
            }
            true
        })
        .map(|input| {
            let member_type = match input.member_type.as_str() {
                "feature_set" => MemberType::FeatureSet,
                _ => MemberType::Feature,
            };
            let mode = input
                .mode
                .as_deref()
                .and_then(MemberMode::from_str)
                .unwrap_or(MemberMode::Include);

            FeatureSetMember {
                id: uuid::Uuid::new_v4().to_string(),
                feature_set_id: feature_set_id.clone(),
                member_type,
                member_id: input.member_id,
                mode,
            }
        })
        .collect();

    feature_set.members = new_members;
    feature_set.updated_at = Utc::now();

    state
        .feature_set_repository
        .update(&feature_set)
        .await
        .map_err(|e| e.to_string())?;

    // Notify MCP clients that feature set changed (if gateway running)
    let space_id = feature_set.space_id.as_deref().unwrap_or("default");
    let gw_state = gateway_state.read().await;
    if let Some(ref grant_service) = gw_state.grant_service {
        if let Err(e) = grant_service.notify_feature_set_modified(space_id, &feature_set_id).await {
            warn!("[FeatureSet] Failed to emit notifications: {}", e);
        }
    }

    Ok(feature_set.into())
}
