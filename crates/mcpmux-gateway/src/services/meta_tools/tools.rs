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
use std::collections::{HashMap, HashSet};
use tokio::sync::broadcast;
use tracing::info;
use uuid::Uuid;

use super::approval::{ApprovalPayload, ApprovalScope};
use super::registry::{
    MetaTool, MetaToolCall, MetaToolError, SESSION_OVERRIDES_REQUIRE_APPROVAL_KEY,
};
use crate::services::ResolvedFeatureSet;

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

pub(crate) fn text_result(v: Value) -> CallToolResult {
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

/// Full resolver output for the caller — space + binding FS ids + source.
async fn caller_resolution(call: &MetaToolCall<'_>) -> Result<ResolvedFeatureSet, MetaToolError> {
    call.ctx
        .resolver
        .resolve(call.session_id, Some(call.client_id))
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))
}

/// Derive the manifest status for one server in the caller's session.
fn derive_server_status(
    server_id: &str,
    binding_servers: &HashSet<String>,
    session_enabled: &HashSet<String>,
    session_disabled: &HashSet<String>,
) -> &'static str {
    if session_disabled.contains(server_id) {
        "disabled_via_session"
    } else if session_enabled.contains(server_id) && !binding_servers.contains(server_id) {
        "enabled_via_session"
    } else if binding_servers.contains(server_id) {
        "enabled_via_binding"
    } else {
        "inactive"
    }
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
// mcpmux_list_servers — read
// ---------------------------------------------------------------------------

pub struct ListServersTool;

#[async_trait]
impl MetaTool for ListServersTool {
    fn name(&self) -> &'static str {
        "mcpmux_list_servers"
    }

    fn description(&self) -> &'static str {
        "List every MCP server installed in the caller's resolved Space with \
         a coarse status per server: enabled_via_binding, enabled_via_session, \
         disabled_via_session, or inactive. Clone installs include optional \
         `cloned_from` (source server_id). Use before enable/disable to see \
         current routing state without loading every tool."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let resolved = caller_resolution(&call).await?;
        let space_id = resolved
            .space_id
            .ok_or_else(|| MetaToolError::Internal("space missing".into()))?;

        let binding_features = call
            .ctx
            .feature_service
            .resolve_feature_sets(&space_id.to_string(), &resolved.feature_set_ids)
            .await?;
        let binding_servers: HashSet<String> = binding_features
            .iter()
            .map(|f| f.server_id.clone())
            .collect();

        let session_enabled = call
            .session_id
            .map(|sid| call.ctx.session_overrides.enabled_set(sid))
            .unwrap_or_default();
        let session_disabled = call
            .session_id
            .map(|sid| call.ctx.session_overrides.disabled_set(sid))
            .unwrap_or_default();

        let features = call
            .ctx
            .server_feature_repo
            .list_for_space(&space_id.to_string())
            .await?;

        let installed = call
            .ctx
            .installed_server_repo
            .list_for_space(&space_id.to_string())
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;
        let cloned_from_by_server: HashMap<String, Option<String>> = installed
            .into_iter()
            .map(|s| (s.server_id, s.cloned_from))
            .collect();

        let mut by_server: HashMap<String, (Option<String>, usize)> = HashMap::new();
        for feature in &features {
            if feature.feature_type != FeatureType::Tool {
                continue;
            }
            let entry = by_server
                .entry(feature.server_id.clone())
                .or_insert((None, 0));
            if entry.0.is_none() {
                entry.0 = feature.display_name.clone();
            }
            entry.1 += 1;
        }

        let mut servers: Vec<Value> = by_server
            .into_iter()
            .map(|(id, (display_name, tool_count))| {
                let name = display_name.unwrap_or_else(|| id.clone());
                let status = derive_server_status(
                    &id,
                    &binding_servers,
                    &session_enabled,
                    &session_disabled,
                );
                let mut entry = json!({
                    "id": id,
                    "name": name,
                    "tool_count": tool_count,
                    "status": status,
                });
                if let Some(cloned_from) = cloned_from_by_server.get(&id).and_then(|v| v.as_ref()) {
                    entry["cloned_from"] = json!(cloned_from);
                }
                entry
            })
            .collect();
        servers.sort_by(|a, b| {
            a.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
        });

        Ok(text_result(json!({ "servers": servers })))
    }
}

// ---------------------------------------------------------------------------
// Writes — each goes through the ApprovalBroker before mutating state.
// ---------------------------------------------------------------------------

/// Common path for every write tool: build payload, ask broker, run the
/// mutation. Returns the broker's decision so the caller can proceed only
/// on success. `mutate` is the thing that runs post-approval and is
/// expected to emit `tools/list_changed` when relevant.
pub(crate) async fn with_approval<F, Fut, T>(
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

fn parse_string_arg(args: &Value, field: &str) -> Result<String, MetaToolError> {
    args.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| MetaToolError::InvalidArgument(format!("missing `{field}`")))
}

/// Parse `scope` for enable/disable server tools.
fn parse_scope(args: &Value) -> Result<&'static str, MetaToolError> {
    match args.get("scope").and_then(|v| v.as_str()) {
        None | Some("session") => Ok("session"),
        Some("workspace") => Ok("workspace"),
        Some(other) => Err(MetaToolError::InvalidArgument(format!(
            "invalid scope '{other}'; expected 'session' or 'workspace'"
        ))),
    }
}

/// Whether session-scope server overrides require desktop approval.
async fn session_overrides_require_approval(ctx: &super::registry::MetaToolContext) -> bool {
    let Some(repo) = ctx.settings_repo.as_ref() else {
        return false;
    };
    match repo.get(SESSION_OVERRIDES_REQUIRE_APPROVAL_KEY).await {
        Ok(Some(v)) => matches!(v.as_str(), "true" | "1"),
        _ => false,
    }
}

