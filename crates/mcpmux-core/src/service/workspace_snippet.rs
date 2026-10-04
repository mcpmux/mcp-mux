//! Per-workspace MCP client config snippets.
//!
//! The McpMux gateway endpoint is registered in a *project-local* MCP client
//! config (e.g. `.cursor/mcp.json`) inside a workspace folder, carrying an
//! `X-Mcpmux-Workspace` header set to that folder's path. The gateway pins the
//! header and routes deterministically even for clients that report MCP
//! `roots` unreliably (notably Cursor).
//!
//! The client table and config rendering here are the single source of truth
//! shared by the desktop installer and the headless `mcpmux workspace config`
//! command. Only clients with a true project-local config scope are supported —
//! a global config can hold only one header value and so cannot be
//! per-workspace.

use serde::Serialize;
use serde_json::json;

/// The server name McpMux registers itself under in every client config.
pub const SERVER_NAME: &str = "mcpmux";

/// The per-workspace routing header. Its value is the workspace folder path
/// (or a bound id key).
pub const WORKSPACE_HEADER: &str = "X-Mcpmux-Workspace";

/// File format of a client's config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFileFormat {
    Json,
    Toml,
}

/// Static description of one client's project-local MCP config shape.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ClientSpec {
    /// Stable id used by the API and UI (e.g. "cursor").
    pub id: &'static str,
    /// Human label.
    pub label: &'static str,
    /// Path of the config file relative to the workspace folder, as segments.
    pub rel_path: &'static [&'static str],
    /// File format of the config.
    pub format: ConfigFileFormat,
    /// Top-level object the server entry nests under. Differs across clients:
    /// `mcpServers` (Cursor/Claude Code), `servers` (VS Code), `mcp`
    /// (opencode), `context_servers` (Zed), `mcp_servers` (Codex).
    pub servers_key: &'static str,
    /// The key the endpoint URL goes under.
    pub url_key: &'static str,
    /// The key the static request headers go under (`headers`, or
    /// `http_headers` for Codex).
    pub headers_key: &'static str,
    /// The transport `type` value when the client requires one. `http` for
    /// Claude Code / VS Code, `remote` for opencode; Cursor, Zed and Codex
    /// infer it from the presence of `url`, so they get `None`.
    pub type_value: Option<&'static str>,
}

/// The supported project-local clients.
pub const CLIENTS: &[ClientSpec] = &[
    ClientSpec {
        id: "cursor",
        label: "Cursor",
        rel_path: &[".cursor", "mcp.json"],
        format: ConfigFileFormat::Json,
        servers_key: "mcpServers",
        url_key: "url",
        headers_key: "headers",
        type_value: None,
    },
    ClientSpec {
        id: "claude-code",
        label: "Claude Code",
        rel_path: &[".mcp.json"],
        format: ConfigFileFormat::Json,
        servers_key: "mcpServers",
        url_key: "url",
        headers_key: "headers",
        type_value: Some("http"),
    },
    ClientSpec {
        id: "vscode",
        label: "VS Code / Copilot",
        rel_path: &[".vscode", "mcp.json"],
        format: ConfigFileFormat::Json,
        servers_key: "servers",
        url_key: "url",
        headers_key: "headers",
        type_value: Some("http"),
    },
    ClientSpec {
        id: "opencode",
        label: "opencode",
        rel_path: &["opencode.json"],
        format: ConfigFileFormat::Json,
        servers_key: "mcp",
        url_key: "url",
        headers_key: "headers",
        type_value: Some("remote"),
    },
    ClientSpec {
        id: "zed",
        label: "Zed",
        rel_path: &[".zed", "settings.json"],
        format: ConfigFileFormat::Json,
        servers_key: "context_servers",
        url_key: "url",
        headers_key: "headers",
        type_value: None,
    },
    // Codex only loads a project's `.codex/config.toml` once the project is
    // trusted in Codex.
    ClientSpec {
        id: "codex",
        label: "Codex",
        rel_path: &[".codex", "config.toml"],
        format: ConfigFileFormat::Toml,
        servers_key: "mcp_servers",
        url_key: "url",
        headers_key: "http_headers",
        type_value: None,
    },
];

