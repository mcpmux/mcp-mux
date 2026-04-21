//! Built-in `mcpmux_*` meta tool implementations.
//!
//! Each tool is a unit struct implementing [`MetaTool`]. Reads execute
//! directly; writes route through the [`ApprovalBroker`] first.

use async_trait::async_trait;
use mcpmux_core::{
    normalize_workspace_root, DomainEvent, FeatureType, MemberMode, WorkspaceBinding,
};
use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::info;
use uuid::Uuid;

use super::approval::{ApprovalPayload, ApprovalScope};
use super::diff::ToolDiff;
use super::registry::{MetaTool, MetaToolCall, MetaToolError};

/// Fire a `FeatureSetMembersChanged` event so MCPNotifier pushes a
/// `tools/list_changed` notification to every connected client in the Space.
/// Used by every write tool after a successful mutation.
fn emit_tools_list_changed(event_tx: &broadcast::Sender<DomainEvent>, space_id: Uuid) {
    let _ = event_tx.send(DomainEvent::FeatureSetMembersChanged {
        space_id,
        feature_set_id: "meta-tool-write".into(),
        added_count: 0,
        removed_count: 0,
    });
}

// NOTE: MetaToolInvoked audit events are emitted centrally by
// MetaToolRegistry::call, so individual tools don't need to fire them.

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn text_result(v: Value) -> CallToolResult {
    CallToolResult::success(vec![Content::text(v.to_string())])
}

/// Resolve the caller's effective Space id, using the pin if set, else the
/// default space. Returns `None` only in pathological "no-default-space"
/// setups, which the meta tools treat as errors.
async fn caller_space_id(call: &MetaToolCall<'_>) -> Result<Uuid, MetaToolError> {
    let client = call
        .ctx
        .client_repo
        .get(call.client_id)
        .await?
        .ok_or_else(|| MetaToolError::Internal("client not found".into()))?;
    if let Some(id) = client.pinned_space_id {
        return Ok(id);
    }
    let default_space = call
        .ctx
        .space_repo
        .get_default()
        .await?
        .ok_or_else(|| MetaToolError::Internal("no default space".into()))?;
    Ok(default_space.id)
}

// ---------------------------------------------------------------------------
// mcpmux_list_all_tools — read
// ---------------------------------------------------------------------------

pub struct ListAllToolsTool;

#[async_trait]
impl MetaTool for ListAllToolsTool {
    fn name(&self) -> &'static str {
        "mcpmux_list_all_tools"
    }

    fn description(&self) -> &'static str {
        "List EVERY tool available on every connected MCP server, without the \
         current FeatureSet filter applied. Useful when you want to know what's \
         possible in this workspace before deciding which tools to pin. Returns \
         an array of {server_id, qualified_name, description, available}."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = caller_space_id(&call).await?;
        let features = call
            .ctx
            .server_feature_repo
            .list_for_space(&space_id.to_string())
            .await?;
        let tools: Vec<_> = features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Tool)
            .map(|f| {
                json!({
                    "server_id": f.server_id,
                    "qualified_name": f.qualified_name(),
                    "description": f.description,
                    "available": f.is_available,
                })
            })
            .collect();
        Ok(text_result(json!({ "tools": tools })))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_list_feature_sets — read
// ---------------------------------------------------------------------------

pub struct ListFeatureSetsTool;

#[async_trait]
impl MetaTool for ListFeatureSetsTool {
    fn name(&self) -> &'static str {
        "mcpmux_list_feature_sets"
    }

    fn description(&self) -> &'static str {
        "List every FeatureSet in the caller's Space — built-ins and custom. \
         Each entry carries `id`, `name`, `type`, `is_active` (the one that \
         applies when no pin/binding matches), and `is_pinned` (this caller's \
         current pin). Use before proposing a pin so you don't recreate one \
         that already fits."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = caller_space_id(&call).await?;
        let space = call
            .ctx
            .space_repo
            .get(&space_id)
            .await?
            .ok_or_else(|| MetaToolError::Internal("space missing".into()))?;
        let client = call.ctx.client_repo.get(call.client_id).await?;
        let pinned_fs = client.as_ref().and_then(|c| c.pinned_feature_set_id);
        let sets = call
            .ctx
            .feature_set_repo
            .list_by_space(&space_id.to_string())
            .await?;
        let sets: Vec<_> = sets
            .iter()
            .filter(|fs| !fs.is_deleted)
            .map(|fs| {
                let id_uuid = Uuid::parse_str(&fs.id).ok();
                json!({
                    "id": fs.id,
                    "name": fs.name,
                    "description": fs.description,
                    "type": fs.feature_set_type,
                    "is_builtin": fs.is_builtin,
                    "is_active": id_uuid
                        .zip(space.active_feature_set_id)
                        .map(|(a, b)| a == b)
                        .unwrap_or(false),
                    "is_pinned": id_uuid
                        .zip(pinned_fs)
                        .map(|(a, b)| a == b)
                        .unwrap_or(false),
                })
            })
            .collect();
        Ok(text_result(
            json!({ "space_id": space_id, "feature_sets": sets }),
        ))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_describe_resolution — read
