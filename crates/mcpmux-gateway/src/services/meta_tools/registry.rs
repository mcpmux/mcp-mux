//! MetaTool trait + registry.
//!
//! Each meta tool is a unit struct implementing [`MetaTool`]. The registry
//! dispatches a tool name to its handler and exposes `list()` for the MCP
//! `tools/list` response.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use dashmap::DashMap;
use mcpmux_core::{
    DomainEvent, EmbeddingRepository, FeatureSetRepository, InboundMcpClientRepository,
    InstalledServerRepository, ServerFeatureRepository, ServerLogManager, SpaceRepository,
    WorkspaceBindingRepository,
};
use rmcp::model::{CallToolResult, Tool};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::broadcast;
use tracing::debug;
use uuid::Uuid;

use super::approval::ApprovalBroker;
use super::disclosure_backend::DisclosureBackend;
use super::invoke_backend::InvokeToolBackend;
use crate::pool::{FeatureService, ServerManager};
use crate::services::{
    EmbeddingService, FeatureSetResolverService, PromptDiscoveryService, ResourceDiscoveryService,
    SessionRootsRegistry, ToolDiscoveryService, ToolIndex,
};

/// Stable hash of sorted `feature_set_ids` for per-session search cache keys.
pub fn feature_set_ids_fingerprint(feature_set_ids: &[String]) -> u64 {
    let mut ids = feature_set_ids.to_vec();
    ids.sort();
    let mut hasher = DefaultHasher::new();
    for id in ids {
        id.hash(&mut hasher);
    }
    hasher.finish()
}

/// App-settings key that toggles the entire `mcpmux_*` namespace.
/// Present + "false" → hidden; missing or anything else → enabled.
pub const META_TOOLS_ENABLED_KEY: &str = "gateway.meta_tools_enabled";

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
    pub installed_server_repo: Arc<dyn InstalledServerRepository>,
    pub resolver: Arc<FeatureSetResolverService>,
    pub feature_service: Arc<FeatureService>,
    /// Backend invoke path — required for `mcpmux_invoke_tool`.
    pub invoke_backend: Option<Arc<dyn InvokeToolBackend>>,
    pub tool_discovery: Arc<ToolDiscoveryService>,
    pub resource_discovery: Arc<ResourceDiscoveryService>,
    pub prompt_discovery: Arc<PromptDiscoveryService>,
    /// Backend read/fetch path — required for `mcpmux_read_resource` / `mcpmux_fetch_prompt`.
    pub disclosure_backend: Option<Arc<dyn DisclosureBackend>>,
    pub session_roots: Arc<SessionRootsRegistry>,
    pub approval_broker: Arc<ApprovalBroker>,
    /// Broadcast domain events (e.g. ToolsChanged) so MCPNotifier can push
    /// `tools/list_changed` to connected peers after a write mutates state.
    pub domain_event_tx: broadcast::Sender<DomainEvent>,
    /// App-settings repo for the `gateway.meta_tools_enabled` master switch.
    /// Optional because older dependency builders may not have wired it.
    /// When absent the switch defaults to ENABLED (matches the product default).
    pub settings_repo: Option<Arc<dyn mcpmux_core::AppSettingsRepository>>,
    /// Runtime connection status for installed servers (pool orchestrator).
    pub server_manager: Arc<ServerManager>,
    /// Per-server log tail reader (`current.log`); same source as the desktop UI.
    pub log_manager: Arc<ServerLogManager>,
    /// Per-session active tool index for `mcpmux_search_tools` (fingerprint-keyed).
    pub search_cache: Arc<DashMap<String, (u64, ToolIndex)>>,
    /// Global embedding vectors keyed by content hash.
    pub embedding_store: Arc<DashMap<String, Vec<f32>>>,
    /// Persistent embedding repository backing `embedding_store` hydration.
    pub embedding_repo: Arc<dyn EmbeddingRepository>,
    /// Local ONNX embedding service for hybrid tool ranking.
    pub embeddings: Arc<EmbeddingService>,
}

