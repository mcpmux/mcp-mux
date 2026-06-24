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
//! Only clients with a true **project-local** config scope are supported — a
//! global config can hold only one header value and so can't be per-workspace.
//! Windsurf/Cline (global-only) and Claude Desktop (stdio, no static headers)
//! are intentionally excluded.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::json;
use tracing::info;

/// The server name McpMux registers itself under in every client config.
const SERVER_NAME: &str = "mcpmux";

/// The per-workspace routing header. Its value is the workspace folder path.
const WORKSPACE_HEADER: &str = "X-Mcpmux-Workspace";

/// Static description of one client's project-local MCP config shape. The
/// research-backed differences between clients live here and nowhere else, so
/// the writer and the UI share a single source of truth.
#[derive(Debug, Clone, Copy)]
struct ClientSpec {
    /// Stable id used by the API and UI (e.g. "cursor").
    id: &'static str,
    /// Human label for the UI.
    label: &'static str,
    /// Path of the config file relative to the workspace folder, as segments
    /// (e.g. `[".cursor", "mcp.json"]`).
    rel_path: &'static [&'static str],
    /// Top-level object the server entry nests under. Differs across clients:
    /// `mcpServers` (Cursor/Claude Code), `servers` (VS Code), `mcp`
    /// (opencode), `context_servers` (Zed).
    servers_key: &'static str,
    /// The key the endpoint URL goes under (always `url` for the project-local
    /// clients we support; Windsurf's `serverUrl` is global-only and excluded).
    url_key: &'static str,
    /// The transport `type` value, when the client requires one. `http` for
    /// Claude Code / VS Code, `remote` for opencode; Cursor and Zed infer it
    /// from the presence of `url`, so they get `None`.
    type_value: Option<&'static str>,
}

/// The supported project-local clients. Adding a client is a one-line table
/// entry plus a test.
const CLIENTS: &[ClientSpec] = &[
    ClientSpec {
        id: "cursor",
        label: "Cursor",
        rel_path: &[".cursor", "mcp.json"],
        servers_key: "mcpServers",
        url_key: "url",
        type_value: None,
    },
    ClientSpec {
        id: "claude-code",
        label: "Claude Code",
        rel_path: &[".mcp.json"],
        servers_key: "mcpServers",
        url_key: "url",
        type_value: Some("http"),
    },
    ClientSpec {
        id: "vscode",
        label: "VS Code / Copilot",
        rel_path: &[".vscode", "mcp.json"],
        servers_key: "servers",
        url_key: "url",
        type_value: Some("http"),
    },
    ClientSpec {
        id: "opencode",
        label: "opencode",
        rel_path: &["opencode.json"],
        servers_key: "mcp",
        url_key: "url",
        type_value: Some("remote"),
    },
    ClientSpec {
        id: "zed",
        label: "Zed",
        rel_path: &[".zed", "settings.json"],
        servers_key: "context_servers",
        url_key: "url",
        type_value: None,
    },
];

fn find_client(id: &str) -> Option<&'static ClientSpec> {
    CLIENTS.iter().find(|c| c.id == id)
}

/// The config file path for a client inside a workspace folder.
fn config_path(spec: &ClientSpec, workspace_dir: &Path) -> PathBuf {
    let mut p = workspace_dir.to_path_buf();
    for seg in spec.rel_path {
        p.push(seg);
    }
    p
}

/// Build the McpMux server entry for a client. The header value is the
/// workspace folder path; an optional bearer token is added as `Authorization`
/// when inbound auth is enabled.
fn build_entry(
    spec: &ClientSpec,
    mcp_url: &str,
    header_value: &str,
    bearer: Option<&str>,
) -> serde_json::Value {
    let mut headers = serde_json::Map::new();
    headers.insert(WORKSPACE_HEADER.to_string(), json!(header_value));
    if let Some(token) = bearer {
        headers.insert(
            "Authorization".to_string(),
            json!(format!("Bearer {token}")),
        );
    }

    let mut entry = serde_json::Map::new();
    // `type` first when present, then url, then headers — cosmetic but stable.
    if let Some(t) = spec.type_value {
        entry.insert("type".to_string(), json!(t));
    }
    entry.insert(spec.url_key.to_string(), json!(mcp_url));
    entry.insert("headers".to_string(), serde_json::Value::Object(headers));
    serde_json::Value::Object(entry)
}

