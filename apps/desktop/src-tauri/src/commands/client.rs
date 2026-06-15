//! Client management commands
//!
//! Identity-only surface: list, get, create, delete, and preset seeding.
//! Connection modes and per-client FeatureSet grants no longer exist —
//! routing is entirely driven by WorkspaceBinding + Space default FS.

use mcpmux_core::Client;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

/// Response for client listing
#[derive(Debug, Serialize)]
pub struct ClientResponse {
    pub id: String,
    pub name: String,
    pub client_type: String,
    pub last_seen: Option<String>,
}

impl From<Client> for ClientResponse {
    fn from(c: Client) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name,
            client_type: c.client_type,
            last_seen: c.last_seen.map(|dt| dt.to_rfc3339()),
        }
    }
}

/// Input for creating a client
#[derive(Debug, Deserialize)]
pub struct CreateClientInput {
    pub name: String,
    pub client_type: String,
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
    let client = Client::new(&input.name, &input.client_type);

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

/// Create preset clients (Cursor, VS Code, Claude Desktop).
#[tauri::command]
pub async fn init_preset_clients(state: State<'_, AppState>) -> Result<(), String> {
    let existing = state
        .client_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;

    if !existing.iter().any(|c| c.client_type == "cursor") {
        let cursor = Client::cursor();
        state
            .client_repository
            .create(&cursor)
            .await
            .map_err(|e| e.to_string())?;
    }

    if !existing.iter().any(|c| c.client_type == "vscode") {
        let vscode = Client::vscode();
        state
            .client_repository
            .create(&vscode)
            .await
            .map_err(|e| e.to_string())?;
    }

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
