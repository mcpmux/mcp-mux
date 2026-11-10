//! `mcpmux_list_servers` — roster of installed MCP servers with readiness.

use async_trait::async_trait;
use mcpmux_core::FeatureType;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::diagnose_server::{
    classify_health, connection_status_label, parse_missing_required_inputs, ServerHealth,
};
use super::meta_tool_common::{caller_resolution, derive_server_readiness, text_result};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use crate::pool::ConnectionStatus;
use crate::services::ResolutionSource;

/// Everything shows `readiness: "bindable"` when the resolver is at
/// `PendingRoots` — indistinguishable from a genuinely unbound workspace on
/// the server list alone. Give the agent a direct diagnosis instead of
/// making it infer the cause from "every server is bindable".
fn pending_roots_note(call: &MetaToolCall<'_>, source: ResolutionSource) -> Option<String> {
    if source != ResolutionSource::PendingRoots {
        return None;
    }
    let roots = call
        .session_id
        .and_then(|sid| call.ctx.resolver.session_roots().get(sid))
        .unwrap_or_default();
    if roots.len() > 1 {
        let listed = roots
            .iter()
            .map(|r| format!("  - {r}"))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!(
            "{} workspace roots reported and none is pinned, so no binding can be resolved \
             unambiguously — every server below shows as bindable regardless of any existing \
             binding. Reported roots:\n{listed}\n\
             Call mcpmux_set_workspace_root with exactly one of those paths (the workspace this \
             agent is actually working in) to disambiguate.",
            roots.len(),
        ))
    } else {
        Some(
            "No workspace root has arrived for this session yet, so no binding can be resolved \
             — every server below shows as bindable regardless of any existing binding. If this \
             persists, call mcpmux_set_workspace_root with this session's workspace path."
                .to_string(),
        )
    }
}

pub struct ListServersTool;

#[async_trait]
impl MetaTool for ListServersTool {
    fn name(&self) -> &'static str {
        "mcpmux_list_servers"
    }

    fn description(&self) -> &'static str {
        "List every MCP server installed in the caller's resolved Space with \
         readiness per server: bindable (not in the active binding — use \
         bindable_feature_set_ids with mcpmux_bind_current_workspace), bound \
         (in binding but not invokable — see blocking_reason), or ready (safe \
         to invoke). Each entry includes connection, health, and conditional \
         missing_inputs when setup is incomplete. Clone installs include \
         optional cloned_from (source server_id)."
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
        let installed_by_server: HashMap<String, mcpmux_core::InstalledServer> = installed
            .into_iter()
            .map(|s| (s.server_id.clone(), s))
            .collect();

        let pool_statuses = call.ctx.server_manager.get_all_statuses(space_id).await;

        let inactive_by_server: HashMap<String, HashSet<String>> = call
            .ctx
            .feature_service
            .list_inactive_discovery_tools(&space_id.to_string(), &resolved.feature_set_ids, None)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?
            .into_iter()
            .fold(HashMap::new(), |mut acc, entry| {
                acc.entry(entry.feature.server_id.clone())
                    .or_default()
                    .insert(entry.bindable_feature_set_id);
                acc
            });

        // Seed every installed server first so servers with no discovered tool features
        // still appear (e.g. auth-pending or needs-setup servers with zero rows).
        let mut by_server: HashMap<String, usize> = installed_by_server
            .keys()
            .map(|id| (id.clone(), 0usize))
            .collect();
        for feature in &features {
            if feature.feature_type != FeatureType::Tool {
                continue;
            }
            *by_server.entry(feature.server_id.clone()).or_insert(0) += 1;
        }

        let mut servers: Vec<Value> = by_server
            .into_iter()
            .map(|(id, tool_count)| {
                let installed = installed_by_server.get(&id);
                let name = installed
                    .map(|s| s.display_name().to_string())
                    .unwrap_or_else(|| id.clone());

                let in_binding = binding_servers.contains(&id);
                let connection_status = pool_statuses
                    .get(&id)
                    .map(|(status, _, _, _)| *status)
                    .unwrap_or(ConnectionStatus::Disconnected);
                let missing_inputs = installed
                    .map(parse_missing_required_inputs)
                    .unwrap_or_default();
                let has_missing_inputs = !missing_inputs.is_empty();
                let health = classify_health(connection_status, has_missing_inputs);
                let (readiness, blocking_reason) =
                    derive_server_readiness(in_binding, connection_status, has_missing_inputs);

                let mut entry = json!({
                    "id": id,
                    "name": name,
                    "tool_count": tool_count,
                    "readiness": readiness,
                    "connection": connection_status_label(connection_status),
                    "health": health,
                });

                if let Some(reason) = blocking_reason {
                    entry["blocking_reason"] = json!(reason);
                }
                if health == ServerHealth::NeedsSetup {
                    entry["missing_inputs"] = json!(missing_inputs);
                }
                if let Some(cloned_from) = installed.and_then(|s| s.cloned_from.as_ref()) {
                    entry["cloned_from"] = json!(cloned_from);
                }
                if let Some(server) = installed {
                    if !server.default_params.is_empty() {
                        let mut keys: Vec<&str> =
                            server.default_params.keys().map(String::as_str).collect();
                        keys.sort_unstable();
                        entry["prefilled_params"] = json!(keys);
                    }
                }
                if readiness == "bindable" {
                    if let Some(fs_ids) = inactive_by_server.get(&id) {
                        let mut ids: Vec<_> = fs_ids.iter().cloned().collect();
                        ids.sort();
                        entry["bindable_feature_set_ids"] = json!(ids);
                    }
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

        let mut result = json!({ "servers": servers });
        if let Some(note) = pending_roots_note(&call, resolved.source) {
            result["note"] = json!(note);
        }
        Ok(text_result(result))
    }
}
