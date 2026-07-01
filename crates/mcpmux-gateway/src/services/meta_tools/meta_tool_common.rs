//! Shared helpers for meta-tool implementations — caller resolution, readiness,
//! structured invoke denial, write approval, and domain-event emission.

use std::collections::{HashMap, HashSet};

use rmcp::model::{CallToolResult, Content};
use serde_json::Value;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::approval::ApprovalPayload;
use super::diagnose_server::{classify_health, parse_missing_required_inputs, ServerHealth};
use super::registry::{MetaToolCall, MetaToolError};
use crate::pool::{
    format_server_bound_offline_error, format_server_inactive_error, ConnectionStatus,
};
use crate::services::ResolvedFeatureSet;
use mcpmux_core::DomainEvent;

/// Fire a `FeatureSetMembersChanged` event so MCPNotifier pushes a
/// `tools/list_changed` notification to every connected client in the Space.
/// Used by every write tool after a successful mutation.
pub(crate) fn emit_tools_list_changed(event_tx: &broadcast::Sender<DomainEvent>, space_id: Uuid) {
    let _ = event_tx.send(DomainEvent::FeatureSetMembersChanged {
        space_id,
        feature_set_id: "meta-tool-write".into(),
        added_count: 0,
        removed_count: 0,
    });
}

/// Notify listeners that a workspace binding row changed.
pub(crate) fn emit_workspace_binding_changed(
    event_tx: &broadcast::Sender<DomainEvent>,
    space_id: Uuid,
    workspace_root: &str,
) {
    let _ = event_tx.send(DomainEvent::WorkspaceBindingChanged {
        space_id,
        workspace_root: workspace_root.to_string(),
    });
}

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
pub(crate) async fn caller_space_id(call: &MetaToolCall<'_>) -> Result<Uuid, MetaToolError> {
    let resolved = call
        .ctx
        .resolver
        .resolve(call.session_id, Some(call.client_id), call.request_machine_id)
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
pub(crate) async fn caller_resolution(
    call: &MetaToolCall<'_>,
) -> Result<ResolvedFeatureSet, MetaToolError> {
    call.ctx
        .resolver
        .resolve(call.session_id, Some(call.client_id), call.request_machine_id)
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))
}

/// Map a health bucket to the `blocking_reason` string for bound-but-not-ready servers.
fn blocking_reason_from_health(health: ServerHealth) -> Option<&'static str> {
    match health {
        ServerHealth::Healthy => None,
        ServerHealth::AuthRequired => Some("auth_required"),
        ServerHealth::NeedsSetup => Some("needs_setup"),
        ServerHealth::Disconnected => Some("disconnected"),
        ServerHealth::Error => Some("error"),
    }
}

/// Derive agent-facing readiness from binding membership and live pool state.
///
/// `ready` requires binding + `Connected` + no missing required inputs; `bound` covers
/// bound-but-offline/auth/setup cases; `bindable` means not in the active binding.
pub(crate) fn derive_server_readiness(
    in_binding: bool,
    connection_status: ConnectionStatus,
    has_missing_inputs: bool,
) -> (&'static str, Option<&'static str>) {
    if !in_binding {
        return ("bindable", None);
    }

    if has_missing_inputs {
        return ("bound", Some("needs_setup"));
    }

    if connection_status == ConnectionStatus::Connected {
        return ("ready", None);
    }

    let health = classify_health(connection_status, false);
    let blocking = blocking_reason_from_health(health).or(Some("disconnected"));
    ("bound", blocking)
}

/// Structured invoke denial reason and remedy meta tool when a server cannot accept calls.
pub(crate) fn classify_invoke_denial(
    in_binding: bool,
    connection_status: ConnectionStatus,
    has_missing_inputs: bool,
) -> Option<(&'static str, &'static str)> {
    let (readiness, blocking_reason) =
        derive_server_readiness(in_binding, connection_status, has_missing_inputs);

    match readiness {
        "ready" => None,
        "bindable" => Some(("inactive", "mcpmux_bind_current_workspace")),
        "bound" => {
            let reason = match blocking_reason {
                Some("needs_setup") => "needs_setup",
                Some("auth_required") => "auth_required",
                _ => "bound_offline",
            };
            Some((reason, "mcpmux_diagnose_server"))
        }
        _ => None,
    }
}

