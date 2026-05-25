//! `mcpmux_invoke_tool` — permission-checked gateway into backend MCP tools.

use async_trait::async_trait;
use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Value};

use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use super::tools::{caller_resolution, caller_space_id};
use crate::pool::{format_invoke_permission_denied, format_server_inactive_error};
use crate::services::tool_discovery::ToolDiscoveryService;
use mcpmux_core::FeatureType;

/// Meta tool that forwards invocations to [`RoutingService::call_tool`].
pub struct InvokeToolTool;

#[async_trait]
impl MetaTool for InvokeToolTool {
    fn name(&self) -> &'static str {
        "mcpmux_invoke_tool"
    }

    fn description(&self) -> &'static str {
        "Invoke a backend MCP tool by server_id and tool name. Requires the \
         server to be active (binding or session enable) and the tool to be \
         in the current permission set. Use mcpmux_search_tools and \
         mcpmux_get_tool_schema before calling."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["server_id", "tool"],
            "properties": {
                "server_id": {
                    "type": "string",
                    "description": "Registry server id (e.g. github)"
                },
                "tool": {
                    "type": "string",
                    "description": "Bare tool name on that server (e.g. list_issues), not the qualified name"
                },
                "args": {
                    "type": "object",
                    "description": "Arguments object passed to the backend tool",
                    "default": {}
                }
            }
        })
    }

    fn is_write(&self) -> bool {
        false
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let server_id = call
            .args
            .get("server_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `server_id`".into()))?
            .to_string();
        let tool_name = call
            .args
            .get("tool")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `tool`".into()))?
            .to_string();
        let args = call.args.get("args").cloned().unwrap_or_else(|| json!({}));

        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;
        let session_id = call.session_id;

        let invokable = call
            .ctx
            .feature_service
            .get_invokable_tools_for_grants(
                &space_id.to_string(),
                &resolved.feature_set_ids,
                session_id,
            )
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
        let session_enabled = session_id
            .map(|sid| call.ctx.session_overrides.enabled_set(sid))
            .unwrap_or_default();
        let session_disabled = session_id
            .map(|sid| call.ctx.session_overrides.disabled_set(sid))
            .unwrap_or_default();

        let is_server_active = binding_servers.contains(&server_id)
            || (session_enabled.contains(&server_id) && !session_disabled.contains(&server_id));

        if session_disabled.contains(&server_id) {
            return Ok(invoke_error(format!(
                "server '{server_id}' is disabled for this session → mcpmux_enable_server({{ \"server_id\": \"{server_id}\" }})"
            )));
        }

        if !is_server_active {
            return Ok(invoke_error(format_server_inactive_error(&server_id)));
        }

        let qualified_name = invokable
            .iter()
            .find(|f| f.server_id == server_id && f.feature_name == tool_name)
            .map(|f| f.qualified_name())
            .unwrap_or_else(|| format!("{server_id}_{tool_name}"));
        let is_invokable = invokable.iter().any(|f| {
            f.feature_type == FeatureType::Tool
                && f.server_id == server_id
                && f.feature_name == tool_name
                && f.is_available
        });

        if !is_invokable {
            let index = call
                .ctx
                .tool_discovery
                .build_index(&space_id.to_string(), &invokable)
                .await
                .map_err(|e| MetaToolError::Internal(e.to_string()))?;
            let suggestions: Vec<String> = ToolDiscoveryService::search(
                &index,
                Some(&tool_name),
                Some(&server_id),
                crate::services::tool_discovery::DetailLevel::Name,
                5,
                None,
            )
            .tools
            .iter()
            .filter_map(|v| {
                v.get("qualified_name")
                    .and_then(|n| n.as_str().map(String::from))
            })
            .collect();
            return Ok(invoke_error(format_invoke_permission_denied(
                &qualified_name,
                &server_id,
                &tool_name,
                &suggestions,
            )));
        }

        let routing = call
            .ctx
            .routing_service
            .as_ref()
            .ok_or_else(|| MetaToolError::Internal("invoke routing not configured".into()))?;
        match routing
            .call_tool(
                space_id,
                &resolved.feature_set_ids,
                session_id,
                &qualified_name,
                args,
            )
            .await
        {
            Ok(result) => {
                let content: Vec<Content> = result
                    .content
                    .into_iter()
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect();
                let mut mcp_result = if result.is_error {
                    CallToolResult::error(content)
                } else {
                    CallToolResult::success(content)
                };
                mcp_result.structured_content = result.structured_content;
                Ok(mcp_result)
            }
            Err(e) => Ok(invoke_error(e.to_string())),
        }
    }
}

/// Build a structured MCP error payload for invoke failures.
fn invoke_error(message: String) -> CallToolResult {
    let payload = json!({
        "error": "invoke_failed",
        "message": message,
    });
    CallToolResult::error(vec![Content::text(payload.to_string())])
}
