//! Space command bridge — shared logic for Tauri IPC and admin REST.

use std::path::Path;

use anyhow::{Context, Result};
use mcpmux_core::{get_space_config_path, ApplicationServices, Space, UserServerEntry};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

/// Default space configuration template written for new spaces.
pub const DEFAULT_SPACE_CONFIG: &str = r#"{
  "mcpServers": {
  }
}
"#;

/// Partial update payload for a space (name, icon, description).
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateSpaceInput {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
}

/// Dependencies required by space bridge functions beyond `ApplicationServices`.
pub struct SpaceBridgeCtx<'a> {
    pub services: &'a ApplicationServices,
    pub spaces_dir: &'a Path,
}

impl<'a> SpaceBridgeCtx<'a> {
    /// Resolve the on-disk JSON config path for a space.
    pub fn config_path(&self, space_id: &str) -> Result<std::path::PathBuf, uuid::Error> {
        get_space_config_path(self.spaces_dir, space_id)
    }
}

/// List all spaces.
pub async fn list_spaces(ctx: &SpaceBridgeCtx<'_>) -> Result<Vec<Space>> {
    ctx.services.space().list().await
}

/// Get a space by ID.
pub async fn get_space(ctx: &SpaceBridgeCtx<'_>, id: Uuid) -> Result<Option<Space>> {
    ctx.services.space().get(id).await
}

/// Create a space and ensure its default config file exists.
pub async fn create_space(
    ctx: &SpaceBridgeCtx<'_>,
    name: String,
    icon: Option<String>,
) -> Result<Space> {
    let space = ctx.services.space().create(&name, icon).await?;
    write_default_config_if_missing(ctx, &space.id.to_string())?;
    info!("[command_bridge::space] Space '{}' created", space.name);
    Ok(space)
}

/// Update a space's display metadata.
pub async fn update_space(
    ctx: &SpaceBridgeCtx<'_>,
    id: Uuid,
    input: UpdateSpaceInput,
) -> Result<Space> {
    let name = input
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    let icon = input
        .icon
        .map(|i| i.trim().to_string())
        .filter(|i| !i.is_empty());
    let description = input.description.map(|d| d.trim().to_string());

    let space = ctx
        .services
        .space()
        .update(id, name, icon, description)
        .await?;
    info!("[command_bridge::space] Space '{}' updated", space.name);
    Ok(space)
}

/// Delete a space by ID.
pub async fn delete_space(ctx: &SpaceBridgeCtx<'_>, id: Uuid) -> Result<()> {
    ctx.services.space().delete(id).await?;
    info!("[command_bridge::space] Space '{}' deleted", id);
    Ok(())
}

/// Read a space configuration file, creating the default template when missing.
pub async fn read_space_config(ctx: &SpaceBridgeCtx<'_>, space_id: &str) -> Result<String> {
    let config_path = ctx.config_path(space_id)?;
    write_default_config_if_missing(ctx, space_id)?;

    std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))
}

/// Save a space configuration file after JSON validation.
pub async fn save_space_config(
    ctx: &SpaceBridgeCtx<'_>,
    space_id: &str,
    content: &str,
) -> Result<()> {
    serde_json::from_str::<serde_json::Value>(content).context("Invalid JSON")?;

    let config_path = ctx.config_path(space_id)?;
    std::fs::write(&config_path, content)
        .with_context(|| format!("Failed to write config file: {}", config_path.display()))
}

/// Remove a server entry from a space config file.
pub async fn remove_server_from_config(
    ctx: &SpaceBridgeCtx<'_>,
    space_id: &str,
    server_id: &str,
) -> Result<bool> {
    let config_path = ctx.config_path(space_id)?;
    if !config_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

    let mut config: serde_json::Value =
        serde_json::from_str(&content).context("Failed to parse config")?;

    let servers = config.get_mut("mcpServers").and_then(|v| v.as_object_mut());
    if let Some(servers) = servers {
        if servers.remove(server_id).is_some() {
            let new_content =
                serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
            std::fs::write(&config_path, new_content).with_context(|| {
                format!("Failed to write config file: {}", config_path.display())
            })?;
            info!(
                "[command_bridge::space] Removed server '{}' from space '{}'",
                server_id, space_id
            );
            return Ok(true);
        }
    }

    Ok(false)
}

/// Replace a custom server's entry in a space config file.
///
/// Matches the target `mcpServers` key by comparing its normalized form
/// (see `UserServerEntry::normalize_server_id`) against `server_id`, since
/// the installed server id is normalized but the raw JSON key may not be.
pub async fn update_server_in_config(
    ctx: &SpaceBridgeCtx<'_>,
    space_id: &str,
    server_id: &str,
    entry: serde_json::Value,
) -> Result<()> {
    if !entry.is_object() {
        anyhow::bail!("Server entry must be a JSON object");
    }

    let config_path = ctx.config_path(space_id)?;
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

    let mut config: serde_json::Value =
        serde_json::from_str(&content).context("Failed to parse config")?;

    let servers = config
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
        .context("Config file has no mcpServers object")?;

    let matching_key = servers
        .keys()
        .find(|key| UserServerEntry::normalize_server_id(key) == server_id)
        .cloned()
        .with_context(|| format!("Server '{}' not found in config", server_id))?;

    servers.insert(matching_key, entry);

    let new_content =
        serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    std::fs::write(&config_path, new_content)
        .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;

    info!(
        "[command_bridge::space] Updated server '{}' in space '{}'",
        server_id, space_id
    );
    Ok(())
}

fn write_default_config_if_missing(ctx: &SpaceBridgeCtx<'_>, space_id: &str) -> Result<()> {
    std::fs::create_dir_all(ctx.spaces_dir)
        .with_context(|| format!("Failed to create spaces dir: {}", ctx.spaces_dir.display()))?;

    let config_path = ctx.config_path(space_id)?;
    if config_path.exists() {
        return Ok(());
    }

    std::fs::write(&config_path, DEFAULT_SPACE_CONFIG).with_context(|| {
        format!(
            "Failed to create default config file: {}",
            config_path.display()
        )
    })?;
    info!(
        "[command_bridge::space] Created default config file: {}",
        config_path.display()
    );
    Ok(())
}
