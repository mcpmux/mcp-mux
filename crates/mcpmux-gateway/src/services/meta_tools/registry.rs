//! MetaTool trait + registry.
//!
//! Each meta tool is a unit struct implementing [`MetaTool`]. The registry
//! dispatches a tool name to its handler and exposes `list()` for the MCP
//! `tools/list` response.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use mcpmux_core::{
    DomainEvent, FeatureSetRepository, InboundMcpClientRepository, ServerFeatureRepository,
    SpaceRepository, WorkspaceBindingRepository,
};
use rmcp::model::{CallToolResult, Tool};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::approval::ApprovalBroker;
use crate::pool::FeatureService;
use crate::services::{FeatureSetResolverService, SessionRootsRegistry};

/// Context injected into every meta-tool invocation.
///
/// Cheap to clone (all `Arc`s); the registry holds one and hands references
/// to tools via [`MetaToolContext`].
#[derive(Clone)]
pub struct MetaToolContext {
    pub client_repo: Arc<dyn InboundMcpClientRepository>,
    pub space_repo: Arc<dyn SpaceRepository>,
    pub feature_set_repo: Arc<dyn FeatureSetRepository>,
    pub binding_repo: Arc<dyn WorkspaceBindingRepository>,
    pub server_feature_repo: Arc<dyn ServerFeatureRepository>,
    pub resolver: Arc<FeatureSetResolverService>,
    pub feature_service: Arc<FeatureService>,
    pub session_roots: Arc<SessionRootsRegistry>,
    pub approval_broker: Arc<ApprovalBroker>,
    /// Broadcast domain events (e.g. ToolsChanged) so MCPNotifier can push
    /// `tools/list_changed` to connected peers after a write mutates state.
    pub domain_event_tx: broadcast::Sender<DomainEvent>,
}

/// Per-request metadata threaded through every tool call.
pub struct MetaToolCall<'a> {
    pub client_id: &'a Uuid,
    pub session_id: Option<&'a str>,
    /// JSON arguments supplied in `CallToolRequestParams.arguments`.
    pub args: Value,
    pub ctx: &'a MetaToolContext,
}

/// Errors a meta tool can surface that map cleanly to `CallToolResult::error`.
#[derive(Debug, Error)]
pub enum MetaToolError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("approval denied by user")]
    ApprovalDenied,
    #[error("approval request timed out")]
    ApprovalTimedOut,
    #[error("approval required but no desktop attached to mcpmux gateway")]
    ApprovalRequiredNoDesktop,
    #[error("rate limited: too many pending approvals for this client")]
    RateLimited,
    #[error("internal: {0}")]
    Internal(String),
}

impl MetaToolError {
    /// Convert to an MCP error result (user-visible message).
    pub fn into_call_tool_result(self) -> CallToolResult {
        use rmcp::model::Content;
        let payload = serde_json::json!({
            "error": match &self {
                MetaToolError::InvalidArgument(_) => "invalid_argument",
                MetaToolError::ApprovalDenied => "approval_denied",
                MetaToolError::ApprovalTimedOut => "approval_timed_out",
                MetaToolError::ApprovalRequiredNoDesktop => "approval_required",
                MetaToolError::RateLimited => "rate_limited",
                MetaToolError::Internal(_) => "internal_error",
            },
            "message": self.to_string(),
        });
        CallToolResult::error(vec![Content::text(payload.to_string())])
    }
}

impl From<anyhow::Error> for MetaToolError {
    fn from(e: anyhow::Error) -> Self {
        MetaToolError::Internal(e.to_string())
    }
}

/// A single self-management tool.
///
/// Tools are unit structs (no per-instance state) — all shared state comes
/// from [`MetaToolContext`].
#[async_trait]
pub trait MetaTool: Send + Sync {
    /// MCP tool name — must start with `mcpmux_`.
    fn name(&self) -> &'static str;

    /// MCP tool description (shown to the LLM).
    fn description(&self) -> &'static str;

    /// JSON-schema describing accepted arguments. The registry converts
    /// this into a [`rmcp::model::Tool`] with the right annotations.
    fn input_schema(&self) -> Value;

    /// Whether this tool modifies state. Writes are routed through the
    /// approval broker; reads are executed immediately.
    fn is_write(&self) -> bool {
        false
    }

    /// Run the tool.
    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError>;
}

/// Registry of every built-in tool. Constructed once at gateway startup.
pub struct MetaToolRegistry {
    ctx: MetaToolContext,
    tools: HashMap<&'static str, Box<dyn MetaTool>>,
}

impl MetaToolRegistry {
    pub fn new(ctx: MetaToolContext) -> Self {
        Self {
            ctx,
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn MetaTool>) {
        let name = tool.name();
        debug_assert!(
            name.starts_with(super::MCPMUX_PREFIX),
            "meta tool name must start with {}: got {name}",
            super::MCPMUX_PREFIX
        );
        self.tools.insert(name, tool);
    }

    /// Is `name` registered here?
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// The `rmcp::model::Tool` list advertised to clients.
    pub fn list_as_tools(&self) -> Vec<Tool> {
        self.tools
            .values()
            .map(|t| {
                let schema: serde_json::Map<String, Value> =
                    serde_json::from_value(t.input_schema()).unwrap_or_default();
                let mut tool = Tool::new(t.name(), t.description(), Arc::new(schema));
                // Annotate writes so well-behaved clients surface the hint.
                if t.is_write() {
                    let mut ann = tool.annotations.unwrap_or_default();
                    ann.destructive_hint = Some(true);
                    ann.read_only_hint = Some(false);
                    tool.annotations = Some(ann);
                } else {
                    let mut ann = tool.annotations.unwrap_or_default();
                    ann.read_only_hint = Some(true);
                    tool.annotations = Some(ann);
                }
                tool
            })
            .collect()
    }

    /// Dispatch. Caller (the MCP handler) has already verified the name
    /// starts with our prefix.
    pub async fn call(
        &self,
        name: &str,
        client_id: &Uuid,
        session_id: Option<&str>,
        args: Value,
    ) -> Result<CallToolResult, MetaToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| MetaToolError::InvalidArgument(format!("unknown meta tool: {name}")))?;
        let call = MetaToolCall {
            client_id,
            session_id,
            args,
            ctx: &self.ctx,
        };
        tool.call(call).await
    }

    pub fn context(&self) -> &MetaToolContext {
        &self.ctx
    }
}
