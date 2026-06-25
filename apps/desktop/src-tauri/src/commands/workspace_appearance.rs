//! Tauri commands for workspace appearance metadata and local icon files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::GenericImageView;
use mcpmux_core::{validate_workspace_root as validate_root, DomainEvent, WorkspaceAppearance};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use super::gateway::GatewayAppState;
use crate::state::AppState;

const LOCAL_ICON_PREFIX: &str = "local:workspace-icons/";
const WORKSPACE_ICON_DIR: &str = "workspace-icons";
const MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ICON_DIMENSION: u32 = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceAppearanceDto {
    pub workspace_root: String,
    pub icon: String,
    pub updated_at: String,
}

impl From<WorkspaceAppearance> for WorkspaceAppearanceDto {
    fn from(value: WorkspaceAppearance) -> Self {
        Self {
            workspace_root: value.workspace_root,
            icon: value.icon,
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceAppearanceInput {
    pub workspace_root: String,
    pub icon: String,
}

fn normalize_and_validate(raw: &str) -> Result<String, String> {
    match validate_root(raw) {
        mcpmux_core::WorkspaceRootValidation::Empty => Err("workspace_root cannot be empty".into()),
        mcpmux_core::WorkspaceRootValidation::Ok { normalized } => Ok(normalized),
        mcpmux_core::WorkspaceRootValidation::Invalid { reason } => Err(reason),
    }
}

fn normalize_icon(icon: &str) -> Option<String> {
    let trimmed = icon.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

pub(crate) fn local_ref_to_file_name(icon: &str) -> Option<&str> {
    let file_name = icon.strip_prefix(LOCAL_ICON_PREFIX)?;
    if file_name.contains('/') || file_name.contains('\\') {
        return None;
    }
    if Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        != Some("png")
    {
        return None;
    }
    Some(file_name)
}

fn icon_ref_to_path(data_dir: &Path, icon_ref: &str) -> Option<PathBuf> {
    let file_name = local_ref_to_file_name(icon_ref)?;
    Some(data_dir.join(WORKSPACE_ICON_DIR).join(file_name))
}

pub(crate) async fn maybe_remove_orphaned_icon_file(
    state: &AppState,
    icon_ref: Option<&str>,
) -> Result<(), String> {
    let Some(icon_ref) = icon_ref else {
        return Ok(());
    };
    let Some(file_name) = local_ref_to_file_name(icon_ref) else {
        return Ok(());
    };

    let icon_ref_owned = icon_ref.to_string();
    let appearances = state
        .workspace_appearance_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;

    if appearances.iter().any(|a| a.icon == icon_ref_owned) {
        return Ok(());
    }

    let bindings = state
        .workspace_binding_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;
    if bindings.iter().any(|b| b.icon.as_deref() == Some(icon_ref)) {
        return Ok(());
    }

    let file_path = state.data_dir().join(WORKSPACE_ICON_DIR).join(file_name);
    match tokio::fs::remove_file(&file_path).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("failed to remove orphaned icon file: {err}")),
    }
    Ok(())
}

pub(crate) async fn emit_workspace_appearance_changed(
    gateway_state: &Arc<RwLock<GatewayAppState>>,
    workspace_root: String,
) {
    let guard = gateway_state.read().await;
    let Some(ref gateway_state) = guard.gateway_state else {
        debug!("[workspace_appearance] gateway not running — skipping emit");
        return;
    };
    gateway_state
        .read()
        .await
        .emit_domain_event(DomainEvent::WorkspaceAppearanceChanged { workspace_root });
}

#[tauri::command]
pub async fn list_workspace_appearances(
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceAppearanceDto>, String> {
    state
        .workspace_appearance_repository
        .list()
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_workspace_appearance(
    input: WorkspaceAppearanceInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<WorkspaceAppearanceDto, String> {
    let workspace_root = normalize_and_validate(&input.workspace_root)?;
    let icon = normalize_icon(&input.icon).ok_or_else(|| "icon cannot be empty".to_string())?;
    let previous_icon = state
        .workspace_appearance_repository
        .get(&workspace_root)
        .await
        .map_err(|e| e.to_string())?
        .map(|a| a.icon);

    let appearance = WorkspaceAppearance::new(workspace_root.clone(), icon);
    state
        .workspace_appearance_repository
        .upsert(&appearance)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(previous_icon) = previous_icon {
        if previous_icon != appearance.icon {
            maybe_remove_orphaned_icon_file(&state, Some(previous_icon.as_str())).await?;
        }
    }

    emit_workspace_appearance_changed(gateway_state.inner(), workspace_root).await;
    Ok(appearance.into())
}

#[tauri::command]
pub async fn delete_workspace_appearance(
    workspace_root: String,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let normalized = normalize_and_validate(&workspace_root)?;
    let previous = state
        .workspace_appearance_repository
        .get(&normalized)
        .await
        .map_err(|e| e.to_string())?;

    state
        .workspace_appearance_repository
        .delete(&normalized)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(previous) = previous {
        maybe_remove_orphaned_icon_file(&state, Some(previous.icon.as_str())).await?;
    }

    emit_workspace_appearance_changed(gateway_state.inner(), normalized).await;
    Ok(())
}

#[tauri::command]
pub async fn upload_workspace_icon(
    source_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let source = PathBuf::from(source_path);
    let metadata = tokio::fs::metadata(&source)
        .await
        .map_err(|e| format!("failed to inspect source file: {e}"))?;
    if metadata.len() > MAX_UPLOAD_BYTES {
        return Err("icon file must be 2MB or smaller".to_string());
    }

    let bytes = tokio::fs::read(&source)
        .await
        .map_err(|e| format!("failed to read icon file: {e}"))?;
    let image =
        image::load_from_memory(&bytes).map_err(|e| format!("failed to decode image file: {e}"))?;

    let (width, height) = image.dimensions();
    let normalized = if width > MAX_ICON_DIMENSION || height > MAX_ICON_DIMENSION {
        image.resize(
            MAX_ICON_DIMENSION,
            MAX_ICON_DIMENSION,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image
    };

    let icon_dir = state.data_dir().join(WORKSPACE_ICON_DIR);
    tokio::fs::create_dir_all(&icon_dir)
        .await
        .map_err(|e| format!("failed to create workspace icon directory: {e}"))?;

    let file_name = format!("{}.png", Uuid::new_v4());
    let target_path = icon_dir.join(&file_name);
    normalized
        .save_with_format(&target_path, image::ImageFormat::Png)
        .map_err(|e| format!("failed to store icon file: {e}"))?;

    Ok(format!("{LOCAL_ICON_PREFIX}{file_name}"))
}

#[tauri::command]
pub async fn resolve_workspace_icon_path(
    icon_ref: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let Some(path) = icon_ref_to_path(state.data_dir(), &icon_ref) else {
        return Ok(None);
    };
    match tokio::fs::metadata(&path).await {
        Ok(_) => Ok(Some(path.to_string_lossy().to_string())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            warn!(path = %path.display(), "[workspace_appearance] icon file missing");
            Ok(None)
        }
        Err(err) => Err(format!("failed to resolve icon path: {err}")),
    }
}
