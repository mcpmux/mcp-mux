//! `mcpmux_invoke_tool` — permission-checked gateway into backend MCP tools.

pub(crate) use super::invoke_alias::feature_matches_tool_name;
pub use super::invoke_alias::{
    normalize_invoke_tool_name, resolve_invoke_server_id, resolve_invoke_tool,
    resolve_invoke_tool_args,
};

use async_trait::async_trait;
use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use tracing::debug;

use super::diagnose_server::parse_missing_required_inputs;
use super::invoke_result_filter::{apply_invoke_result_filter, parse_invoke_filter};
use super::meta_tool_common::{
    caller_resolution, caller_space_id, classify_invoke_denial,
    format_invoke_not_ready_action_with_name,
};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use crate::pool::{format_invoke_permission_denied, ConnectionStatus};
use crate::services::levenshtein_suggestions;
use mcpmux_core::{DefaultParamsStrategy, FeatureType};

/// Meta tool that forwards invocations to [`RoutingService::call_tool`].
pub struct InvokeToolTool;

#[async_trait]
impl MetaTool for InvokeToolTool {
    fn name(&self) -> &'static str {
        "mcpmux_invoke_tool"
    }

    fn description(&self) -> &'static str {
        "Invoke a backend MCP tool by server_id and tool (bare or qualified from \
         mcpmux_search_tools). Skip search when you already know the tool — pass \
         bare_name or qualified_name directly. Set preflight: true to check readiness \
         without calling the backend (returns { ready: true } or a structured not_ready \
         error). Requires the server to be ready and the tool in the current permission \
         set. Search results include required_params types — mcpmux_get_tool_schema is \
         optional for complex tools. Pass an optional filter to bound large payloads; omit \
         filter to return the backend response as-is."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["server_id", "tool"],
            "properties": {
                "server_id": {
                    "type": "string",
                    "description": "Registry server id (e.g. github). Aliases: server, serverId (server_id wins if multiple are set)."
                },
                "server": {
                    "type": "string",
                    "description": "Alias for server_id"
                },
                "serverId": {
                    "type": "string",
                    "description": "Alias for server_id"
                },
                "tool": {
                    "type": "string",
                    "description": "Tool name on that server — bare (e.g. list_issues) or qualified from mcpmux_search_tools (e.g. github_list_issues); bare_name in search results is the invoke value when unsure. Known tools can be invoked directly without a prior search. Alias: tool_name (tool wins if both are set)."
                },
                "tool_name": {
                    "type": "string",
                    "description": "Alias for tool"
                },
                "preflight": {
                    "type": "boolean",
                    "default": false,
                    "description": "When true, verify server and tool readiness without calling the backend. Returns { ready: true } on success or a structured not_ready error (same shape as a failed invoke)."
                },
                "args": {
                    "type": "object",
                    "description": "Arguments object passed to the backend tool. Aliases: params, arguments (args wins if multiple are set).",
                    "default": {}
                },
                "params": {
                    "type": "object",
                    "description": "Alias for args"
                },
                "arguments": {
                    "type": "object",
                    "description": "Alias for args"
                },
                "filter": {
                    "type": "object",
                    "description": "Optional result shaping (max_rows, max_bytes, fields, format). Omit to return the backend response as-is.",
                    "properties": {
                        "max_rows": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum rows/items to return from large arrays"
                        },
                        "max_bytes": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum UTF-8 bytes for text or serialized JSON payloads"
                        },
                        "fields": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "When set, keep only these fields on each object in list results"
                        },
                        "format": {
                            "type": "string",
                            "enum": ["summary", "full"],
                            "description": "When max_rows is set: summary caps the sample at min(max_rows, 5); full returns up to max_rows rows. Ignored when max_rows is omitted."
                        }
                    }
                }
            }
        })
    }

    fn is_write(&self) -> bool {
        false
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let server_id = resolve_invoke_server_id(&call.args).ok_or_else(|| {
            MetaToolError::InvalidArgument("missing `server_id` (aliases: server, serverId)".into())
        })?;
        let tool_input = resolve_invoke_tool(&call.args).ok_or_else(|| {
            MetaToolError::InvalidArgument(
                "missing `tool` (aliases: tool_name; bare or qualified, e.g. \"list_issues\" or \"github_list_issues\")"
                    .into(),
            )
        })?;
        let bare_tool_name = normalize_invoke_tool_name(&server_id, &tool_input);
        let preflight = call
            .args
            .get("preflight")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let args = resolve_invoke_tool_args(&call.args);
        let filter = parse_invoke_filter(call.args.get("filter"));

        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;

        let invokable = call
            .ctx
            .feature_service
            .get_invokable_tools_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let binding_features = call
            .ctx
            .feature_service
            .resolve_feature_sets(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;
        let binding_servers: std::collections::HashSet<String> = binding_features
            .iter()
            .map(|f| f.server_id.clone())
            .collect();

        let installed = call
            .ctx
            .installed_server_repo
            .get_by_server_id(&space_id.to_string(), &server_id)
            .await
            .ok()
            .flatten();
        let server_display_name = installed.as_ref().map(|s| s.display_name().to_string());

        if !binding_servers.contains(&server_id) {
            let (reason, tool) =
                classify_invoke_denial(false, ConnectionStatus::Disconnected, false)
                    .unwrap_or(("inactive", "mcpmux_bind_current_workspace"));
            return Ok(invoke_not_ready(
                reason,
                format_invoke_not_ready_action_with_name(
                    reason,
                    &server_id,
                    server_display_name.as_deref(),
                ),
                tool,
            ));
        }

        let pool_statuses = call.ctx.server_manager.get_all_statuses(space_id).await;
        let connection_status = pool_statuses
            .get(&server_id)
            .map(|(status, _, _, _)| *status)
            .unwrap_or(ConnectionStatus::Disconnected);
        let has_missing_inputs = installed
            .as_ref()
            .map(|server| !parse_missing_required_inputs(server).is_empty())
            .unwrap_or(false);

        if let Some((reason, tool)) =
            classify_invoke_denial(true, connection_status, has_missing_inputs)
        {
            return Ok(invoke_not_ready(
                reason,
                format_invoke_not_ready_action_with_name(
                    reason,
                    &server_id,
                    server_display_name.as_deref(),
                ),
                tool,
            ));
        }

        let matched = invokable.iter().find(|f| {
            f.feature_type == FeatureType::Tool
                && f.server_id == server_id
                && feature_matches_tool_name(
                    &f.feature_name,
                    &f.qualified_name(),
                    &tool_input,
                    &bare_tool_name,
                )
        });
        let qualified_name = matched.map(|f| f.qualified_name()).unwrap_or_else(|| {
            if tool_input.starts_with(&format!("{server_id}_")) {
                tool_input.clone()
            } else {
                format!("{server_id}_{bare_tool_name}")
            }
        });
        let is_invokable = matched.map(|f| f.is_available).unwrap_or(false);

        if !is_invokable {
            if preflight {
                return Ok(invoke_not_ready(
                    "permission_denied",
                    format_invoke_not_ready_action_with_name(
                        "permission_denied",
                        &server_id,
                        server_display_name.as_deref(),
                    ),
                    "mcpmux_search_tools",
                ));
            }
            let candidates: Vec<String> = invokable
                .iter()
                .filter(|f| f.server_id == server_id)
                .map(|f| f.feature_name.clone())
                .collect();
            let suggestions = levenshtein_suggestions(&bare_tool_name, &candidates, 5);
            return Ok(invoke_error(format_invoke_permission_denied(
                &qualified_name,
                &server_id,
                &bare_tool_name,
                &suggestions,
            )));
        }

        if preflight {
            return Ok(invoke_preflight_ok());
        }

        let effective_args = match installed {
            Some(server) => {
                merge_default_params(args, &server.default_params, server.default_params_strategy)
            }
            None => args,
        };

        let backend = call
            .ctx
            .invoke_backend
            .as_ref()
            .ok_or_else(|| MetaToolError::Internal("invoke routing not configured".into()))?;
        match backend
            .call_tool(
                space_id,
                &resolved.feature_set_ids,
                &qualified_name,
                effective_args,
            )
            .await
        {
            Ok(result) => {
                if result.is_error {
                    let content: Vec<Content> = result
                        .content
                        .into_iter()
                        .filter_map(|v| serde_json::from_value(v).ok())
                        .collect();
                    let mut mcp_result = CallToolResult::error(content);
                    mcp_result.structured_content = result.structured_content;
                    return Ok(mcp_result);
                }

                let (content, structured_content) = match filter.as_ref().filter(|f| f.has_effect())
                {
                    Some(active_filter) => apply_invoke_result_filter(
                        result.content,
                        result.structured_content,
                        active_filter,
                    ),
                    None => (result.content, result.structured_content),
                };
                let parsed_content: Vec<Content> = content
                    .into_iter()
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect();
                let mut mcp_result = CallToolResult::success(parsed_content);
                mcp_result.structured_content = structured_content;
                Ok(mcp_result)
            }
            Err(e) => Ok(invoke_error(e.to_string())),
        }
    }
}