/// Look up a supported client by id.
pub fn find_client(id: &str) -> Option<&'static ClientSpec> {
    CLIENTS.iter().find(|c| c.id == id)
}

/// Build the McpMux server entry for a client. The header value is the
/// workspace folder path; an optional bearer token is added as `Authorization`
/// when inbound auth is enabled. TOML clients get the same keys rendered as a
/// TOML table.
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
    // `type` first when present, then url, then headers — cosmetic but stable.
    if let Some(t) = spec.type_value {
        entry.insert("type".to_string(), json!(t));
    }
    entry.insert(spec.url_key.to_string(), json!(mcp_url));
    entry.insert(
        spec.headers_key.to_string(),
        serde_json::Value::Object(headers),
    );
    serde_json::Value::Object(entry)
}

/// Merge the McpMux entry into an existing config (or a fresh one when there's
/// none), preserving every other server and setting already configured.
/// Returns the full file content.
///
/// Refuses to touch a file that doesn't parse (e.g. JSONC with comments) or
/// whose root / servers key isn't an object — the caller surfaces that as an
/// error rather than clobbering the user's file.
pub fn render_config(
    existing: Option<&str>,
    spec: &ClientSpec,
    mcp_url: &str,
    header_value: &str,
    bearer: Option<&str>,
) -> Result<String, String> {
    let existing = existing.filter(|s| !s.trim().is_empty());
    let entry = build_entry(spec, mcp_url, header_value, bearer);
    match spec.format {
        ConfigFileFormat::Json => merge_json(existing, spec, entry),
        ConfigFileFormat::Toml => merge_toml(existing, spec, &entry),
    }
}

/// A fresh config holding just the McpMux entry, with the top-level key
/// included so it pastes cleanly into an empty project config.
pub fn snippet(spec: &ClientSpec, mcp_url: &str, header_value: &str) -> Result<String, String> {
    render_config(None, spec, mcp_url, header_value, None)
}