/// Human-readable `action` string for structured invoke denial payloads.
pub(crate) fn format_invoke_not_ready_action(reason: &str, server_id: &str) -> String {
    match reason {
        "inactive" => format_server_inactive_error(server_id),
        "permission_denied" => format!(
            "Tool not granted for server '{server_id}'. \
             Use mcpmux_search_tools to discover invokable tools with current grants."
        ),
        "auth_required" => format!(
            "Server '{server_id}' requires authentication. Run mcpmux_diagnose_server to connect."
        ),
        "needs_setup" => format!(
            "Server '{server_id}' has missing required setup inputs. Run mcpmux_diagnose_server to see what's needed."
        ),
        _ => format_server_bound_offline_error(server_id),
    }
}

/// Like [`format_invoke_not_ready_action`] but appends the server display name when known.
pub(crate) fn format_invoke_not_ready_action_with_name(
    reason: &str,
    server_id: &str,
    display_name: Option<&str>,
) -> String {
    let base = format_invoke_not_ready_action(reason, server_id);
    match display_name {
        Some(name) if !name.is_empty() && name != server_id => format!("{base} ({name})"),
        _ => base,
    }
}

/// Display names and pre-configured default param keys per installed server.
pub(crate) async fn build_installed_server_meta_maps(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
) -> Result<(HashMap<String, String>, HashMap<String, Vec<String>>), MetaToolError> {
    let installed = call
        .ctx
        .installed_server_repo
        .list_for_space(&space_id.to_string())
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;

    let mut display_names = HashMap::new();
    let mut prefilled_params = HashMap::new();
    for server in installed {
        display_names.insert(server.server_id.clone(), server.display_name().to_string());
        if !server.default_params.is_empty() {
            let mut keys: Vec<String> = server.default_params.keys().cloned().collect();
            keys.sort();
            prefilled_params.insert(server.server_id.clone(), keys);
        }
    }

    Ok((display_names, prefilled_params))
}

/// Whether the caller omitted or blanked the search query.
pub(crate) fn is_query_empty(query: Option<&str>) -> bool {
    query.map(str::trim).is_none_or(str::is_empty)
}

/// Point-in-time `readiness` label per server for search hit enrichment.
pub(crate) async fn build_server_readiness_map(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
    resolved: &ResolvedFeatureSet,
) -> Result<HashMap<String, &'static str>, MetaToolError> {
    let binding_features = call
        .ctx
        .feature_service
        .resolve_feature_sets(&space_id.to_string(), &resolved.feature_set_ids)
        .await?;
    let binding_servers: HashSet<String> = binding_features
        .iter()
        .map(|f| f.server_id.clone())
        .collect();

    let installed = call
        .ctx
        .installed_server_repo
        .list_for_space(&space_id.to_string())
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;
    let installed_by_id: HashMap<String, mcpmux_core::InstalledServer> = installed
        .into_iter()
        .map(|s| (s.server_id.clone(), s))
        .collect();

    let pool_statuses = call.ctx.server_manager.get_all_statuses(*space_id).await;

    let server_ids: HashSet<String> = binding_servers
        .iter()
        .chain(installed_by_id.keys())
        .cloned()
        .collect();

    let map = server_ids
        .into_iter()
        .map(|server_id| {
            let in_binding = binding_servers.contains(&server_id);
            let connection_status = pool_statuses
                .get(&server_id)
                .map(|(status, _, _, _)| *status)
                .unwrap_or(ConnectionStatus::Disconnected);
            let has_missing_inputs = installed_by_id
                .get(&server_id)
                .map(|server| !parse_missing_required_inputs(server).is_empty())
                .unwrap_or(false);
            let (readiness, _) =
                derive_server_readiness(in_binding, connection_status, has_missing_inputs);
            (server_id, readiness)
        })
        .collect();
    Ok(map)
}

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

pub(crate) fn parse_uuid_arg(args: &Value, field: &str) -> Result<Uuid, MetaToolError> {
    let s = args
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| MetaToolError::InvalidArgument(format!("missing `{field}`")))?;
    Uuid::parse_str(s)
        .map_err(|_| MetaToolError::InvalidArgument(format!("`{field}` is not a UUID: {s}")))
}
