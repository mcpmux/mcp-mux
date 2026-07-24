//! `mcpmux_diagnose_server` meta tool and diagnose helper functions.
//!
//! Logic ported from [`dashboard.helpers.ts`](../../../../apps/desktop/src/features/dashboard/dashboard.helpers.ts):
//! missing required inputs, health buckets, and server diagnosis assembly.

use std::collections::HashMap;

use async_trait::async_trait;
#[allow(unused_imports)]
use mcpmux_core::{FeatureType, InstalledServer, LogLevel, ServerDefinition, TransportConfig};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use uuid::Uuid;

use super::diagnose_view::{
    build_config_view_from_definition, build_runtime_view, parse_diagnose_args, DiagnoseArgs,
};

pub use super::diagnose_view::{ConfigView, ServerHealth};
use super::meta_tool_common::{caller_space_id, text_result};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use crate::pool::ConnectionStatus;

/// Returns IDs of required transport inputs that have no user value.
///
/// Mirrors `hasMissingRequiredInputs` in the dashboard: uses `cached_definition`
/// transport metadata and treats empty strings as missing. Invalid JSON yields
/// an empty list (same as the TS `catch` path).
pub fn parse_missing_required_inputs(installed: &InstalledServer) -> Vec<String> {
    let Some(definition) = installed.get_definition() else {
        return Vec::new();
    };

    let values = &installed.input_values;
    let mut missing = Vec::new();

    for input in &definition.transport.metadata().inputs {
        if !input.required {
            continue;
        }
        let has_value = values.get(&input.id).is_some_and(|v| !v.is_empty());
        if !has_value {
            missing.push(input.id.clone());
        }
    }

    missing.sort();
    missing
}

/// Whether any required input is unset (see [`parse_missing_required_inputs`]).
#[allow(dead_code)]
pub fn has_missing_required_inputs(installed: &InstalledServer) -> bool {
    !parse_missing_required_inputs(installed).is_empty()
}

/// Map runtime connection status and setup state to a diagnose health bucket.
///
/// Priority matches the dashboard attention panel: missing inputs win over
/// runtime status; then error, then OAuth required, then disconnected.
pub fn classify_health(status: ConnectionStatus, has_missing_inputs: bool) -> ServerHealth {
    if has_missing_inputs {
        return ServerHealth::NeedsSetup;
    }

    match status {
        ConnectionStatus::Error => ServerHealth::Error,
        ConnectionStatus::AuthRequired => ServerHealth::AuthRequired,
        ConnectionStatus::Disconnected => ServerHealth::Disconnected,
        ConnectionStatus::Connected
        | ConnectionStatus::Connecting
        | ConnectionStatus::Refreshing
        | ConnectionStatus::Authenticating => ServerHealth::Healthy,
    }
}

/// Build a redacted config view from the installed server's cached definition.
///
/// Secret input values are never included; only transport shape and key names.
pub fn build_config_view(installed: &InstalledServer) -> ConfigView {
    let Some(definition) = installed.get_definition() else {
        return ConfigView::default();
    };

    build_config_view_from_definition(&definition)
}

/// Re-export for sibling modules that imported `diagnose::connection_status_label`.
pub(crate) use super::diagnose_view::connection_status_label;

/// Count installed tool features per server in a Space.
async fn tool_counts_for_space(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
) -> Result<HashMap<String, usize>, MetaToolError> {
    let features = call
        .ctx
        .server_feature_repo
        .list_for_space(&space_id.to_string())
        .await?;
    let mut counts = HashMap::new();
    for feature in features {
        if feature.feature_type != FeatureType::Tool {
            continue;
        }
        *counts.entry(feature.server_id.clone()).or_insert(0) += 1;
    }
    Ok(counts)
}

/// Read and serialize the log tail for one server when requested.
async fn build_logs_view(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
    server_id: &str,
    include_logs: bool,
    log_limit: usize,
    log_level_filter: Option<LogLevel>,
) -> Result<Option<Value>, MetaToolError> {
    if !include_logs {
        return Ok(None);
    }

    let entries = call
        .ctx
        .log_manager
        .read_logs(
            &space_id.to_string(),
            server_id,
            log_limit,
            log_level_filter,
        )
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;

    Ok(Some(json!({
        "count": entries.len(),
        "level_filter": log_level_filter.map(|level| level.as_str()),
        "entries": entries,
    })))
}

