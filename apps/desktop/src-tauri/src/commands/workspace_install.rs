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

/// Make sure the directories leading to a client's config inside the
/// workspace are real directories (creating missing ones) and that the
/// config path itself is not a symlink. Returns the path to write.
///
/// The workspace may be an untrusted checkout: a symlinked `.cursor` /
/// `.codex` directory or `mcp.json` would otherwise make the installer edit
/// files elsewhere (e.g. the user's global `~/.cursor/mcp.json`). Symlinks
/// (and Windows junctions) anywhere below the workspace root are refused;
/// the root itself is whatever folder the user picked.
fn prepare_config_path(spec: &ClientSpec, workspace_dir: &Path) -> Result<PathBuf, String> {
    let root = workspace_dir
        .canonicalize()
        .map_err(|e| format!("cannot resolve workspace folder: {e}"))?;
    let (file_name, dirs) = spec
        .rel_path
        .split_last()
        .ok_or_else(|| "client has no config path".to_string())?;

    let mut dir = workspace_dir.to_path_buf();
    for seg in dirs {
        dir.push(seg);
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "{} is a symbolic link; refusing to write through it",
                    dir.display()
                ));
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(format!("{} is not a directory", dir.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&dir).map_err(|e| {
                    format!("failed to create config directory {}: {e}", dir.display())
                })?;
            }
            Err(e) => return Err(format!("cannot inspect {}: {e}", dir.display())),
        }
    }
    // Belt and braces: whatever the checks above saw, the directory we write
    // into must resolve to a place inside the workspace.
    let resolved = dir
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", dir.display()))?;
    if !resolved.starts_with(&root) {
        return Err(format!("{} resolves outside the workspace", dir.display()));
    }

    let path = dir.join(file_name);
    refuse_symlink(&path)?;
    Ok(path)
}

/// Error if `path` exists as a symlink or as anything but a regular file.
fn refuse_symlink(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "{} is a symbolic link; refusing to write through it",
            path.display()
        )),
        Ok(meta) if !meta.is_file() => Err(format!("{} is not a regular file", path.display())),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot inspect {}: {e}", path.display())),
    }
}