/// Per-request metadata threaded through every tool call.
///
/// `client_id` is the OAuth client identity from the JWT — opaque string
/// (a UUID for preset-clients, a `client_metadata` URL for DCR-registered
/// clients like Claude Code). The registry treats it as a hash key only.
pub struct MetaToolCall<'a> {
    pub client_id: &'a str,
    pub session_id: Option<&'a str>,
    /// JSON arguments supplied in `CallToolRequestParams.arguments`.
    pub args: Value,
    pub ctx: &'a MetaToolContext,
    /// Write tools set this before returning `Ok` to override the default
    /// `"allow_once"` audit decision (e.g. workspace bind).
    pub audit_decision: Arc<Mutex<Option<&'static str>>>,
    /// Physical device identity from the caller's `X-Mcpmux-Machine-Id`
    /// header, when the transport carries one. `None` for local/stdio
    /// callers and for tests that don't exercise per-device routing.
    pub request_machine_id: Option<Uuid>,
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

    /// Master switch: are meta tools enabled in app settings? When disabled,
    /// the gateway handler hides `mcpmux_*` from `list_tools` and routes
    /// `call_tool` invocations straight to the feature-set path (where they
    /// will miss and return "tool not found").
    ///
    /// Default when the setting is missing or the repo is not wired: ON.
    /// Default when the setting value is unparseable: ON (fail-open on the
    /// discoverability side; security-sensitive writes still require approval).
    pub async fn is_enabled(&self) -> bool {
        let Some(repo) = self.ctx.settings_repo.as_ref() else {
            return true;
        };
        match repo.get(META_TOOLS_ENABLED_KEY).await {
            Ok(Some(v)) => !matches!(v.as_str(), "false" | "0"),
            _ => true,
        }
    }

    /// The `rmcp::model::Tool` list advertised to clients.
    ///
    /// Only [`super::CORE_META_TOOLS`] are included. The remaining registered
    /// tools are hidden from `tools/list` but remain fully callable — agents
    /// reach them through the error/hint recovery strings that name them.
    pub fn list_as_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<_> = self
            .tools
            .values()
            .filter(|t| super::CORE_META_TOOLS.contains(&t.name()))
            .map(|t| {
                let name = t.name();
                let schema: serde_json::Map<String, Value> = match serde_json::from_value(
                    t.input_schema(),
                ) {
                    Ok(map) => map,
                    Err(e) => {
                        debug!(tool = name, error = %e, "meta tool input_schema parse failed; advertising empty schema");
                        serde_json::Map::new()
                    }
                };
                let mut tool = Tool::new(name, t.description(), Arc::new(schema));
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
            .collect();
        tools.sort_by_key(|t| {
            super::CORE_META_TOOLS
                .iter()
                .position(|name| *name == t.name.as_ref())
                .unwrap_or(usize::MAX)
        });
        tools
    }

    /// Dispatch. Caller (the MCP handler) has already verified the name
    /// starts with our prefix.
    ///
    /// Every invocation — read or write, success or failure — emits a
    /// [`DomainEvent::MetaToolInvoked`] audit event so the desktop
    /// Connection Log can render a row. Read tools get `decision = "read"`;
    /// write tools get the actual approval decision or an error string.
    pub async fn call(
        &self,
        name: &str,
        client_id: &str,
        session_id: Option<&str>,
        args: Value,
    ) -> Result<CallToolResult, MetaToolError> {
        self.call_from_device(name, client_id, session_id, args, None)
            .await
    }

    /// Like [`Self::call`] but threads through the caller's physical device
    /// identity (`X-Mcpmux-Machine-Id`) when the transport has one, so
    /// device-scoped meta tools (workspace binding) write and resolve
    /// against the same machine the resolver would pick for this caller.
    pub async fn call_from_device(
        &self,
        name: &str,
        client_id: &str,
        session_id: Option<&str>,
        args: Value,
        request_machine_id: Option<Uuid>,
    ) -> Result<CallToolResult, MetaToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| MetaToolError::InvalidArgument(format!("unknown meta tool: {name}")))?;
        let is_write = tool.is_write();
        let audit_decision = Arc::new(Mutex::new(None));
        let call = MetaToolCall {
            client_id,
            session_id,
            args: args.clone(),
            ctx: &self.ctx,
            audit_decision: audit_decision.clone(),
            request_machine_id,
        };
        let result = tool.call(call).await;

        let (decision, summary) = match &result {
            Ok(_) if is_write => (
                audit_decision
                    .lock()
                    .ok()
                    .and_then(|g| *g)
                    .unwrap_or("allow_once"),
                format!("{name} succeeded"),
            ),
            Ok(_) => ("read", format!("{name} read")),
            Err(MetaToolError::ApprovalDenied) => ("deny", format!("{name} denied by user")),
            Err(MetaToolError::ApprovalTimedOut) => ("timeout", format!("{name} timed out")),
            Err(MetaToolError::ApprovalRequiredNoDesktop) => {
                ("approval_required", format!("{name} no desktop"))
            }
            Err(MetaToolError::RateLimited) => ("rate_limited", format!("{name} rate-limited")),
            Err(MetaToolError::InvalidArgument(m)) => ("invalid_args", format!("{name}: {m}")),
            Err(MetaToolError::Internal(m)) => ("error", format!("{name}: {m}")),
        };
        let _ = self.ctx.domain_event_tx.send(DomainEvent::MetaToolInvoked {
            client_id: client_id.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            tool_name: name.to_string(),
            decision: decision.to_string(),
            resolved_feature_set_id: None,
            summary,
        });

        result
    }

    pub fn context(&self) -> &MetaToolContext {
        &self.ctx
    }

    /// Evict cached active index for one MCP session.
    pub fn evict_search_cache_for_session(&self, session_id: &str) {
        self.ctx.search_cache.remove(session_id);
    }

    /// Whether a session has a cached active search index entry.
    pub fn search_cache_contains(&self, session_id: &str) -> bool {
        self.ctx.search_cache.contains_key(session_id)
    }
}
