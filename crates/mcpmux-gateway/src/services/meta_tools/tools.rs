//! Built-in `mcpmux_*` meta tool implementations.
//!
//! Each tool is a unit struct implementing [`MetaTool`]. Reads execute
//! directly; writes route through the [`ApprovalBroker`] first.

use async_trait::async_trait;
use mcpmux_core::{
    normalize_workspace_root, DomainEvent, FeatureType, MemberMode, ServerFeature, WorkspaceBinding,
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

/// Resolve the Space an operation targets: an explicit `space_id` arg when the
/// caller names one (validated to exist), otherwise the caller's resolved
/// Space. This is what lets a client manage ANY Space it can discover via
/// `mcpmux_list_spaces` — writes stay gated by the approval dialog, which names
/// the target Space so cross-Space changes are a conscious user choice.
async fn target_space_id(call: &MetaToolCall<'_>) -> Result<Uuid, MetaToolError> {
    match opt_str_arg(&call.args, "space_id") {
        Some(s) => {
            let id = Uuid::parse_str(&s).map_err(|_| {
                MetaToolError::InvalidArgument(format!("`space_id` is not a UUID: {s}"))
            })?;
            call.ctx.space_repo.get(&id).await?.ok_or_else(|| {
                MetaToolError::InvalidArgument(format!("Space '{id}' does not exist"))
            })?;
            Ok(id)
        }
        None => caller_space_id(call).await,
    }
}

/// Human-readable Space name for approval summaries; falls back to the id.
async fn space_label(call: &MetaToolCall<'_>, space_id: Uuid) -> String {
    match call.ctx.space_repo.get(&space_id).await {
        Ok(Some(space)) => space.name,
        _ => space_id.to_string(),
    }
}

/// The optional `space_id` input-schema property shared by every meta tool.
/// Omitted ⇒ the tool targets the caller's resolved Space (back-compatible).
fn space_id_schema_prop() -> Value {
    json!({
        "type": "string",
        "description": "Target Space id (from mcpmux_list_spaces). Omit to use the current workspace's resolved Space."
    })
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
        "List every tool installed in a Space (default: the caller's resolved \
         Space; pass `space_id` to target another), without the current \
         FeatureSet filter applied. Prefer `mcpmux_search_tools` unless you need \
         the full list — this dump is token-heavy. Returns an array of \
         {server_id, qualified_name, description, available}."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "space_id": space_id_schema_prop() }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = target_space_id(&call).await?;
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
        Ok(text_result(json!({ "space_id": space_id, "tools": tools })))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_search_tools — read
// ---------------------------------------------------------------------------

pub struct SearchToolsTool;

/// Default cap on returned matches — keeps the payload (and the agent's token
/// spend) bounded when a broad query matches many tools.
const SEARCH_TOOLS_DEFAULT_LIMIT: usize = 25;
/// Hard ceiling so a caller can't request an unbounded dump via `limit`.
const SEARCH_TOOLS_MAX_LIMIT: usize = 100;

#[async_trait]
impl MetaTool for SearchToolsTool {
    fn name(&self) -> &'static str {
        "mcpmux_search_tools"
    }

    fn description(&self) -> &'static str {
        "Search the tools installed in a Space by keyword (default: the caller's \
         resolved Space; pass `space_id` to target another), without the current \
         FeatureSet filter applied. Prefer this over `mcpmux_list_all_tools` when \
         you know roughly what you're looking for — it returns only matches, so \
         it's far cheaper than dumping the whole catalog. `query` is matched \
         case-insensitively against each tool's qualified name, description, and \
         server id. Optional `limit` (default 25, max 100). Returns an array of \
         {server_id, qualified_name, description, available}."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "keyword(s) matched against tool name, description, and server id"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": SEARCH_TOOLS_MAX_LIMIT,
                    "description": "max matches to return (default 25)"
                },
                "space_id": space_id_schema_prop()
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let query = opt_str_arg(&call.args, "query").ok_or_else(|| {
            MetaToolError::InvalidArgument("search requires a non-empty `query`".into())
        })?;
        let needle = query.to_lowercase();

        // `limit`: clamp to [1, MAX]; fall back to the default when absent or
        // not a positive integer.
        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| (n as usize).clamp(1, SEARCH_TOOLS_MAX_LIMIT))
            .unwrap_or(SEARCH_TOOLS_DEFAULT_LIMIT);

        let space_id = target_space_id(&call).await?;
        let features = call
            .ctx
            .server_feature_repo
            .list_for_space(&space_id.to_string())
            .await?;

        let mut matches: Vec<&ServerFeature> = features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Tool)
            .filter(|f| {
                f.qualified_name().to_lowercase().contains(&needle)
                    || f.server_id.to_lowercase().contains(&needle)
                    || f.description
                        .as_deref()
                        .map(|d| d.to_lowercase().contains(&needle))
                        .unwrap_or(false)
            })
            .collect();

        // Stable, predictable ordering before truncating to `limit`.
        matches.sort_by_key(|f| f.qualified_name());
        let total = matches.len();
        let truncated = total > limit;

        let tools: Vec<_> = matches
            .into_iter()
            .take(limit)
            .map(|f| {
                json!({
                    "server_id": f.server_id,
                    "qualified_name": f.qualified_name(),
                    "description": f.description,
                    "available": f.is_available,
                })
            })
            .collect();

        Ok(text_result(json!({
            "space_id": space_id,
            "query": query,
            "match_count": total,
            "returned": tools.len(),
            "truncated": truncated,
            "tools": tools,
        })))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_list_spaces — read
