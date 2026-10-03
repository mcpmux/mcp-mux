//! Per-workspace MCP client config snippets.
//!
//! The McpMux gateway endpoint is registered in a *project-local* MCP client
//! config (e.g. `.cursor/mcp.json`) inside a workspace folder, carrying an
//! `X-Mcpmux-Workspace` header set to that folder's path. The gateway pins the
//! header and routes deterministically even for clients that report MCP
//! `roots` unreliably (notably Cursor).
//!
//! The client table here is the single source of truth shared by the desktop
//! installer and the headless `mcpmux workspace config` command. Only clients
//! with a true project-local config scope are supported — a global config can
//! hold only one header value and so cannot be per-workspace.

use serde::Serialize;
use serde_json::json;

/// The server name McpMux registers itself under in every client config.
pub const SERVER_NAME: &str = "mcpmux";

/// The per-workspace routing header. Its value is the workspace folder path
/// (or a bound id key).
pub const WORKSPACE_HEADER: &str = "X-Mcpmux-Workspace";

/// Static description of one client's project-local MCP config shape.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ClientSpec {
    /// Stable id used by the API and UI (e.g. "cursor").
    pub id: &'static str,
    /// Human label.
    pub label: &'static str,
    /// Path of the config file relative to the workspace folder, as segments.
    pub rel_path: &'static [&'static str],
    /// Top-level object the server entry nests under.
    pub servers_key: &'static str,
    /// The key the endpoint URL goes under.
    pub url_key: &'static str,
    /// The transport `type` value when the client requires one.
    pub type_value: Option<&'static str>,
}

/// The supported project-local clients.
pub const CLIENTS: &[ClientSpec] = &[
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

/// Look up a supported client by id.
pub fn find_client(id: &str) -> Option<&'static ClientSpec> {
    CLIENTS.iter().find(|c| c.id == id)
}

/// Build the McpMux server entry for a client. The header value is the
/// workspace folder path; an optional bearer token is added as `Authorization`
/// when inbound auth is enabled.
pub fn build_entry(
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
    if let Some(t) = spec.type_value {
        entry.insert("type".to_string(), json!(t));
    }
    entry.insert(spec.url_key.to_string(), json!(mcp_url));
    entry.insert("headers".to_string(), serde_json::Value::Object(headers));
    serde_json::Value::Object(entry)
}

/// Merge the McpMux entry into a fresh config, returning pretty-printed file
/// content with the top-level key included so it pastes cleanly into an empty
/// project config.
pub fn snippet(spec: &ClientSpec, mcp_url: &str, header_value: &str) -> Result<String, String> {
    let entry = build_entry(spec, mcp_url, header_value, None);
    let mut root = serde_json::Map::new();
    let mut servers = serde_json::Map::new();
    servers.insert(SERVER_NAME.to_string(), entry);
    root.insert(
        spec.servers_key.to_string(),
        serde_json::Value::Object(servers),
    );

    let mut out = serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_entry_has_header_and_no_type() {
        let entry = build_entry(find_client("cursor").unwrap(), "http://x/mcp", "/p", None);
        assert_eq!(entry["url"], "http://x/mcp");
        assert_eq!(entry["headers"][WORKSPACE_HEADER], "/p");
        assert!(entry.get("type").is_none());
    }

    #[test]
    fn vscode_entry_uses_type_and_servers_key() {
        let spec = find_client("vscode").unwrap();
        let entry = build_entry(spec, "http://x/mcp", "/p", None);
        assert_eq!(entry["type"], "http");
        let content = snippet(spec, "http://x/mcp", "/p").unwrap();
        assert!(content.contains("\"servers\""));
        assert!(content.contains("X-Mcpmux-Workspace"));
    }

    #[test]
    fn bearer_becomes_authorization_header() {
        let entry = build_entry(
            find_client("cursor").unwrap(),
            "http://x/mcp",
            "/p",
            Some("abc"),
        );
        assert_eq!(entry["headers"]["Authorization"], "Bearer abc");
    }
}
