//! Client management commands
//!
//! IPC commands for managing AI clients (Cursor, VS Code, etc.).

use mcpmux_core::{Client, ConnectionMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::commands::gateway::GatewayAppState;
use crate::state::AppState;

/// Response for client listing
#[derive(Debug, Serialize)]
pub struct ClientResponse {
    pub id: String,
    pub name: String,
    pub client_type: String,
    pub connection_mode: String,
    pub locked_space_id: Option<String>,
    pub grants: HashMap<String, Vec<String>>,
    pub last_seen: Option<String>,
}

impl From<Client> for ClientResponse {
    fn from(c: Client) -> Self {
        let (mode, locked_id) = match &c.connection_mode {
            ConnectionMode::Locked { space_id } => {
                ("locked".to_string(), Some(space_id.to_string()))
            }
            ConnectionMode::FollowActive => ("follow_active".to_string(), None),
            ConnectionMode::AskOnChange { .. } => ("ask_on_change".to_string(), None),
        };

        let grants: HashMap<String, Vec<String>> = c
            .grants
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|u| u.to_string()).collect()))
            .collect();

        Self {
            id: c.id.to_string(),
            name: c.name,
            client_type: c.client_type,
            connection_mode: mode,
            locked_space_id: locked_id,
            grants,
            last_seen: c.last_seen.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Input for creating a client
#[derive(Debug, Deserialize)]
pub struct CreateClientInput {
    pub name: String,
    pub client_type: String,
    pub connection_mode: String,
    pub locked_space_id: Option<String>,
}

/// Input for updating client grants
#[derive(Debug, Deserialize)]
pub struct UpdateGrantsInput {
    pub space_id: String,
    pub feature_set_ids: Vec<String>,
}

/// List all clients.
#[tauri::command]
pub async fn list_clients(state: State<'_, AppState>) -> Result<Vec<ClientResponse>, String> {
    let clients = state
        .client_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;

    Ok(clients.into_iter().map(Into::into).collect())
}

/// Get a client by ID.
#[tauri::command]
pub async fn get_client(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<ClientResponse>, String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let client = state
        .client_repository
        .get(&uuid)
        .await
        .map_err(|e| e.to_string())?;

    Ok(client.map(Into::into))
}

/// Create a new client.
#[tauri::command]
pub async fn create_client(
    input: CreateClientInput,
    state: State<'_, AppState>,
) -> Result<ClientResponse, String> {
    let connection_mode = match input.connection_mode.as_str() {
        "locked" => {
            let space_id = input
                .locked_space_id
                .ok_or("locked_space_id required for locked mode")?;
            let uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;
            ConnectionMode::Locked { space_id: uuid }
        }
        "ask_on_change" => ConnectionMode::AskOnChange { triggers: vec![] },
        _ => ConnectionMode::FollowActive,
    };

    let mut client = Client::new(&input.name, &input.client_type);
    client.connection_mode = connection_mode;

    state
        .client_repository
        .create(&client)
        .await
        .map_err(|e| e.to_string())?;

    Ok(client.into())
}

/// Delete a client.
#[tauri::command]
pub async fn delete_client(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state
        .client_repository
        .delete(&uuid)
        .await
        .map_err(|e| e.to_string())
}

/// Update client grants for a specific space (using client_grants table).
#[tauri::command]
pub async fn update_client_grants(
    client_id: String,
    input: UpdateGrantsInput,
    state: State<'_, AppState>,
) -> Result<ClientResponse, String> {
    let client_uuid = Uuid::parse_str(&client_id).map_err(|e| e.to_string())?;

    // Verify client exists
    let client = state
        .client_repository
        .get(&client_uuid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Client not found")?;

    // Update grants using the client_grants table
    state
        .client_repository
        .set_grants_for_space(&client_uuid, &input.space_id, &input.feature_set_ids)
        .await
        .map_err(|e| e.to_string())?;

    Ok(client.into())
}

/// Get effective grants for a specific client and space.
/// This includes explicit grants PLUS the default feature set (merged as a set).
#[tauri::command]
pub async fn get_client_grants(
    client_id: String,
    space_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let client_uuid = Uuid::parse_str(&client_id).map_err(|e| e.to_string())?;

    // Get effective grants (explicit + default, deduplicated)
    state
        .client_service
        .get_effective_grants(&client_uuid, &space_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get all grants for a client across all spaces.
#[tauri::command]
pub async fn get_all_client_grants(
    client_id: String,
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<String>>, String> {
    let client_uuid = Uuid::parse_str(&client_id).map_err(|e| e.to_string())?;

    state
        .client_repository
        .get_all_grants(&client_uuid)
        .await
        .map_err(|e| e.to_string())
}

/// Grant a specific feature set to a client.
///
/// Emits MCP list_changed notifications to connected clients.
#[tauri::command]
pub async fn grant_feature_set_to_client(
    client_id: String,
    space_id: String,
    feature_set_id: String,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let client_uuid = Uuid::parse_str(&client_id).map_err(|e| e.to_string())?;
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    // Grant the feature set
    state
        .client_repository
        .grant_feature_set(&client_uuid, &space_id, &feature_set_id)
        .await
        .map_err(|e| e.to_string())?;

    // Emit notifications if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref emitter) = gw_state.event_emitter {
        emitter.emit_all_changed_for_space(space_uuid);
    }

    Ok(())
}

/// Revoke a specific feature set from a client.
///
/// Emits MCP list_changed notifications to connected clients.
#[tauri::command]
pub async fn revoke_feature_set_from_client(
    client_id: String,
    space_id: String,
    feature_set_id: String,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let client_uuid = Uuid::parse_str(&client_id).map_err(|e| e.to_string())?;
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;

    // Revoke the feature set
    state
        .client_repository
        .revoke_feature_set(&client_uuid, &space_id, &feature_set_id)
        .await
        .map_err(|e| e.to_string())?;

    // Emit notifications if gateway is running
    let gw_state = gateway_state.read().await;
    if let Some(ref emitter) = gw_state.event_emitter {
        emitter.emit_all_changed_for_space(space_uuid);
    }

    Ok(())
}

/// Update client connection mode.
///
/// Emits MCP list_changed notifications when the client's effective space changes.
#[tauri::command]
pub async fn update_client_mode(
    client_id: String,
    mode: String,
    locked_space_id: Option<String>,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<ClientResponse, String> {
    let client_uuid = Uuid::parse_str(&client_id).map_err(|e| e.to_string())?;

    let mut client = state
        .client_repository
        .get(&client_uuid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Client not found")?;

    client.connection_mode = match mode.as_str() {
        "locked" => {
            let space_id = locked_space_id.ok_or("locked_space_id required for locked mode")?;
            let uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;
            ConnectionMode::Locked { space_id: uuid }
        }
        "ask_on_change" => ConnectionMode::AskOnChange { triggers: vec![] },
        _ => ConnectionMode::FollowActive,
    };
    client.updated_at = chrono::Utc::now();

    state
        .client_repository
        .update(&client)
        .await
        .map_err(|e| e.to_string())?;

    // Emit notifications for the space this client is now using
    let gw_state = gateway_state.read().await;
    if let Some(emitter) = &gw_state.event_emitter {
        match &client.connection_mode {
            ConnectionMode::Locked { space_id } => {
                emitter.emit_all_changed_for_space(*space_id);
            }
            _ => {
                // For follow_active or ask_on_change, notifications will be sent
                // when the client reconnects and resolves its space
            }
        }
    }

    Ok(client.into())
}

/// Create preset clients (Cursor, VS Code, Claude Desktop).
#[tauri::command]
pub async fn init_preset_clients(state: State<'_, AppState>) -> Result<(), String> {
    let existing = state
        .client_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;

    // Create Cursor if not exists
    if !existing.iter().any(|c| c.client_type == "cursor") {
        let cursor = Client::cursor();
        state
            .client_repository
            .create(&cursor)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Create VS Code if not exists
    if !existing.iter().any(|c| c.client_type == "vscode") {
        let vscode = Client::vscode();
        state
            .client_repository
            .create(&vscode)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Create Claude Desktop if not exists
    if !existing.iter().any(|c| c.client_type == "claude") {
        let claude = Client::claude_desktop();
        state
            .client_repository
            .create(&claude)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