// ---------------------------------------------------------------------------

pub struct DescribeResolutionTool;

#[async_trait]
impl MetaTool for DescribeResolutionTool {
    fn name(&self) -> &'static str {
        "mcpmux_describe_resolution"
    }

    fn description(&self) -> &'static str {
        "Explain which FeatureSet the caller is currently resolved to and \
         why (pin | workspace_binding | space_active | deny). Always call \
         this before a write tool so you know the baseline."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let resolved = call
            .ctx
            .resolver
            .resolve(call.client_id, call.session_id)
            .await?;
        let fs_name = if let Some(id) = resolved.feature_set_id {
            call.ctx
                .feature_set_repo
                .get(&id.to_string())
                .await?
                .map(|fs| fs.name)
        } else {
            None
        };
        let tool_count = if let Some(id) = resolved.feature_set_id {
            let space_id = caller_space_id(&call).await?;
            call.ctx
                .feature_service
                .get_tools_for_grants(&space_id.to_string(), &[id.to_string()])
                .await?
                .iter()
                .filter(|f| f.is_available)
                .count()
        } else {
            0
        };
        Ok(text_result(json!({
            "feature_set_id": resolved.feature_set_id,
            "feature_set_name": fs_name,
            "source": resolved.source,
            "resolved_tool_count": tool_count,
        })))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_describe_workspace — read
// ---------------------------------------------------------------------------

pub struct DescribeWorkspaceTool;

#[async_trait]
impl MetaTool for DescribeWorkspaceTool {
    fn name(&self) -> &'static str {
        "mcpmux_describe_workspace"
    }

    fn description(&self) -> &'static str {
        "Report the workspace roots the caller declared via the MCP `roots` \
         capability, and any WorkspaceBinding in this Space that matches. \
         Empty roots means the client didn't declare the `roots` capability \
         — bindings won't apply and workspace-based tools should be skipped."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = caller_space_id(&call).await?;
        let roots = call
            .session_id
            .and_then(|sid| call.ctx.session_roots.get(sid))
            .unwrap_or_default();
        let matched = if !roots.is_empty() {
            call.ctx
                .binding_repo
                .find_longest_prefix_match(&space_id, &roots)
                .await?
        } else {
            None
        };
        Ok(text_result(json!({
            "space_id": space_id,
            "reported_roots": roots,
            "matched_binding": matched.map(|b| json!({
                "id": b.id,
                "workspace_root": b.workspace_root,
                "feature_set_id": b.feature_set_id,
            })),
        })))
    }
}

// ---------------------------------------------------------------------------
// Writes — each goes through the ApprovalBroker before mutating state.
// ---------------------------------------------------------------------------

/// Common path for every write tool: build payload, ask broker, run the
/// mutation. Returns the broker's decision so the caller can proceed only
/// on success. `mutate` is the thing that runs post-approval and is
/// expected to emit `tools/list_changed` when relevant.
async fn with_approval<F, Fut, T>(
    call: &MetaToolCall<'_>,
    tool_name: &'static str,
    summary: String,
    diff: Option<Value>,
    affects_other_clients: bool,
    raw_args: Value,
    mutate: F,
) -> Result<T, MetaToolError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, MetaToolError>>,
{
    let payload = ApprovalPayload {
        tool_name: tool_name.to_string(),
        summary,
        diff,
        raw_args,
        affects_other_clients,
    };
    call.ctx
        .approval_broker
        .request_approval(*call.client_id, tool_name, payload)
        .await?;
    mutate().await
}

fn parse_uuid_arg(args: &Value, field: &str) -> Result<Uuid, MetaToolError> {
    let s = args
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| MetaToolError::InvalidArgument(format!("missing `{field}`")))?;
    Uuid::parse_str(s)
        .map_err(|_| MetaToolError::InvalidArgument(format!("`{field}` is not a UUID: {s}")))
}

// ---------------------------------------------------------------------------
// mcpmux_pin_this_session — write (caller-scope)
// ---------------------------------------------------------------------------

pub struct PinThisSessionTool;