/// Replace `path` with `contents` via a fresh temp file in the same
/// directory and a rename, so the write never follows a link at `path` and
/// readers never see a half-written file. Keeps `permissions` when given;
/// a new file is private (0600 on Unix), since it can hold a bearer token.
fn write_replacing(
    path: &Path,
    contents: &[u8],
    permissions: Option<std::fs::Permissions>,
) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}-{nanos}.mcpmux-tmp", std::process::id()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        // Private from the start: the contents are written before the
        // original permissions could otherwise be applied.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
    let path = match prepare_config_path(spec, workspace_dir) {
        Ok(p) => p,
        Err(e) => return error_result(spec, &config_path(spec, workspace_dir), e),
    };
    let existed = path.is_file();

    let (existing, permissions) = if existed {
        match std::fs::read_to_string(&path) {
            Ok(s) => (
                Some(s),
                std::fs::metadata(&path).ok().map(|m| m.permissions()),
            ),
            Err(e) => {
                return error_result(spec, &path, format!("failed to read existing config: {e}"))
            }
        }
    } else {
        (None, None)
    };

    let merged = match render_config(existing.as_deref(), spec, mcp_url, header_value, bearer) {
        Ok(m) => m,
        Err(e) => return error_result(spec, &path, e),
    };

    // Back up an existing file before overwriting. The backup is written the
    // same link-safe way: a planted `mcp.json.mcpmux-bak` symlink is refused.
    let mut backed_up = None;
    if let Some(original) = existing.as_deref() {
        let bak = PathBuf::from(format!("{}.mcpmux-bak", path.display()));
        if let Err(e) = refuse_symlink(&bak) {
            return error_result(spec, &path, format!("cannot back up existing config: {e}"));
        }
        if let Err(e) = write_replacing(&bak, original.as_bytes(), permissions.clone()) {
            return error_result(
                spec,
                &path,
                format!("failed to back up existing config: {e}"),
            );
        }
        backed_up = Some(bak.to_string_lossy().to_string());
    }

    if let Err(e) = write_replacing(&path, merged.as_bytes(), permissions) {
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

    /// A fresh scratch dir with a "workspace" and an "outside" folder.
    #[cfg(unix)]
    fn scratch(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let tmp =
            std::env::temp_dir().join(format!("mcpmux-wsinstall-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let ws = tmp.join("ws");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (tmp, ws, outside)
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_config_directory_is_refused() {
        let (tmp, ws, outside) = scratch("dirlink");
        // A checkout whose `.cursor` points at the user's global config dir.
        std::fs::write(outside.join("mcp.json"), "{\"mcpServers\":{}}").unwrap();
        std::os::unix::fs::symlink(&outside, ws.join(".cursor")).unwrap();

        let r = install_one(spec("cursor"), &ws, "http://x/mcp", "/p", None);
        assert_eq!(r.action, "error");
        assert!(r.error.unwrap().contains("symbolic link"));
        assert_eq!(
            std::fs::read_to_string(outside.join("mcp.json")).unwrap(),
            "{\"mcpServers\":{}}",
            "the file behind the link is untouched"
        );
        assert!(!outside.join("mcp.json.mcpmux-bak").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_or_dangling_config_file_is_refused() {
        let (tmp, ws, outside) = scratch("filelink");
        std::fs::create_dir_all(ws.join(".vscode")).unwrap();
        // Dangling: `exists()` is false for it, which used to mean "create".
        let target = outside.join("created-by-installer.json");
        std::os::unix::fs::symlink(&target, ws.join(".vscode/mcp.json")).unwrap();

        let r = install_one(spec("vscode"), &ws, "http://x/mcp", "/p", None);
        assert_eq!(r.action, "error");
        assert!(
            !target.exists(),
            "nothing written through the dangling link"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn planted_backup_symlink_is_refused() {
        let (tmp, ws, outside) = scratch("baklink");
        let config = ws.join(".mcp.json");
        std::fs::write(&config, "{\"mcpServers\":{}}").unwrap();
        let victim = outside.join("victim");
        std::fs::write(&victim, "original").unwrap();
        std::os::unix::fs::symlink(&victim, ws.join(".mcp.json.mcpmux-bak")).unwrap();

        let r = install_one(spec("claude-code"), &ws, "http://x/mcp", "/p", None);
        assert_eq!(r.action, "error");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original");
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "{\"mcpServers\":{}}",
            "config left as it was"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn update_keeps_file_permissions_and_leaves_no_temp_files() {
        use std::os::unix::fs::PermissionsExt;
        let (tmp, ws, _outside) = scratch("perms");
        let config = ws.join(".mcp.json");
        std::fs::write(&config, "{\"mcpServers\":{}}").unwrap();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();

        let r = install_one(spec("claude-code"), &ws, "http://x/mcp", "/p", Some("tok"));
        assert_eq!(r.action, "updated", "{:?}", r.error);
        for file in [config.clone(), ws.join(".mcp.json.mcpmux-bak")] {
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", file.display());
        }
        let leftovers: Vec<_> = std::fs::read_dir(&ws)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".mcpmux-tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn a_new_config_is_private_and_an_existing_one_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (tmp, ws, _outside) = scratch("new-perms");
        let config = ws.join(".mcp.json");
        let r = install_one(spec("claude-code"), &ws, "http://x/mcp", "/p", Some("tok"));
        assert_eq!(r.action, "created", "{:?}", r.error);
        let mode = std::fs::metadata(&config).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "holds the bearer token");

        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
        let r = install_one(spec("claude-code"), &ws, "http://y/mcp", "/p", Some("tok"));
        assert_eq!(r.action, "updated", "{:?}", r.error);
        let mode = std::fs::metadata(&config).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "the user's choice is kept");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A directory junction (no admin rights needed to create one) pointing
    /// outside the workspace is refused like a symlink.
    #[cfg(windows)]
    #[test]
    fn junctioned_config_directory_is_refused() {
        let tmp =
            std::env::temp_dir().join(format!("mcpmux-wsinstall-junction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let ws = tmp.join("ws");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(ws.join(".cursor"))
            .arg(&outside)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "mklink /J failed");

        let r = install_one(spec("cursor"), &ws, "http://x/mcp", "/p", None);
        assert_eq!(r.action, "error", "{:?}", r.error);
        assert!(
            !outside.join("mcp.json").exists(),
            "nothing written through the junction"
        );
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
