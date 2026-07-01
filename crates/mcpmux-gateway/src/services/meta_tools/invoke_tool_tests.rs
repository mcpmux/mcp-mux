use super::*;
use serde_json::json;

#[test]
fn resolve_invoke_tool_args_prefers_args_over_params() {
    let call_args = json!({
        "args": { "owner": "a" },
        "params": { "owner": "b" }
    });
    assert_eq!(
        resolve_invoke_tool_args(&call_args),
        json!({ "owner": "a" })
    );
}

#[test]
fn resolve_invoke_tool_args_falls_back_to_params() {
    let call_args = json!({ "params": { "repo": "mcp-mux" } });
    assert_eq!(
        resolve_invoke_tool_args(&call_args),
        json!({ "repo": "mcp-mux" })
    );
}

#[test]
fn resolve_invoke_tool_args_defaults_to_empty_object() {
    assert_eq!(resolve_invoke_tool_args(&json!({})), json!({}));
}

#[test]
fn resolve_invoke_tool_args_falls_back_to_arguments() {
    let call_args = json!({ "arguments": { "id": "page-1" } });
    assert_eq!(
        resolve_invoke_tool_args(&call_args),
        json!({ "id": "page-1" })
    );
}

#[test]
fn resolve_invoke_tool_args_prefers_args_over_arguments() {
    let call_args = json!({
        "args": { "id": "a" },
        "arguments": { "id": "b" }
    });
    assert_eq!(resolve_invoke_tool_args(&call_args), json!({ "id": "a" }));
}

#[test]
fn resolve_invoke_server_id_accepts_aliases() {
    assert_eq!(
        resolve_invoke_server_id(&json!({ "server_id": "github" })),
        Some("github".to_string())
    );
    assert_eq!(
        resolve_invoke_server_id(&json!({ "server": "notion" })),
        Some("notion".to_string())
    );
    assert_eq!(
        resolve_invoke_server_id(&json!({ "serverId": "jira" })),
        Some("jira".to_string())
    );
}

#[test]
fn resolve_invoke_server_id_prefers_server_id_over_aliases() {
    let call_args = json!({
        "server_id": "canonical",
        "server": "alias",
        "serverId": "other"
    });
    assert_eq!(
        resolve_invoke_server_id(&call_args),
        Some("canonical".to_string())
    );
}

#[test]
fn resolve_invoke_tool_accepts_tool_name_alias() {
    assert_eq!(
        resolve_invoke_tool(&json!({ "tool_name": "notion-fetch" })),
        Some("notion-fetch".to_string())
    );
}

#[test]
fn resolve_invoke_tool_prefers_tool_over_tool_name() {
    let call_args = json!({
        "tool": "bare",
        "tool_name": "alias"
    });
    assert_eq!(resolve_invoke_tool(&call_args), Some("bare".to_string()));
}

#[test]
fn normalize_invoke_tool_name_strips_server_prefix() {
    assert_eq!(
        normalize_invoke_tool_name("github", "github_list_issues"),
        "list_issues"
    );
}

#[test]
fn normalize_invoke_tool_name_passes_bare_through() {
    assert_eq!(
        normalize_invoke_tool_name("github", "list_issues"),
        "list_issues"
    );
}

#[test]
fn normalize_invoke_tool_name_strips_repeated_prefix() {
    assert_eq!(
        normalize_invoke_tool_name("github", "github_github_list_issues"),
        "list_issues"
    );
}

#[test]
fn feature_matches_tool_name_accepts_qualified_or_bare() {
    assert!(feature_matches_tool_name(
        "list_issues",
        "github_list_issues",
        "github_list_issues",
        "list_issues"
    ));
    assert!(feature_matches_tool_name(
        "list_issues",
        "github_list_issues",
        "list_issues",
        "list_issues"
    ));
    assert!(!feature_matches_tool_name(
        "other_tool",
        "github_other_tool",
        "list_issues",
        "list_issues"
    ));
}