/// Merge the McpMux entry into an existing config (or a fresh `{}` when there's
/// none), preserving every other server already configured. Returns the
/// pretty-printed file content.
///
/// Refuses to touch a file that isn't plain JSON (e.g. JSONC with comments) or
/// whose root / servers key isn't an object — the caller surfaces that as an
/// error rather than clobbering the user's file.
fn merge_entry(
    existing: Option<&str>,
    spec: &ClientSpec,
    entry: serde_json::Value,
) -> Result<String, String> {
    let mut root: serde_json::Value = match existing {
        Some(s) if !s.trim().is_empty() => serde_json::from_str(s).map_err(|e| {
            format!("existing config is not plain JSON ({e}); edit it by hand to add McpMux")
        })?,
        _ => json!({}),
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| "existing config root is not a JSON object".to_string())?;

    let servers = obj.entry(spec.servers_key).or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or_else(|| {
        format!(
            "'{}' in the existing config is not an object",
            spec.servers_key
        )
    })?;

    servers.insert(SERVER_NAME.to_string(), entry);

    let mut out = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
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

    let entry = build_entry(spec, mcp_url, header_value, bearer);
    let merged = match merge_entry(existing.as_deref(), spec, entry) {
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
    let entry = build_entry(spec, &server_url, &workspace_root, bearer.as_deref());
    // A full-file snippet (top-level key included) so it pastes cleanly into an
    // empty project config; merging into an existing file is what the install
    // command is for.
    let content = merge_entry(None, spec, entry)?;
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
    use serde_json::Value;

    fn spec(id: &str) -> &'static ClientSpec {
        find_client(id).unwrap()
    }

    #[test]
    fn builds_entry_with_header_and_optional_type() {
        // Cursor: no type, url + headers.
        let cursor = build_entry(
            spec("cursor"),
            "http://localhost:45818/mcp",
            "d:\\proj",
            None,
        );
        assert_eq!(cursor["url"], "http://localhost:45818/mcp");
        assert_eq!(cursor["headers"][WORKSPACE_HEADER], "d:\\proj");
        assert!(cursor.get("type").is_none());

        // VS Code: type=http.
        let vscode = build_entry(spec("vscode"), "http://x/mcp", "/p", None);
        assert_eq!(vscode["type"], "http");

        // opencode: type=remote.
        let oc = build_entry(spec("opencode"), "http://x/mcp", "/p", None);
        assert_eq!(oc["type"], "remote");
    }

    #[test]
    fn bearer_token_becomes_authorization_header() {
        let e = build_entry(spec("cursor"), "http://x/mcp", "/p", Some("abc123"));
        assert_eq!(e["headers"]["Authorization"], "Bearer abc123");
    }

    #[test]
    fn merge_into_empty_creates_top_level_key() {
        let entry = build_entry(spec("cursor"), "http://x/mcp", "/p", None);
        let out = merge_entry(None, spec("cursor"), entry).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["mcpmux"]["url"], "http://x/mcp");
    }

    #[test]
    fn vscode_uses_servers_key() {
        let entry = build_entry(spec("vscode"), "http://x/mcp", "/p", None);
        let out = merge_entry(None, spec("vscode"), entry).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["servers"]["mcpmux"]["type"], "http");
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn merge_preserves_other_servers() {
        let existing = r#"{
            "mcpServers": {
                "other": { "url": "http://other/mcp" }
            },
            "someOtherTopLevel": 42
        }"#;
        let entry = build_entry(spec("cursor"), "http://x/mcp", "/p", None);
        let out = merge_entry(Some(existing), spec("cursor"), entry).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        // Our entry is added...
        assert_eq!(v["mcpServers"]["mcpmux"]["url"], "http://x/mcp");
        // ...the sibling server is preserved...
        assert_eq!(v["mcpServers"]["other"]["url"], "http://other/mcp");
        // ...and unrelated top-level keys are untouched.
        assert_eq!(v["someOtherTopLevel"], 42);
    }

    #[test]
    fn merge_replaces_an_existing_mcpmux_entry() {
        let existing = r#"{ "mcpServers": { "mcpmux": { "url": "http://old/mcp" } } }"#;
        let entry = build_entry(spec("cursor"), "http://new/mcp", "/p", None);
        let out = merge_entry(Some(existing), spec("cursor"), entry).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["mcpmux"]["url"], "http://new/mcp");
    }

    #[test]
    fn merge_rejects_non_json_existing() {
        // JSONC with a comment is not plain JSON — refuse rather than clobber.
        let existing = "{ // a comment\n  \"servers\": {} }";
        let entry = build_entry(spec("vscode"), "http://x/mcp", "/p", None);
        assert!(merge_entry(Some(existing), spec("vscode"), entry).is_err());
    }

    #[test]
    fn merge_rejects_non_object_servers_key() {
        let existing = r#"{ "mcpServers": "oops" }"#;
        let entry = build_entry(spec("cursor"), "http://x/mcp", "/p", None);
        assert!(merge_entry(Some(existing), spec("cursor"), entry).is_err());
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
        assert!(written.contains(WORKSPACE_HEADER));

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
    fn snippet_lists_all_clients() {
        let clients = list_workspace_install_clients();
        let ids: Vec<&str> = clients.iter().map(|c| c.id.as_str()).collect();
        for expected in ["cursor", "claude-code", "vscode", "opencode", "zed"] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
    }
}
