//! Per-workspace MCP client config installer.
//!
//! Registers the McpMux gateway endpoint in a *project-local* MCP client config
//! (e.g. `.cursor/mcp.json`, `.vscode/mcp.json`) inside a chosen workspace
//! folder, injecting an `X-Mcpmux-Workspace` header whose value is that folder's
//! path. The gateway pins that header and routes the connection to the folder's
//! workspace binding deterministically — even for clients that don't report MCP
//! `roots` reliably (notably Cursor). This is the "less manual work" path: pick
//! a folder, pick clients, and McpMux writes (or extends) each client's config.
//!
//! Distinct from `config_export` (which exports the *upstream server list* to a
//! client): here we register the single gateway entry with a per-workspace
//! header.
//!
//! The client table and config rendering (JSON or TOML, merged into any
//! existing file) live in `mcpmux-core` (`service/workspace_snippet.rs`), shared with the
//! headless `mcpmux workspace config` command; this module does the file I/O.
//!
//! Only clients with a true **project-local** config scope are supported — a
//! global config can hold only one header value and so can't be per-workspace.
//! Windsurf/Cline (global-only) and Claude Desktop (stdio, no static headers)
//! are intentionally excluded.

use std::path::{Path, PathBuf};

use mcpmux_core::{find_client, render_config, ClientSpec, CLIENTS};
use serde::Serialize;
use tracing::info;

/// The config file path for a client inside a workspace folder.
fn config_path(spec: &ClientSpec, workspace_dir: &Path) -> PathBuf {
    let mut p = workspace_dir.to_path_buf();
    for seg in spec.rel_path {
        p.push(seg);
    }
    p
}

/// Result of installing into one client's config.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInstallResult {
    pub client: String,
    pub label: String,
    /// Absolute path of the config file written (or that failed).
    pub path: String,
    /// "created" | "updated" | "error".
    pub action: String,
    /// Path of the backup written when an existing file was modified.
    pub backed_up: Option<String>,
    /// Error message when `action == "error"`.
    pub error: Option<String>,
}

fn error_result(spec: &ClientSpec, path: &Path, msg: String) -> WorkspaceInstallResult {
    WorkspaceInstallResult {
        client: spec.id.to_string(),
        label: spec.label.to_string(),
        path: path.to_string_lossy().to_string(),
        action: "error".to_string(),
        backed_up: None,
        error: Some(msg),
    }
}

/// Write (or extend) one client's config. Backs up an existing file before
/// modifying it, and creates parent directories as needed.
fn install_one(
    spec: &ClientSpec,
    workspace_dir: &Path,
    mcp_url: &str,
    header_value: &str,
    bearer: Option<&str>,
) -> WorkspaceInstallResult {
    let path = config_path(spec, workspace_dir);
    let existed = path.exists();

    let existing = if existed {
        match std::fs::read_to_string(&path) {
            Ok(s) => Some(s),
            Err(e) => {
                return error_result(spec, &path, format!("failed to read existing config: {e}"))
            }
        }
    } else {
        None
    };

    let merged = match render_config(existing.as_deref(), spec, mcp_url, header_value, bearer) {
        Ok(m) => m,
        Err(e) => return error_result(spec, &path, e),
    };

    // Back up an existing file before overwriting.
    let mut backed_up = None;
    if existed {
        let bak = PathBuf::from(format!("{}.mcpmux-bak", path.display()));
        if let Err(e) = std::fs::copy(&path, &bak) {
            return error_result(
                spec,
                &path,
                format!("failed to back up existing config: {e}"),
            );
        }
        backed_up = Some(bak.to_string_lossy().to_string());
    }

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return error_result(
                spec,
                &path,
                format!("failed to create config directory: {e}"),
            );
        }
    }
    if let Err(e) = std::fs::write(&path, merged) {
        return error_result(spec, &path, format!("failed to write config: {e}"));
    }

    WorkspaceInstallResult {
        client: spec.id.to_string(),
        label: spec.label.to_string(),
        path: path.to_string_lossy().to_string(),
        action: if existed { "updated" } else { "created" }.to_string(),
        backed_up,
        error: None,
    }
}

/// One supported client, for the UI checklist.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInstallClient {
    pub id: String,
    pub label: String,
    /// The project-local config path, shown to the user (e.g. ".cursor/mcp.json").
    pub config_path: String,
}

/// List the clients the per-workspace installer supports.
#[tauri::command]
pub fn list_workspace_install_clients() -> Vec<WorkspaceInstallClient> {
    CLIENTS
        .iter()
        .map(|c| WorkspaceInstallClient {
            id: c.id.to_string(),
            label: c.label.to_string(),
            config_path: c.rel_path.join("/"),
        })
        .collect()
}

/// A copy-paste config snippet for one client.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceConfigSnippet {
    pub client: String,
    pub label: String,
    /// Where this would be written, relative to the workspace folder.
    pub config_path: String,
    /// Full file content (top-level key + the McpMux entry), ready to paste
    /// into a fresh file.
    pub content: String,
}

