//! `mcpmux_set_workspace_root` — manually declare this session's workspace root.
//!
//! Emergency escape hatch for roots-capable clients (e.g. older Cursor versions)
//! that declare the MCP `roots` capability at `initialize` but never respond to
//! server-initiated `roots/list` probes. In that scenario the resolver stays in
//! the `PendingRoots` state and returns an empty FeatureSet — all backend servers
//! show as `bindable` even when a binding already exists in the database.
//!
//! Calling this tool injects `workspace_root` into the session registry for the
//! current session, re-triggers resolution, and fires `tools/list_changed` so
//! the session immediately sees its bound tools.

use async_trait::async_trait;
use mcpmux_core::normalize_workspace_root;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use super::meta_tool_common::{emit_tools_list_changed, text_result};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};

pub struct SetWorkspaceRootTool;

#[async_trait]
impl MetaTool for SetWorkspaceRootTool {
    fn name(&self) -> &'static str {
        "mcpmux_set_workspace_root"
    }

    fn description(&self) -> &'static str {
        "Emergency escape hatch: manually declare this session's workspace root when \
         the automatic roots/list probe is not working (e.g. Cursor reports the MCP \
         roots capability but never responds to list_roots). Injects the given path \
         into the session registry and re-resolves the FeatureSet binding — all bound \
         servers become available immediately without restarting. Use when \
         mcpmux_list_servers shows readiness: bindable for all servers despite a \
         workspace binding already existing in McpMux."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["workspace_root"],
            "properties": {
                "workspace_root": {
                    "type": "string",
                    "description": "Absolute path or file:// URI of the workspace root \
                                    (e.g. /Users/joe/myproject)"
                }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let raw_root = call
            .args
            .get("workspace_root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `workspace_root`".into()))?;

        let session_id = call.session_id.ok_or_else(|| {
            MetaToolError::InvalidArgument(
                "no session id — stateless transport cannot track workspace root".into(),
            )
        })?;

        let normalized = normalize_workspace_root(raw_root);
        if normalized.is_empty() {
            return Err(MetaToolError::InvalidArgument(format!(
                "workspace_root `{raw_root}` normalized to an empty string — provide an absolute path"
            )));
        }

        call.ctx
            .session_roots
            .set(session_id, std::iter::once(normalized.as_str()));

        let resolved = call
            .ctx
            .resolver
            .resolve(Some(session_id), Some(call.client_id), call.request_machine_id)
            .await?;

        let space_id = resolved
            .space_id
            .ok_or_else(|| MetaToolError::Internal("no Space resolved".into()))?;

        emit_tools_list_changed(&call.ctx.domain_event_tx, space_id);

        let message = if resolved.feature_set_ids.is_empty() {
            format!(
                "Root injected but no binding found for '{normalized}'. \
                 Use mcpmux_bind_current_workspace to create one."
            )
        } else {
            format!(
                "Root injected and binding resolved ({} FeatureSet(s)). \
                 tools/list_changed fired — your bound servers are now available.",
                resolved.feature_set_ids.len()
            )
        };

        Ok(text_result(json!({
            "ok": true,
            "workspace_root": normalized,
            "session_id": session_id,
            "resolved_feature_set_ids": resolved.feature_set_ids,
            "resolution_source": resolved.source,
            "message": message
        })))
    }
}