// ---------------------------------------------------------------------------

pub struct ListSpacesTool;

#[async_trait]
impl MetaTool for ListSpacesTool {
    fn name(&self) -> &'static str {
        "mcpmux_list_spaces"
    }

    fn description(&self) -> &'static str {
        "List every Space McpMux knows about. Returns an array of \
         {id, name, is_default, description}. Use a returned `id` as the \
         `space_id` argument to other tools to inspect or configure a specific \
         Space (e.g. compose a FeatureSet in one Space and bind the current \
         workspace to it). The Space marked `is_default` is the fallback when a \
         workspace has no binding."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let spaces = call.ctx.space_repo.list().await?;
        let spaces: Vec<_> = spaces
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "name": s.name,
                    "is_default": s.is_default,
                    "description": s.description,
                })
            })
            .collect();
        Ok(text_result(json!({ "spaces": spaces })))
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
        "List every FeatureSet defined in a Space (default: the caller's resolved \
         Space; pass `space_id` to target another) — built-ins and custom. Each \
         entry carries `id`, `name`, `description`, `type`, and `is_builtin`. Use \
         before composing a new FeatureSet so you don't recreate one that already \
         fits."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "space_id": space_id_schema_prop() }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = target_space_id(&call).await?;
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
#[allow(clippy::too_many_arguments)]
async fn with_approval<F, Fut, T>(
    call: &MetaToolCall<'_>,
    tool_name: &'static str,
    summary: String,
    space_name: Option<String>,
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
        space_name,
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

/// Trimmed non-empty string arg, or `None` (treats whitespace as absent).
fn opt_str_arg(args: &Value, field: &str) -> Option<String> {
    args.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// String-array arg (e.g. a list of qualified tool names); empty when absent.
fn str_array_arg(args: &Value, field: &str) -> Vec<String> {
    args.get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Resolve qualified tool names to their `ServerFeature`s within a Space.
/// Returns `(matched, unmatched)` so callers can fail with an actionable
/// message instead of silently dropping names the agent got wrong.
async fn resolve_tool_features(
    call: &MetaToolCall<'_>,
    space_id: Uuid,
    names: &[String],
) -> Result<(Vec<ServerFeature>, Vec<String>), MetaToolError> {
    let all = call
        .ctx
        .server_feature_repo
        .list_for_space(&space_id.to_string())
        .await?;
    let matched: Vec<ServerFeature> = all
        .into_iter()
        .filter(|f| f.feature_type == FeatureType::Tool && names.contains(&f.qualified_name()))
        .collect();
    let matched_names: Vec<String> = matched.iter().map(|f| f.qualified_name()).collect();
    let unmatched: Vec<String> = names
        .iter()
        .filter(|n| !matched_names.contains(n))
        .cloned()
        .collect();
    Ok((matched, unmatched))
}

/// Guard: a FeatureSet targeted by update/delete must belong to the caller's
/// resolved Space (or be legacy/global with no `space_id`) and must be custom.
/// Built-in sets (the auto-seeded Starter) are not mutable via MCP.
fn ensure_custom_in_space(
    fs: &mcpmux_core::FeatureSet,
    space_id: Uuid,
    fs_id: Uuid,
) -> Result<(), MetaToolError> {
    if let Some(fs_space) = fs.space_id.as_deref() {
        if fs_space != space_id.to_string() {
            return Err(MetaToolError::InvalidArgument(format!(
                "FeatureSet '{fs_id}' belongs to a different Space"
            )));
        }
    }
    if fs.is_builtin {
        return Err(MetaToolError::InvalidArgument(format!(
            "FeatureSet '{fs_id}' is built-in and can't be modified or deleted via MCP"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// mcpmux_manage_feature_set — write (create / update / delete a custom FS)
// ---------------------------------------------------------------------------

pub struct ManageFeatureSetTool;

impl ManageFeatureSetTool {
    async fn create(
        &self,
        call: &MetaToolCall<'_>,
        space_id: Uuid,
    ) -> Result<CallToolResult, MetaToolError> {
        let name = opt_str_arg(&call.args, "name").ok_or_else(|| {
            MetaToolError::InvalidArgument("create requires a non-empty `name`".into())
        })?;
        let description = opt_str_arg(&call.args, "description");
        let add = str_array_arg(&call.args, "add");
        if add.is_empty() {
            return Err(MetaToolError::InvalidArgument(
                "create requires `add` with at least one qualified tool name".into(),
            ));
        }
        let (matched, unmatched) = resolve_tool_features(call, space_id, &add).await?;
        if !unmatched.is_empty() {
            return Err(MetaToolError::InvalidArgument(format!(
                "unknown tool name(s): {}",
                unmatched.join(", ")
            )));
        }

        let space = space_label(call, space_id).await;
        let summary = format!(
            "Create FeatureSet '{name}' in Space '{space}' with {} tool(s)",
            matched.len()
        );
        let diff = json!({
            "added": matched.iter().map(|f| f.qualified_name()).collect::<Vec<_>>(),
        });

        let fs_repo = call.ctx.feature_set_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        let name_c = name.clone();
        with_approval(
            call,
            "mcpmux_manage_feature_set",
            summary,
            Some(space),
            Some(diff),
            false,
            call.args.clone(),
            || async move {
                let mut fs = mcpmux_core::FeatureSet::new_custom(&name_c, space_id.to_string());
                fs.description = description;
                fs_repo.create(&fs).await?;
                for feature in &matched {
                    fs_repo
                        .add_feature_member(&fs.id, &feature.id.to_string(), MemberMode::Include)
                        .await?;
                }
                emit_tools_list_changed(&event_tx, space_id);
                info!(fs_id = %fs.id, name = %name_c, "[meta_tools] manage_feature_set create applied");
                Ok(text_result(json!({
                    "ok": true,
                    "action": "create",
                    "feature_set_id": fs.id,
                    "tool_count": matched.len(),
                })))
            },
        )
        .await
    }

    async fn update(
        &self,
        call: &MetaToolCall<'_>,
        space_id: Uuid,
    ) -> Result<CallToolResult, MetaToolError> {
        let fs_id = parse_uuid_arg(&call.args, "feature_set_id")?;
        let fs = call
            .ctx
            .feature_set_repo
            .get_with_members(&fs_id.to_string())
            .await?
            .ok_or_else(|| {
                MetaToolError::InvalidArgument(format!("FeatureSet '{fs_id}' does not exist"))
            })?;
        ensure_custom_in_space(&fs, space_id, fs_id)?;

        let new_name = opt_str_arg(&call.args, "name");
        let new_description = opt_str_arg(&call.args, "description");
        let add = str_array_arg(&call.args, "add");
        let remove = str_array_arg(&call.args, "remove");
        if new_name.is_none() && new_description.is_none() && add.is_empty() && remove.is_empty() {
            return Err(MetaToolError::InvalidArgument(
                "update requires at least one of `name`, `description`, `add`, `remove`".into(),
            ));
        }

        let (add_features, add_unmatched) = resolve_tool_features(call, space_id, &add).await?;
        if !add_unmatched.is_empty() {
            return Err(MetaToolError::InvalidArgument(format!(
                "unknown tool name(s) in `add`: {}",
                add_unmatched.join(", ")
            )));
        }
        // Removes that don't resolve to a tool are simply no-ops (the tool
        // may already be absent) — don't fail the whole update on them.
        let (remove_features, _unmatched_removes) =
            resolve_tool_features(call, space_id, &remove).await?;

        let rename_suffix = new_name
            .as_deref()
            .map(|n| format!(", rename → '{n}'"))
            .unwrap_or_default();
        let space = space_label(call, space_id).await;
        let summary = format!(
            "Update FeatureSet '{}' in Space '{space}': +{} / -{}{}",
            fs.name,
            add_features.len(),
            remove_features.len(),
            rename_suffix
        );
        let diff = json!({
            "added": add_features.iter().map(|f| f.qualified_name()).collect::<Vec<_>>(),
            "removed": remove_features.iter().map(|f| f.qualified_name()).collect::<Vec<_>>(),
        });

        let fs_repo = call.ctx.feature_set_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        let fs_id_s = fs_id.to_string();
        with_approval(
            call,
            "mcpmux_manage_feature_set",
            summary,
            Some(space),
            Some(diff),
            true,
            call.args.clone(),
            || async move {
                // Rename / description first — `update` rewrites the row from
                // `fs.members` (the set we loaded, unchanged here), so the
                // member deltas below still land on top.
                if new_name.is_some() || new_description.is_some() {
                    let mut updated = fs.clone();
                    if let Some(n) = new_name {
                        updated.name = n;
                    }
                    if let Some(d) = new_description {
                        updated.description = Some(d);
                    }
                    fs_repo.update(&updated).await?;
                }
                for feature in &remove_features {
                    fs_repo
                        .remove_feature_member(&fs_id_s, &feature.id.to_string())
                        .await?;
                }
                for feature in &add_features {
                    fs_repo
                        .add_feature_member(&fs_id_s, &feature.id.to_string(), MemberMode::Include)
                        .await?;
                }
                emit_tools_list_changed(&event_tx, space_id);
                info!(fs_id = %fs_id_s, "[meta_tools] manage_feature_set update applied");
                Ok(text_result(json!({
                    "ok": true,
                    "action": "update",
                    "feature_set_id": fs_id,
                    "added": add_features.len(),
                    "removed": remove_features.len(),
                })))
            },
        )
        .await
    }

    async fn delete(
        &self,
        call: &MetaToolCall<'_>,
        space_id: Uuid,
    ) -> Result<CallToolResult, MetaToolError> {
        let fs_id = parse_uuid_arg(&call.args, "feature_set_id")?;
        let fs = call
            .ctx
            .feature_set_repo
            .get(&fs_id.to_string())
            .await?
            .ok_or_else(|| {
                MetaToolError::InvalidArgument(format!("FeatureSet '{fs_id}' does not exist"))
            })?;
        ensure_custom_in_space(&fs, space_id, fs_id)?;

        let space = space_label(call, space_id).await;
        let summary = format!("Delete FeatureSet '{}' in Space '{space}'", fs.name);
        let fs_repo = call.ctx.feature_set_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        let fs_id_s = fs_id.to_string();
        with_approval(
            call,
            "mcpmux_manage_feature_set",
            summary,
            Some(space),
            None,
            true,
            call.args.clone(),
            || async move {
                fs_repo.delete(&fs_id_s).await?;
                emit_tools_list_changed(&event_tx, space_id);
                info!(fs_id = %fs_id_s, "[meta_tools] manage_feature_set delete applied");
                Ok(text_result(json!({
                    "ok": true,
                    "action": "delete",
                    "feature_set_id": fs_id,
                })))
            },
        )
        .await
    }
}

#[async_trait]
impl MetaTool for ManageFeatureSetTool {
    fn name(&self) -> &'static str {
        "mcpmux_manage_feature_set"
    }

    fn description(&self) -> &'static str {
        "Create, update, or delete a custom FeatureSet (a named tool bundle) in a \
         Space (default: the caller's resolved Space; pass `space_id` from \
         `mcpmux_list_spaces` to target another). `action`: 'create' (needs \
         `name` + `add` qualified tool names), 'update' (needs `feature_set_id`; \
         pass any of `name` / `description` / `add` / `remove`), or 'delete' \
         (needs `feature_set_id`). Tool names are the qualified names from \
         `mcpmux_list_all_tools`/`mcpmux_search_tools`. Built-in sets can't be \
         modified. Requires user approval. Route a workspace through a FeatureSet \
         with `mcpmux_bind_current_workspace`. Prefer a small initial set you \
         expand later over adding everything upfront."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": { "type": "string", "enum": ["create", "update", "delete"] },
                "name": {
                    "type": "string",
                    "description": "FeatureSet name — required for create, optional rename on update"
                },
                "description": { "type": "string" },
                "feature_set_id": {
                    "type": "string",
                    "description": "required for update and delete"
                },
                "add": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "qualified tool names to add (create uses this as the initial set)"
                },
                "remove": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "qualified tool names to remove (update only)"
                },
                "space_id": space_id_schema_prop()
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let action = call
            .args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let space_id = target_space_id(&call).await?;
        match action.as_str() {
            "create" => self.create(&call, space_id).await,
            "update" => self.update(&call, space_id).await,
            "delete" => self.delete(&call, space_id).await,
            "" => Err(MetaToolError::InvalidArgument(
                "`action` is required (create | update | delete)".into(),
            )),
            other => Err(MetaToolError::InvalidArgument(format!(
                "unknown action '{other}' (expected create | update | delete)"
            ))),
        }
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
        "Route the caller's current workspace (its first reported MCP root) to a \
         FeatureSet in a Space — by default the caller's resolved Space, or pass \
         `space_id` (from `mcpmux_list_spaces`) to route this workspace into a \
         different Space entirely. Idempotent: calling it again for the same \
         workspace REBINDS it (no separate unbind). Omit `feature_set_id` to bind \
         the workspace to NO Space tools (built-ins still apply). Matching is \
         exact — only a future connection reporting this EXACT root resolves \
         here, with no subdirectory/ancestor inheritance. Requires user approval \
         and a client that declared MCP roots."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "feature_set_id": {
                    "type": "string",
                    "description": "FeatureSet to route this workspace to (must live in the target Space); omit for no Space tools. Re-binding the same workspace replaces the previous mapping."
                },
                "space_id": space_id_schema_prop()
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = target_space_id(&call).await?;
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

        // FeatureSet is optional — omitted/empty means "no Space tools here".
        // When given, it MUST exist and belong to the *target* Space (a binding
        // whose FS lives in another Space would resolve against the wrong Space
        // and silently yield an empty tool set). Legacy/global (no space_id)
        // FSes are accepted in any Space.
        let (fs_ids, fs_label) = match opt_str_arg(&call.args, "feature_set_id") {
            Some(s) => {
                let fs_id = Uuid::parse_str(&s).map_err(|_| {
                    MetaToolError::InvalidArgument(format!("`feature_set_id` is not a UUID: {s}"))
                })?;
                let fs = call
                    .ctx
                    .feature_set_repo
                    .get(&fs_id.to_string())
                    .await?
                    .ok_or_else(|| {
                        MetaToolError::InvalidArgument(format!(
                            "FeatureSet '{fs_id}' does not exist"
                        ))
                    })?;
                if let Some(fs_space) = fs.space_id.as_deref() {
                    if fs_space != space_id.to_string() {
                        return Err(MetaToolError::InvalidArgument(format!(
                            "FeatureSet '{fs_id}' is not in the target Space — bind it within its own Space, or pass that Space's `space_id`"
                        )));
                    }
                }
                (vec![fs_id.to_string()], fs.name)
            }
            None => (Vec::new(), "(no Space tools)".to_string()),
        };

        // Upsert: rebind if a binding for this exact root already exists.
        let existing = call
            .ctx
            .binding_repo
            .find_exact_for_roots(std::slice::from_ref(&normalized))
            .await?;
        let verb = if existing.is_some() { "Rebind" } else { "Bind" };
        let space = space_label(&call, space_id).await;
        let summary = format!(
            "{verb} workspace '{normalized}' to FeatureSet '{fs_label}' in Space '{space}'. \
             Affects every future connection that reports this path."
        );

        let binding_repo = call.ctx.binding_repo.clone();
        let event_tx = call.ctx.domain_event_tx.clone();
        with_approval(
            &call,
            "mcpmux_bind_current_workspace",
            summary,
            Some(space),
            None,
            true,
            call.args.clone(),
            || async move {
                let binding_id = match existing {
                    Some(mut b) => {
                        b.feature_set_ids = fs_ids.clone();
                        b.space_id = space_id;
                        binding_repo.update(&b).await?;
                        b.id
                    }
                    None => {
                        let binding = WorkspaceBinding::new_multi(
                            normalized.clone(),
                            space_id,
                            fs_ids.clone(),
                        );
                        binding_repo.create(&binding).await?;
                        binding.id
                    }
                };
                info!(
                    %space_id,
                    workspace_root = %normalized,
                    feature_set_ids = ?fs_ids,
                    "[meta_tools] bind_current_workspace applied",
                );
                // A binding change isn't a FeatureSet-membership change — emit
                // the binding-specific event. It both drives MCPNotifier's
                // list_changed push to peers AND is the event the desktop
                // Workspaces tab refreshes on (`workspace-binding-changed`).
                // Using FeatureSetMembersChanged here left that tab stale.
                let _ = event_tx.send(DomainEvent::WorkspaceBindingChanged {
                    space_id,
                    workspace_root: normalized.clone(),
                });
                Ok(text_result(json!({
                    "ok": true,
                    "binding_id": binding_id,
                    "workspace_root": normalized,
                    "feature_set_ids": fs_ids,
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
