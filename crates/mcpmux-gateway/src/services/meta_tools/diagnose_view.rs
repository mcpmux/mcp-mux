//! Health types, config views, and runtime view builders for `mcpmux_diagnose_server`.
//!
//! Logic ported from [`dashboard.helpers.ts`](../../../../apps/desktop/src/features/dashboard/dashboard.helpers.ts):
//! redacted transport config views and runtime status serialization.

use mcpmux_core::{LogLevel, ServerDefinition, TransportConfig};
use serde::Serialize;
use serde_json::{json, Value};

use super::registry::MetaToolError;
use crate::pool::ConnectionStatus;

/// Operator-facing health bucket for a single installed server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerHealth {
    Healthy,
    Error,
    AuthRequired,
    NeedsSetup,
    Disconnected,
}

impl ServerHealth {
    /// Whether this bucket counts as unhealthy for no-arg diagnose filtering.
    pub fn is_unhealthy(self) -> bool {
        !matches!(self, Self::Healthy)
    }
}

/// Redacted transport configuration (keys only for secrets; no input values).
#[derive(Debug, Clone, Serialize, Default)]
pub struct ConfigView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub header_keys: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input_keys: Vec<String>,
}

/// Build a redacted config view from a server definition (no installed input values).
pub(crate) fn build_config_view_from_definition(definition: &ServerDefinition) -> ConfigView {
    let metadata = definition.transport.metadata();
    let mut input_keys: Vec<String> = metadata.inputs.iter().map(|i| i.id.clone()).collect();
    input_keys.sort();
    input_keys.dedup();

    match &definition.transport {
        TransportConfig::Stdio {
            command, args, env, ..
        } => {
            let mut env_keys: Vec<String> = env.keys().cloned().collect();
            env_keys.sort();

            ConfigView {
                transport_type: Some("stdio".to_string()),
                command: Some(command.clone()),
                url: None,
                args: args.clone(),
                env_keys,
                header_keys: Vec::new(),
                input_keys,
            }
        }
        TransportConfig::Http { url, headers, .. } => {
            let mut header_keys: Vec<String> = headers.keys().cloned().collect();
            header_keys.sort();

            ConfigView {
                transport_type: Some("http".to_string()),
                command: None,
                url: Some(url.clone()),
                args: Vec::new(),
                env_keys: Vec::new(),
                header_keys,
                input_keys,
            }
        }
    }
}

/// Serialize a pool [`ConnectionStatus`] as the diagnose runtime status string.
pub(crate) fn connection_status_label(status: ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Disconnected => "disconnected",
        ConnectionStatus::Connecting => "connecting",
        ConnectionStatus::Connected => "connected",
        ConnectionStatus::Refreshing => "refreshing",
        ConnectionStatus::AuthRequired => "auth_required",
        ConnectionStatus::Authenticating => "authenticating",
        ConnectionStatus::Error => "error",
    }
}

/// Parsed arguments for [`super::diagnose_server::DiagnoseServerTool`].
pub(crate) struct DiagnoseArgs {
    pub(crate) server_id: Option<String>,
    pub(crate) include_logs: bool,
    pub(crate) log_limit: usize,
    pub(crate) log_level_filter: Option<LogLevel>,
}

/// Parse and validate `mcpmux_diagnose_server` call arguments.
pub(crate) fn parse_diagnose_args(args: &Value) -> Result<DiagnoseArgs, MetaToolError> {
    let server_id = args
        .get("server_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let include_logs = args
        .get("include_logs")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let log_limit = args
        .get("log_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(500) as usize;

    let log_level_filter = match args.get("log_level_filter") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let Some(raw) = v.as_str() else {
                return Err(MetaToolError::InvalidArgument(
                    "`log_level_filter` must be a string".into(),
                ));
            };
            Some(LogLevel::parse(raw).ok_or_else(|| {
                MetaToolError::InvalidArgument(format!(
                    "invalid log_level_filter '{raw}'; expected trace, debug, info, warn, or error"
                ))
            })?)
        }
    };

    Ok(DiagnoseArgs {
        server_id,
        include_logs,
        log_limit,
        log_level_filter,
    })
}

/// Build the runtime sub-object for one diagnosed server.
pub(crate) fn build_runtime_view(
    status: ConnectionStatus,
    flow_id: u64,
    has_connected_before: bool,
    message: Option<String>,
) -> Value {
    json!({
        "status": connection_status_label(status),
        "flow_id": flow_id,
        "has_connected_before": has_connected_before,
        "message": message,
    })
}