#[async_trait]
impl MetaTool for PinThisSessionTool {
    fn name(&self) -> &'static str {
        "mcpmux_pin_this_session"
    }

    fn description(&self) -> &'static str {
        "Pin THIS caller's access key to the given FeatureSet. Affects only \
         the calling client; does not touch other connections. Requires user \
         approval. After approval the gateway emits tools/list_changed so \
         the trimmed toolset appears on the next list_tools."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["feature_set_id"],
            "properties": {
                "feature_set_id": { "type": "string", "description": "FeatureSet UUID" }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let new_fs = parse_uuid_arg(&call.args, "feature_set_id")?;
        let space_id = caller_space_id(&call).await?;

        // Current resolution — becomes the `before` side of the diff.
        let before_resolved = call
            .ctx
            .resolver
            .resolve(call.client_id, call.session_id)
            .await?;
        let diff = ToolDiff::compute(
            &call.ctx.feature_service,
            space_id,
            before_resolved.feature_set_id,
            Some(new_fs),
        )
        .await?;

        let fs_name = call
            .ctx
            .feature_set_repo
            .get(&new_fs.to_string())
            .await?
            .map(|fs| fs.name)
            .unwrap_or_else(|| new_fs.to_string());
        let summary = format!(
            "Pin this connection to FeatureSet '{fs_name}' ({} tools)",
            diff.after.len()
        );

        let client_id = *call.client_id;
        let client_repo = call.ctx.client_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        with_approval(
            &call,
            "mcpmux_pin_this_session",
            summary,
            Some(serde_json::to_value(&diff).unwrap_or(Value::Null)),
            false,
            call.args.clone(),
            || async move {
                client_repo
                    .set_pin(&client_id, &space_id, Some(&new_fs))
                    .await?;
                info!(%client_id, new_fs = %new_fs, "[meta_tools] pin_this_session applied");
                // Trigger a list_changed notification so the caller
                // re-fetches the trimmed toolset immediately.
                emit_tools_list_changed(&event_tx, space_id);
                Ok(text_result(json!({
                    "ok": true,
                    "pinned_feature_set_id": new_fs,
                    "tool_count": diff.after.len(),
                })))
            },
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// mcpmux_create_feature_set — write (creates FS, optionally activates)
// ---------------------------------------------------------------------------

pub struct CreateFeatureSetTool;

#[async_trait]
impl MetaTool for CreateFeatureSetTool {
    fn name(&self) -> &'static str {
        "mcpmux_create_feature_set"
    }

    fn description(&self) -> &'static str {
        "Create a new custom FeatureSet in the caller's Space from an explicit \
         list of qualified tool names (e.g. ['github_create_issue', \
         'firebase_deploy']). Returns the new FS id; does NOT activate it — \
         call mcpmux_pin_this_session or mcpmux_set_space_active separately \
         so the user sees the activation dialog distinct from creation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name", "tool_qualified_names"],
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "tool_qualified_names": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let name = call
            .args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `name`".into()))?
            .to_string();
        let description = call
            .args
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let qualified_names: Vec<String> = call
            .args
            .get("tool_qualified_names")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if qualified_names.is_empty() {
            return Err(MetaToolError::InvalidArgument(
                "tool_qualified_names must contain at least one entry".into(),
            ));
        }

        let space_id = caller_space_id(&call).await?;

        // Resolve qualified names → ServerFeature ids up-front so the
        // approval dialog can show the exact tool count.
        let all_features = call
            .ctx
            .server_feature_repo
            .list_for_space(&space_id.to_string())
            .await?;
        let matched: Vec<_> = all_features
            .iter()
            .filter(|f| {
                f.feature_type == FeatureType::Tool && qualified_names.contains(&f.qualified_name())
            })
            .cloned()
            .collect();
        if matched.is_empty() {
            return Err(MetaToolError::InvalidArgument(
                "no provided qualified_names matched any tool in this Space".into(),
            ));
        }

        let summary = format!("Create FeatureSet '{name}' with {} tools", matched.len());
        let diff = json!({
            "added_tools": matched.iter().map(|f| f.qualified_name()).collect::<Vec<_>>(),
        });

        let fs_repo = call.ctx.feature_set_repo.clone();
        let name_for_closure = name.clone();
        let description_for_closure = description.clone();
        with_approval(
            &call,
            "mcpmux_create_feature_set",
            summary,
            Some(diff),
            false,
            call.args.clone(),
            || async move {
                let mut fs =
                    mcpmux_core::FeatureSet::new_custom(&name_for_closure, space_id.to_string());
                fs.description = description_for_closure;
                fs_repo.create(&fs).await?;
                for feature in &matched {
                    fs_repo
                        .add_feature_member(&fs.id, &feature.id.to_string(), MemberMode::Include)
                        .await?;
                }
                info!(fs_id = %fs.id, name = %name_for_closure, "[meta_tools] create_feature_set applied");
                Ok(text_result(json!({
                    "ok": true,
                    "feature_set_id": fs.id,
                    "tool_count": matched.len(),
                })))
            },
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// mcpmux_bind_current_workspace — write (persistent, space-wide effect)
// ---------------------------------------------------------------------------

pub struct BindCurrentWorkspaceTool;

#[async_trait]
impl MetaTool for BindCurrentWorkspaceTool {
    fn name(&self) -> &'static str {
        "mcpmux_bind_current_workspace"
    }

    fn description(&self) -> &'static str {
        "Persistently bind the caller's first reported workspace root to the \
         given FeatureSet. Every future connection in this Space that reports \
         the same root (or a subdirectory) will resolve to this FeatureSet \
         unless they have an explicit pin. Requires user approval and the \
         calling client MUST have declared MCP roots."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["feature_set_id"],
            "properties": {
                "feature_set_id": { "type": "string" }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let fs_id = parse_uuid_arg(&call.args, "feature_set_id")?;

        let space_id = caller_space_id(&call).await?;
        let roots = call
            .session_id
            .and_then(|sid| call.ctx.session_roots.get(sid))
            .unwrap_or_default();
        let root = roots.into_iter().next().ok_or_else(|| {
            MetaToolError::InvalidArgument(
                "caller did not report any MCP roots; cannot bind".into(),
            )
        })?;
        let normalized = normalize_workspace_root(&root);

        let fs_name = call
            .ctx
            .feature_set_repo
            .get(&fs_id.to_string())
            .await?
            .map(|fs| fs.name)
            .unwrap_or_else(|| fs_id.to_string());

        let summary = format!(
            "Bind workspace '{normalized}' in this Space to FeatureSet '{fs_name}'. \
             Affects every future connection that reports this path."
        );

        let binding_repo = call.ctx.binding_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        with_approval(
            &call,
            "mcpmux_bind_current_workspace",
            summary,
            None,
            true,
            call.args.clone(),
            || async move {
                let binding = WorkspaceBinding::new(space_id, normalized.clone(), fs_id);
                binding_repo.create(&binding).await?;
                info!(
                    %space_id,
                    workspace_root = %normalized,
                    feature_set_id = %fs_id,
                    "[meta_tools] bind_current_workspace applied",
                );
                emit_tools_list_changed(&event_tx, space_id);
                Ok(text_result(json!({
                    "ok": true,
                    "binding_id": binding.id,
                    "workspace_root": normalized,
                    "feature_set_id": fs_id,
                })))
            },
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// mcpmux_set_space_active — write (affects every client in the Space)
// ---------------------------------------------------------------------------

pub struct SetSpaceActiveTool;

#[async_trait]
impl MetaTool for SetSpaceActiveTool {
    fn name(&self) -> &'static str {
        "mcpmux_set_space_active"
    }

    fn description(&self) -> &'static str {
        "Change the Space's ACTIVE FeatureSet — the fallback applied to every \
         connected client that has no pin and no matching workspace binding. \
         This affects OTHER clients beyond the caller; use sparingly. Requires \
         user approval."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["feature_set_id"],
            "properties": {
                "feature_set_id": { "type": "string" }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let fs_id = parse_uuid_arg(&call.args, "feature_set_id")?;
        let space_id = caller_space_id(&call).await?;

        let space = call
            .ctx
            .space_repo
            .get(&space_id)
            .await?
            .ok_or_else(|| MetaToolError::Internal("space missing".into()))?;

        let fs_name = call
            .ctx
            .feature_set_repo
            .get(&fs_id.to_string())
            .await?
            .map(|fs| fs.name)
            .unwrap_or_else(|| fs_id.to_string());

        let diff = ToolDiff::compute(
            &call.ctx.feature_service,
            space_id,
            space.active_feature_set_id,
            Some(fs_id),
        )
        .await?;

        let summary = format!(
            "Set the Space's active FeatureSet to '{fs_name}' ({} tools). \
             Affects every connection in this Space that has no pin and no workspace binding.",
            diff.after.len(),
        );

        let space_repo = call.ctx.space_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        with_approval(
            &call,
            "mcpmux_set_space_active",
            summary,
            Some(serde_json::to_value(&diff).unwrap_or(Value::Null)),
            true,
            call.args.clone(),
            || async move {
                space_repo
                    .set_active_feature_set(&space_id, Some(&fs_id))
                    .await?;
                info!(%space_id, feature_set_id = %fs_id, "[meta_tools] set_space_active applied");
                emit_tools_list_changed(&event_tx, space_id);
                Ok(text_result(json!({
                    "ok": true,
                    "space_id": space_id,
                    "active_feature_set_id": fs_id,
                    "tool_count": diff.after.len(),
                })))
            },
        )
        .await
    }
}

// Suppress unused warning — `ApprovalScope` is re-exported for the Tauri
// surface and will land as a command argument once the dialog is wired up.
#[allow(dead_code)]
fn _unused_approval_scope(_: ApprovalScope) {}