/// Build one server entry for the diagnose response payload.
async fn build_server_diagnosis(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
    installed: &InstalledServer,
    runtime: (ConnectionStatus, u64, bool, Option<String>),
    tool_count: usize,
    args: &DiagnoseArgs,
) -> Result<Value, MetaToolError> {
    let missing = parse_missing_required_inputs(installed);
    let has_missing = !missing.is_empty();
    let (status, flow_id, has_connected_before, message) = runtime;
    let health = classify_health(status, has_missing);

    let mut entry = json!({
        "server_id": installed.server_id,
        "display_name": installed.display_name(),
        "health": health,
        "runtime": build_runtime_view(status, flow_id, has_connected_before, message),
        "config": build_config_view(installed),
        "missing_required_inputs": missing,
        "tool_count": tool_count,
    });

    if let Some(logs) = build_logs_view(
        call,
        space_id,
        &installed.server_id,
        args.include_logs,
        args.log_limit,
        args.log_level_filter,
    )
    .await?
    {
        entry["logs"] = logs;
    }

    Ok(entry)
}

/// Read-only combo diagnostic for MCP servers in the caller's resolved Space.
pub struct DiagnoseServerTool;

#[async_trait]
impl MetaTool for DiagnoseServerTool {
    fn name(&self) -> &'static str {
        "mcpmux_diagnose_server"
    }

    fn description(&self) -> &'static str {
        "Operator diagnostic: return runtime status, redacted transport config, \
         missing required inputs, and a recent log tail for MCP servers in the \
         caller's resolved Space. Omit server_id to list only unhealthy servers; \
         pass server_id to inspect one server regardless of health."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_id": {
                    "type": "string",
                    "description": "Optional. When omitted, only unhealthy servers are returned"
                },
                "include_logs": {
                    "type": "boolean",
                    "default": true,
                    "description": "Set false to omit the logs block"
                },
                "log_limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 500,
                    "default": 50,
                    "description": "Maximum number of log entries to return"
                },
                "log_level_filter": {
                    "type": "string",
                    "enum": ["trace", "debug", "info", "warn", "error"],
                    "description": "Minimum log level to include (inclusive)"
                }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let space_id = caller_space_id(&call).await?;
        let args = parse_diagnose_args(&call.args)?;

        let installed = call
            .ctx
            .installed_server_repo
            .list_for_space(&space_id.to_string())
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        if let Some(ref target) = args.server_id {
            if !installed.iter().any(|s| &s.server_id == target) {
                return Err(MetaToolError::InvalidArgument(format!(
                    "unknown server_id '{target}' in this Space"
                )));
            }
        }

        let statuses = call.ctx.server_manager.get_all_statuses(space_id).await;
        let tool_counts = tool_counts_for_space(&call, &space_id).await?;

        let mut servers: Vec<Value> = Vec::new();
        for server in &installed {
            if args
                .server_id
                .as_ref()
                .is_some_and(|target| &server.server_id != target)
            {
                continue;
            }

            let runtime = statuses.get(&server.server_id).cloned().unwrap_or((
                ConnectionStatus::Disconnected,
                0_u64,
                false,
                None::<String>,
            ));

            let missing = parse_missing_required_inputs(server);
            let health = classify_health(runtime.0, !missing.is_empty());

            if args.server_id.is_none() && !health.is_unhealthy() {
                continue;
            }

            let tool_count = tool_counts.get(&server.server_id).copied().unwrap_or(0);
            servers.push(
                build_server_diagnosis(&call, &space_id, server, runtime, tool_count, &args)
                    .await?,
            );
        }

        servers.sort_by(|a, b| {
            a.get("server_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(b.get("server_id").and_then(|v| v.as_str()).unwrap_or(""))
        });

        let total_unhealthy = servers
            .iter()
            .filter(|entry| entry.get("health").and_then(|v| v.as_str()) != Some("healthy"))
            .count();

        Ok(text_result(json!({
            "space_id": space_id,
            "servers": servers,
            "total_unhealthy": total_unhealthy,
        })))
    }
}

#[cfg(test)]
#[path = "diagnose_tests.rs"]
mod tests;
