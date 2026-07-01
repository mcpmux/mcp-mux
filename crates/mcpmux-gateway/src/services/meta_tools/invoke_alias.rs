//! Invoke argument alias resolution for `mcpmux_invoke_tool`.

use serde_json::{json, Value};

/// Strip repeated `{server_id}_` prefixes when agents pass a qualified name from search.
pub fn normalize_invoke_tool_name(server_id: &str, tool: &str) -> String {
    let prefix = format!("{server_id}_");
    let mut bare = tool;
    while let Some(stripped) = bare.strip_prefix(&prefix) {
        bare = stripped;
    }
    bare.to_string()
}

/// First non-empty string value for any of `keys` on a JSON object (agent alias resolution).
fn first_nonempty_str(args: &Value, keys: &[&str]) -> Option<String> {
    let obj = args.as_object()?;
    for key in keys {
        let Some(value) = obj.get(*key) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            continue;
        };
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

/// Resolve `server_id` from invoke call args (`server_id`, alias `serverId`, alias `server`).
pub fn resolve_invoke_server_id(args: &Value) -> Option<String> {
    first_nonempty_str(args, &["server_id", "serverId", "server"])
}

/// Resolve `tool` from invoke call args (`tool`, alias `tool_name`).
pub fn resolve_invoke_tool(args: &Value) -> Option<String> {
    first_nonempty_str(args, &["tool", "tool_name"])
}

/// Whether an invokable feature matches the caller's `tool` (bare or qualified).
pub(crate) fn feature_matches_tool_name(
    feature_name: &str,
    qualified_name: &str,
    tool_input: &str,
    bare: &str,
) -> bool {
    feature_name == bare || qualified_name == tool_input
}

/// Resolve backend tool arguments from `mcpmux_invoke_tool` call args.
///
/// Prefers `args`, then `params`, then `arguments`, then `tool_arguments` (common agent/UI aliases).
pub fn resolve_invoke_tool_args(args: &Value) -> Value {
    args.get("args")
        .or_else(|| args.get("params"))
        .or_else(|| args.get("arguments"))
        .or_else(|| args.get("tool_arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}))
}
