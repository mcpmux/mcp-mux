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

/// Resolve the Space the caller is *actually* routed into — i.e. whichever
/// Space the resolver picks via WorkspaceBinding for this session's reported
/// roots, falling back to the default Space when no binding matches.
///
/// Every meta tool reads (and writes) inside this Space. That keeps the
/// caller's tool/FS view aligned with the tools the gateway actually exposes
/// to them, and prevents an LLM in workspace A from mutating FSes in
/// workspace B just because both sit under the same default-Space-flagged
/// row in the DB.
async fn caller_space_id(call: &MetaToolCall<'_>) -> Result<Uuid, MetaToolError> {
    let resolved = call
        .ctx
        .resolver
        .resolve(call.session_id, Some(call.client_id))
        .await?;
    if let Some(space_id) = resolved.space_id {
        return Ok(space_id);
    }
    // Resolver returned no space — should only happen in the pathological
    // "no default space configured" setup. Fail loudly so callers see why.
    Err(MetaToolError::Internal(
        "no Space resolved for this caller (no default Space configured?)".into(),
    ))
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
        "List every tool installed in the caller's resolved Space, without \
         the current FeatureSet filter applied. Use this to see what the \
         workspace could expose before composing a custom FeatureSet. \
         Returns an array of {server_id, qualified_name, description, available}."
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
        "List every FeatureSet defined in the caller's resolved Space — \
         built-ins and custom. Each entry carries `id`, `name`, `description`, \
         `type`, and `is_builtin`. Use before composing a new FeatureSet so \
         you don't recreate one that already fits."
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
        let sets = call
            .ctx
            .feature_set_repo
            .list_by_space(&space_id.to_string())
            .await?;
        let sets: Vec<_> = sets
            .iter()
            .filter(|fs| !fs.is_deleted)
            .map(|fs| {
                json!({
                    "id": fs.id,
                    "name": fs.name,
                    "description": fs.description,
                    "type": fs.feature_set_type,
                    "is_builtin": fs.is_builtin,
                })
            })
            .collect();
        Ok(text_result(
            json!({ "space_id": space.id, "feature_sets": sets }),
        ))
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
        .request_approval(call.client_id, tool_name, payload)
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
// mcpmux_create_feature_set — write (creates FS, optionally activates)
// ---------------------------------------------------------------------------

pub struct CreateFeatureSetTool;

#[async_trait]
impl MetaTool for CreateFeatureSetTool {
    fn name(&self) -> &'static str {
        "mcpmux_create_feature_set"
    }

    fn description(&self) -> &'static str {
        "Create a new custom FeatureSet in the caller's resolved Space from \
         an explicit list of qualified tool names (e.g. ['github_create_issue', \
         'firebase_deploy']). Returns the new FS id. To make a workspace \
         actually route through this FeatureSet, follow up with \
         `mcpmux_bind_current_workspace`."
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
            .trim()
            .to_string();
        if name.is_empty() {
            return Err(MetaToolError::InvalidArgument(
                "`name` must not be empty or whitespace".into(),
            ));
        }
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
         given FeatureSet inside the caller's resolved Space. Only a future \
         connection that reports this EXACT root resolves to this FeatureSet — \
         bindings are exact-match, with no subdirectory/ancestor inheritance. \
         Requires user approval and the calling client MUST have declared MCP \
         roots."
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

        // The FeatureSet MUST exist and belong to the caller's resolved Space.
        // Binding a cross-Space FS persists inconsistent routing state from
        // LLM-supplied input: it would later resolve `get_tools_for_grants`
        // against the wrong Space and silently yield an empty tool set.
        // A FS with no space_id is legacy/global and accepted in any Space.
        let fs = call
            .ctx
            .feature_set_repo
            .get(&fs_id.to_string())
            .await?
            .ok_or_else(|| {
                MetaToolError::InvalidArgument(format!("FeatureSet '{fs_id}' does not exist"))
            })?;
        if let Some(fs_space) = fs.space_id.as_deref() {
            if fs_space != space_id.to_string() {
                return Err(MetaToolError::InvalidArgument(format!(
                    "FeatureSet '{fs_id}' belongs to a different Space and cannot be bound here"
                )));
            }
        }
        let fs_name = fs.name;

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
                let binding =
                    WorkspaceBinding::new(normalized.clone(), space_id, fs_id.to_string());
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

// Suppress unused warning — `ApprovalScope` is re-exported for the Tauri
// surface and will land as a command argument once the dialog is wired up.
#[allow(dead_code)]
fn _unused_approval_scope(_: ApprovalScope) {}