/// Ensure `server_id` has at least one feature row in the caller's Space.
async fn validate_server_in_space(
    call: &MetaToolCall<'_>,
    space_id: Uuid,
    server_id: &str,
) -> Result<(), MetaToolError> {
    let features = call
        .ctx
        .server_feature_repo
        .list_for_space(&space_id.to_string())
        .await?;
    if features.iter().any(|f| f.server_id == server_id) {
        return Ok(());
    }
    Err(MetaToolError::InvalidArgument(format!(
        "unknown server_id '{server_id}' in this Space"
    )))
}

fn require_session_id(call: &MetaToolCall<'_>) -> Result<String, MetaToolError> {
    call.session_id.map(|s| s.to_string()).ok_or_else(|| {
        MetaToolError::InvalidArgument("session scope requires an MCP session id".into())
    })
}

// ---------------------------------------------------------------------------
// mcpmux_enable_server / mcpmux_disable_server — write (session scope)
// ---------------------------------------------------------------------------

pub struct EnableServerTool;

#[async_trait]
impl MetaTool for EnableServerTool {
    fn name(&self) -> &'static str {
        "mcpmux_enable_server"
    }

    fn description(&self) -> &'static str {
        "Enable an MCP server. Default scope is session (ephemeral). Use \
         scope: \"workspace\" to persist on the matched workspace binding \
         (requires approval). Use mcpmux_list_servers first."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["server_id"],
            "properties": {
                "server_id": { "type": "string" },
                "scope": {
                    "type": "string",
                    "enum": ["session", "workspace"],
                    "default": "session"
                }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let scope = parse_scope(&call.args)?;
        let server_id = parse_string_arg(&call.args, "server_id")?;
        let space_id = caller_space_id(&call).await?;
        validate_server_in_space(&call, space_id, &server_id).await?;

        if scope == "workspace" {
            return super::workspace_server::enable_workspace_server(call, space_id, server_id)
                .await;
        }

        let session_id = require_session_id(&call)?;

        if session_overrides_require_approval(call.ctx).await {
            let overrides = call.ctx.session_overrides.clone();
            let server_id_for_closure = server_id.clone();
            let session_id_owned = session_id.clone();
            let summary = format!("Enable server '{server_id}' for this session");
            return with_approval(
                &call,
                "mcpmux_enable_server",
                summary,
                None,
                false,
                call.args.clone(),
                || async move {
                    overrides.enable(&session_id_owned, &server_id_for_closure);
                    info!(
                        session_id = %session_id_owned,
                        server_id = %server_id_for_closure,
                        "[meta_tools] enable_server applied (approved)"
                    );
                    Ok(text_result(json!({
                        "ok": true,
                        "server_id": server_id_for_closure,
                        "scope": "session",
                    })))
                },
            )
            .await;
        }

        call.ctx.session_overrides.enable(&session_id, &server_id);
        if let Ok(mut decision) = call.audit_decision.lock() {
            *decision = Some("session_override");
        }
        info!(
            %session_id,
            server_id = %server_id,
            "[meta_tools] enable_server applied"
        );
        Ok(text_result(json!({
            "ok": true,
            "server_id": server_id,
            "scope": "session",
        })))
    }
}

pub struct DisableServerTool;

#[async_trait]
impl MetaTool for DisableServerTool {
    fn name(&self) -> &'static str {
        "mcpmux_disable_server"
    }

    fn description(&self) -> &'static str {
        "Disable an MCP server. Default scope is session (ephemeral). Use \
         scope: \"workspace\" to remove the server-all layer from the \
         workspace binding (requires approval; custom FeatureSets must be \
         edited in the Workspaces UI)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["server_id"],
            "properties": {
                "server_id": { "type": "string" },
                "scope": {
                    "type": "string",
                    "enum": ["session", "workspace"],
                    "default": "session"
                }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let scope = parse_scope(&call.args)?;
        let server_id = parse_string_arg(&call.args, "server_id")?;
        let space_id = caller_space_id(&call).await?;
        validate_server_in_space(&call, space_id, &server_id).await?;

        if scope == "workspace" {
            return super::workspace_server::disable_workspace_server(call, space_id, server_id)
                .await;
        }

        let session_id = require_session_id(&call)?;

        if session_overrides_require_approval(call.ctx).await {
            let overrides = call.ctx.session_overrides.clone();
            let server_id_for_closure = server_id.clone();
            let session_id_owned = session_id.clone();
            let summary = format!("Disable server '{server_id}' for this session");
            return with_approval(
                &call,
                "mcpmux_disable_server",
                summary,
                None,
                false,
                call.args.clone(),
                || async move {
                    overrides.disable(&session_id_owned, &server_id_for_closure);
                    info!(
                        session_id = %session_id_owned,
                        server_id = %server_id_for_closure,
                        "[meta_tools] disable_server applied (approved)"
                    );
                    Ok(text_result(json!({
                        "ok": true,
                        "server_id": server_id_for_closure,
                        "scope": "session",
                    })))
                },
            )
            .await;
        }

        call.ctx.session_overrides.disable(&session_id, &server_id);
        if let Ok(mut decision) = call.audit_decision.lock() {
            *decision = Some("session_override");
        }
        info!(
            %session_id,
            server_id = %server_id,
            "[meta_tools] disable_server applied"
        );
        Ok(text_result(json!({
            "ok": true,
            "server_id": server_id,
            "scope": "session",
        })))
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
         given FeatureSet inside the caller's resolved Space. Every future \
         connection that reports the same root (or a subdirectory) will \
         resolve to this FeatureSet. Requires user approval and the calling \
         client MUST have declared MCP roots."
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