/// Merge per-server default params with caller-supplied args.
///
/// `Fill` (default): `{ ...defaults, ...caller_args }` — caller wins on collision.
/// `Override`:       `{ ...caller_args, ...defaults }` — defaults win on collision.
///
/// Returns `args` unchanged when `defaults` is empty or `args` is not an Object.
fn merge_default_params(
    args: Value,
    defaults: &HashMap<String, Value>,
    strategy: DefaultParamsStrategy,
) -> Value {
    if defaults.is_empty() {
        return args;
    }
    let Value::Object(caller_map) = args else {
        debug!("merge_default_params: args is not an Object; server defaults not applied");
        return args;
    };
    let defaults_map: Map<String, Value> = defaults
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut merged = match strategy {
        DefaultParamsStrategy::Fill => {
            // defaults as base, caller overwrites
            let mut m = defaults_map;
            m.extend(caller_map);
            m
        }
        DefaultParamsStrategy::Override => {
            // caller as base, defaults overwrite
            let mut m: Map<String, Value> = caller_map;
            m.extend(defaults_map);
            m
        }
    };
    // keep deterministic key order for tests
    merged.sort_keys();
    Value::Object(merged)
}

/// Build a structured MCP error payload for invoke failures.
fn invoke_error(message: String) -> CallToolResult {
    let payload = json!({
        "error": "invoke_failed",
        "message": message,
    });
    CallToolResult::error(vec![Content::text(payload.to_string())])
}

/// Build a structured not-ready denial before backend dispatch.
fn invoke_not_ready(reason: &str, action: String, tool: &str) -> CallToolResult {
    let payload = json!({
        "error": "not_ready",
        "reason": reason,
        "action": action,
        "tool": tool,
    });
    CallToolResult::error(vec![Content::text(payload.to_string())])
}

/// Successful preflight response — readiness verified, no backend call.
fn invoke_preflight_ok() -> CallToolResult {
    CallToolResult::success(vec![Content::text(json!({ "ready": true }).to_string())])
}

#[cfg(test)]
#[path = "invoke_tool_tests.rs"]
mod tests;