/// Generate a copy-paste config snippet for one client without writing anything.
#[tauri::command]
pub fn generate_workspace_config_snippet(
    client: String,
    server_url: String,
    workspace_root: String,
    bearer: Option<String>,
) -> Result<WorkspaceConfigSnippet, String> {
    let spec = find_client(&client).ok_or_else(|| format!("unknown client '{client}'"))?;
    // A full-file snippet (top-level key included) so it pastes cleanly into an
    // empty project config; merging into an existing file is what the install
    // command is for.
    let content = render_config(None, spec, &server_url, &workspace_root, bearer.as_deref())?;
    Ok(WorkspaceConfigSnippet {
        client: spec.id.to_string(),
        label: spec.label.to_string(),
        config_path: spec.rel_path.join("/"),
        content,
    })
}

/// Install (create or extend) the McpMux gateway entry into the chosen clients'
/// project-local configs inside `workspace_root`, injecting the
/// `X-Mcpmux-Workspace` header set to `workspace_root`.
///
/// `server_url` is the gateway MCP endpoint (e.g.
/// `http://localhost:45818/mcp`). `bearer` is an optional access token to embed
/// as `Authorization` when inbound auth is enabled; omit it when auth is
/// disabled.
#[tauri::command]
pub fn install_workspace_mcp_config(
    workspace_root: String,
    server_url: String,
    clients: Vec<String>,
    bearer: Option<String>,
) -> Result<Vec<WorkspaceInstallResult>, String> {
    let dir = PathBuf::from(&workspace_root);
    if !dir.is_dir() {
        return Err(format!("workspace folder does not exist: {workspace_root}"));
    }
    if server_url.trim().is_empty() {
        return Err("server URL is empty".to_string());
    }
    if clients.is_empty() {
        return Err("no clients selected".to_string());
    }

    let mut results = Vec::with_capacity(clients.len());
    for id in &clients {
        match find_client(id) {
            Some(spec) => {
                results.push(install_one(
                    spec,
                    &dir,
                    &server_url,
                    &workspace_root,
                    bearer.as_deref(),
                ));
            }
            None => {
                results.push(WorkspaceInstallResult {
                    client: id.clone(),
                    label: id.clone(),
                    path: String::new(),
                    action: "error".to_string(),
                    backed_up: None,
                    error: Some(format!("unknown client '{id}'")),
                });
            }
        }
    }

    let ok = results.iter().filter(|r| r.action != "error").count();
    info!(
        "[WorkspaceInstall] {} of {} client config(s) written for {}",
        ok,
        results.len(),
        workspace_root
    );
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str) -> &'static ClientSpec {
        find_client(id).unwrap()
    }

    #[test]
    fn install_creates_then_updates_with_backup() {
        let tmp = std::env::temp_dir().join(format!("mcpmux-wsinstall-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // First install → created, no backup.
        let r1 = install_one(
            spec("cursor"),
            &tmp,
            "http://x/mcp",
            &tmp.to_string_lossy(),
            None,
        );
        assert_eq!(r1.action, "created", "{:?}", r1.error);
        assert!(r1.backed_up.is_none());
        let written = std::fs::read_to_string(config_path(spec("cursor"), &tmp)).unwrap();
        assert!(written.contains("mcpmux"));
        assert!(written.contains(mcpmux_core::WORKSPACE_HEADER));

        // Second install → updated, with backup.
        let r2 = install_one(
            spec("cursor"),
            &tmp,
            "http://y/mcp",
            &tmp.to_string_lossy(),
            None,
        );
        assert_eq!(r2.action, "updated");
        assert!(r2.backed_up.is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_codex_extends_existing_toml() {
        let tmp =
            std::env::temp_dir().join(format!("mcpmux-wsinstall-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let path = config_path(spec("codex"), &tmp);
        assert!(path.ends_with(".codex/config.toml"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let existing = "# mine\nmodel = \"o3\"\n";
        std::fs::write(&path, existing).unwrap();

        let r = install_one(spec("codex"), &tmp, "http://x/mcp", "/p", None);
        assert_eq!(r.action, "updated", "{:?}", r.error);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with(existing), "{written}");
        assert!(written.contains("[mcp_servers.mcpmux]"));
        assert!(written.contains("http_headers"));
        let backup = std::fs::read_to_string(r.backed_up.unwrap()).unwrap();
        assert_eq!(backup, existing);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_refuses_to_clobber_unparseable_config() {
        let tmp = std::env::temp_dir().join(format!("mcpmux-wsinstall-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let path = config_path(spec("codex"), &tmp);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "model = ").unwrap();

        let r = install_one(spec("codex"), &tmp, "http://x/mcp", "/p", None);
        assert_eq!(r.action, "error");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "model = ");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn snippet_lists_all_clients() {
        let clients = list_workspace_install_clients();
        let ids: Vec<&str> = clients.iter().map(|c| c.id.as_str()).collect();
        for expected in [
            "cursor",
            "claude-code",
            "vscode",
            "opencode",
            "zed",
            "codex",
        ] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
        let codex = clients.iter().find(|c| c.id == "codex").unwrap();
        assert_eq!(codex.config_path, ".codex/config.toml");
    }
}