fn merge_json(
    existing: Option<&str>,
    spec: &ClientSpec,
    entry: serde_json::Value,
) -> Result<String, String> {
    let mut root: serde_json::Value = match existing {
        Some(s) => serde_json::from_str(s).map_err(|e| {
            format!("existing config is not plain JSON ({e}); edit it by hand to add McpMux")
        })?,
        None => json!({}),
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

/// TOML counterpart of [`merge_json`]. Edits the document in place so the
/// user's comments and formatting survive.
fn merge_toml(
    existing: Option<&str>,
    spec: &ClientSpec,
    entry: &serde_json::Value,
) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table, Value};

    let mut doc: DocumentMut = match existing {
        Some(s) => s.parse().map_err(|e| {
            format!("existing config is not valid TOML ({e}); edit it by hand to add McpMux")
        })?,
        None => DocumentMut::new(),
    };

    let mut table = toml_table(entry);
    let servers = doc.entry(spec.servers_key).or_insert_with(|| {
        // Implicit, so a fresh file gets `[mcp_servers.mcpmux]` without an
        // empty `[mcp_servers]` header above it.
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    match servers {
        Item::Table(servers) => {
            // Replacing an existing entry keeps its place in the file and the
            // comments above it.
            if let Some(old) = servers.get(SERVER_NAME).and_then(Item::as_table) {
                if let Some(pos) = old.position() {
                    table.set_position(pos);
                }
                *table.decor_mut() = old.decor().clone();
            }
            servers.insert(SERVER_NAME, Item::Table(table));
        }
        Item::Value(Value::InlineTable(servers)) => {
            servers.insert(SERVER_NAME, Value::InlineTable(table.into_inline_table()));
        }
        _ => {
            return Err(format!(
                "'{}' in the existing config is not a table",
                spec.servers_key
            ))
        }
    }

    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Render a [`build_entry`] value (string fields plus one header map) as a
/// TOML table, with the header map inline.
fn toml_table(entry: &serde_json::Value) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    for (key, val) in entry.as_object().into_iter().flatten() {
        match val {
            serde_json::Value::String(s) => {
                table.insert(key, toml_edit::value(s.as_str()));
            }
            serde_json::Value::Object(map) => {
                let mut inline = toml_edit::InlineTable::new();
                for (k, v) in map {
                    if let Some(s) = v.as_str() {
                        inline.insert(k, s.into());
                    }
                }
                table.insert(key, toml_edit::value(inline));
            }
            _ => {}
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn spec(id: &str) -> &'static ClientSpec {
        find_client(id).unwrap()
    }

    fn parse_toml(s: &str) -> toml_edit::DocumentMut {
        s.parse().expect("output is valid TOML")
    }

    #[test]
    fn cursor_entry_has_header_and_no_type() {
        let entry = build_entry(spec("cursor"), "http://x/mcp", "d:\\proj", None);
        assert_eq!(entry["url"], "http://x/mcp");
        assert_eq!(entry["headers"][WORKSPACE_HEADER], "d:\\proj");
        assert!(entry.get("type").is_none());
    }

    #[test]
    fn type_value_per_client() {
        let vscode = build_entry(spec("vscode"), "http://x/mcp", "/p", None);
        assert_eq!(vscode["type"], "http");
        let oc = build_entry(spec("opencode"), "http://x/mcp", "/p", None);
        assert_eq!(oc["type"], "remote");
        let codex = build_entry(spec("codex"), "http://x/mcp", "/p", None);
        assert!(codex.get("type").is_none());
    }

    #[test]
    fn bearer_becomes_authorization_header() {
        let entry = build_entry(spec("cursor"), "http://x/mcp", "/p", Some("abc"));
        assert_eq!(entry["headers"]["Authorization"], "Bearer abc");
    }

    #[test]
    fn vscode_snippet_uses_servers_key() {
        let content = snippet(spec("vscode"), "http://x/mcp", "/p").unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["servers"]["mcpmux"]["type"], "http");
        assert_eq!(v["servers"]["mcpmux"]["headers"][WORKSPACE_HEADER], "/p");
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn json_merge_into_empty_creates_top_level_key() {
        let out = render_config(Some("  \n"), spec("cursor"), "http://x/mcp", "/p", None).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["mcpmux"]["url"], "http://x/mcp");
    }

    #[test]
    fn json_merge_preserves_other_servers() {
        let existing = r#"{
            "mcpServers": {
                "other": { "url": "http://other/mcp" }
            },
            "someOtherTopLevel": 42
        }"#;
        let out =
            render_config(Some(existing), spec("cursor"), "http://x/mcp", "/p", None).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        // Our entry is added...
        assert_eq!(v["mcpServers"]["mcpmux"]["url"], "http://x/mcp");
        // ...the sibling server is preserved...
        assert_eq!(v["mcpServers"]["other"]["url"], "http://other/mcp");
        // ...and unrelated top-level keys are untouched.
        assert_eq!(v["someOtherTopLevel"], 42);
    }

    #[test]
    fn json_merge_replaces_an_existing_mcpmux_entry() {
        let existing = r#"{ "mcpServers": { "mcpmux": { "url": "http://old/mcp" } } }"#;
        let out =
            render_config(Some(existing), spec("cursor"), "http://new/mcp", "/p", None).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["mcpmux"]["url"], "http://new/mcp");
    }

    #[test]
    fn json_merge_rejects_non_json_existing() {
        // JSONC with a comment is not plain JSON — refuse rather than clobber.
        let existing = "{ // a comment\n  \"servers\": {} }";
        assert!(render_config(Some(existing), spec("vscode"), "http://x/mcp", "/p", None).is_err());
    }

    #[test]
    fn json_merge_rejects_non_object_servers_key() {
        let existing = r#"{ "mcpServers": "oops" }"#;
        assert!(render_config(Some(existing), spec("cursor"), "http://x/mcp", "/p", None).is_err());
    }

    #[test]
    fn codex_snippet_is_toml_with_http_headers() {
        let content = snippet(spec("codex"), "http://x/mcp", "d:\\proj").unwrap();
        assert!(content.starts_with("[mcp_servers.mcpmux]\n"), "{content}");
        let doc = parse_toml(&content);
        let entry = &doc["mcp_servers"]["mcpmux"];
        assert_eq!(entry["url"].as_str(), Some("http://x/mcp"));
        // Windows paths round-trip through TOML string escaping.
        assert_eq!(
            entry["http_headers"][WORKSPACE_HEADER].as_str(),
            Some("d:\\proj")
        );
        assert!(entry.get("type").is_none());
        assert!(entry.get("headers").is_none());
    }

    #[test]
    fn codex_bearer_goes_into_http_headers() {
        let out = render_config(None, spec("codex"), "http://x/mcp", "/p", Some("abc")).unwrap();
        let doc = parse_toml(&out);
        assert_eq!(
            doc["mcp_servers"]["mcpmux"]["http_headers"]["Authorization"].as_str(),
            Some("Bearer abc")
        );
    }

    #[test]
    fn codex_merge_preserves_comments_settings_and_other_servers() {
        let existing = "\
# my codex settings
model = \"gpt-5-codex\"

[mcp_servers.other]
# keep me
url = \"http://other/mcp\"
";
        let out = render_config(Some(existing), spec("codex"), "http://x/mcp", "/p", None).unwrap();
        assert!(
            out.starts_with(existing),
            "existing content rewritten:\n{out}"
        );
        let doc = parse_toml(&out);
        assert_eq!(doc["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(
            doc["mcp_servers"]["other"]["url"].as_str(),
            Some("http://other/mcp")
        );
        assert_eq!(
            doc["mcp_servers"]["mcpmux"]["url"].as_str(),
            Some("http://x/mcp")
        );
    }

    #[test]
    fn codex_merge_replaces_existing_entry_in_place() {
        let existing = "\
[mcp_servers.mcpmux]
url = \"http://old/mcp\"
enabled = false

# trailing server
[mcp_servers.other]
url = \"http://other/mcp\"
";
        let out =
            render_config(Some(existing), spec("codex"), "http://new/mcp", "/p", None).unwrap();
        let doc = parse_toml(&out);
        let entry = &doc["mcp_servers"]["mcpmux"];
        assert_eq!(entry["url"].as_str(), Some("http://new/mcp"));
        assert!(entry.get("enabled").is_none());
        assert_eq!(
            doc["mcp_servers"]["other"]["url"].as_str(),
            Some("http://other/mcp")
        );
        // The replaced entry stays ahead of the sibling it preceded.
        assert!(out.find("[mcp_servers.mcpmux]") < out.find("[mcp_servers.other]"));
        assert!(out.contains("# trailing server\n[mcp_servers.other]"));
    }

    #[test]
    fn codex_merge_into_inline_servers_table() {
        let existing = "mcp_servers = { other = { url = \"http://other/mcp\" } }\n";
        let out = render_config(Some(existing), spec("codex"), "http://x/mcp", "/p", None).unwrap();
        let doc = parse_toml(&out);
        assert_eq!(
            doc["mcp_servers"]["other"]["url"].as_str(),
            Some("http://other/mcp")
        );
        assert_eq!(
            doc["mcp_servers"]["mcpmux"]["http_headers"][WORKSPACE_HEADER].as_str(),
            Some("/p")
        );
    }

    #[test]
    fn codex_merge_rejects_invalid_toml_and_non_table_servers() {
        assert!(
            render_config(Some("model = "), spec("codex"), "http://x/mcp", "/p", None).is_err()
        );
        assert!(render_config(
            Some("mcp_servers = 1\n"),
            spec("codex"),
            "http://x/mcp",
            "/p",
            None
        )
        .is_err());
    }
}
