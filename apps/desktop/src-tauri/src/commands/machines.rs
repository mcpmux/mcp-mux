//! Machine catalog and local install identity commands.

use chrono::Utc;
use mcpmux_core::{AppSettingsService, Machine, MachineRepository};
use mcpmux_storage::{InboundClientRepository, SqliteMachineRepository};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::commands::gateway::{hot_reload_local_machine_id, GatewayAppState};
use crate::state::AppState;

/// Machine row exposed to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineDto {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub hostname: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Machine> for MachineDto {
    fn from(machine: Machine) -> Self {
        Self {
            id: machine.id.to_string(),
            name: machine.name,
            icon: machine.icon,
            hostname: machine.hostname,
            created_at: machine.created_at.to_rfc3339(),
            updated_at: machine.updated_at.to_rfc3339(),
        }
    }
}

/// Input for creating a machine.
#[derive(Debug, Deserialize)]
pub struct CreateMachineInput {
    pub name: String,
    pub icon: Option<String>,
    pub hostname: Option<String>,
}

/// Input for updating a machine.
#[derive(Debug, Deserialize)]
pub struct UpdateMachineInput {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub hostname: Option<String>,
}

fn machine_repo(state: &AppState) -> SqliteMachineRepository {
    SqliteMachineRepository::new(state.database())
}

fn settings_service(state: &AppState) -> AppSettingsService {
    AppSettingsService::new(state.settings_repository.clone())
}

fn inbound_client_repo(state: &AppState) -> InboundClientRepository {
    InboundClientRepository::new(state.database())
}

/// List all registered machines.
#[tauri::command]
pub async fn list_machines(state: State<'_, AppState>) -> Result<Vec<MachineDto>, String> {
    let machines = machine_repo(&state)
        .list()
        .await
        .map_err(|e| e.to_string())?;
    Ok(machines.into_iter().map(Into::into).collect())
}

/// Create a new machine.
#[tauri::command]
pub async fn create_machine(
    input: CreateMachineInput,
    state: State<'_, AppState>,
) -> Result<MachineDto, String> {
    let mut machine = Machine::new(input.name);
    machine.icon = input.icon;
    machine.hostname = input.hostname;

    machine_repo(&state)
        .create(&machine)
        .await
        .map_err(|e| e.to_string())?;

    Ok(machine.into())
}

/// Update machine display metadata.
#[tauri::command]
pub async fn update_machine(
    id: String,
    input: UpdateMachineInput,
    state: State<'_, AppState>,
) -> Result<MachineDto, String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let repo = machine_repo(&state);

    let mut machine = repo
        .get(&uuid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Machine not found: {id}"))?;

    if let Some(name) = input.name {
        machine.name = name;
    }
    if input.icon.is_some() {
        machine.icon = input.icon;
    }
    if input.hostname.is_some() {
        machine.hostname = input.hostname;
    }
    machine.updated_at = Utc::now();

    repo.update(&machine).await.map_err(|e| e.to_string())?;

    Ok(machine.into())
}

/// Delete a machine by id.
#[tauri::command]
pub async fn delete_machine(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    machine_repo(&state)
        .delete(&uuid)
        .await
        .map_err(|e| e.to_string())
}

/// Get the machine id this install is registered as.
#[tauri::command]
pub async fn get_local_machine_id(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(settings_service(&state)
        .get_local_machine_id()
        .await
        .map(|id| id.to_string()))
}

/// Input for setting this install's machine identity.
#[derive(Debug, Deserialize)]
pub struct SetLocalMachineIdInput {
    pub machine_id: Option<String>,
}

/// Set or clear the machine id for this install.
#[tauri::command]
pub async fn set_local_machine_id(
    input: SetLocalMachineIdInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let parsed = match input.machine_id {
        None => None,
        Some(value) => Some(Uuid::parse_str(&value).map_err(|e| e.to_string())?),
    };

    if let Some(id) = parsed {
        let exists = machine_repo(&state)
            .get(&id)
            .await
            .map_err(|e| e.to_string())?
            .is_some();
        if !exists {
            return Err(format!("Machine not found: {id}"));
        }
    }

    settings_service(&state)
        .set_local_machine_id(parsed)
        .await
        .map_err(|e| e.to_string())?;

    hot_reload_local_machine_id(gateway_state.inner(), parsed).await;
    Ok(())
}

/// Input for assigning a machine to an inbound OAuth client.
#[derive(Debug, Deserialize)]
pub struct SetClientMachineIdInput {
    pub machine_id: Option<String>,
}

/// Get the machine id assigned to an inbound OAuth client.
#[tauri::command]
pub async fn get_client_machine_id(
    client_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    Ok(inbound_client_repo(&state)
        .get_machine_id(&client_id)
        .await
        .map_err(|e| e.to_string())?
        .map(|id| id.to_string()))
}

/// Assign or clear the machine for an inbound OAuth client.
#[tauri::command]
pub async fn set_client_machine_id(
    client_id: String,
    input: SetClientMachineIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let parsed = match input.machine_id {
        None => None,
        Some(value) => Some(Uuid::parse_str(&value).map_err(|e| e.to_string())?),
    };

    if let Some(id) = parsed {
        let exists = machine_repo(&state)
            .get(&id)
            .await
            .map_err(|e| e.to_string())?
            .is_some();
        if !exists {
            return Err(format!("Machine not found: {id}"));
        }
    }

    inbound_client_repo(&state)
        .set_machine_id(&client_id, parsed)
        .await
        .map_err(|e| e.to_string())
}

/// Return the OS hostname as a hint for first-time machine registration.
#[tauri::command]
pub fn get_hostname() -> Result<String, String> {
    hostname::get()
        .map(|value| value.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}
