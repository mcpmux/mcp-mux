//! Self-management meta tools (`mcpmux_*`).
//!
//! A small built-in toolset exposed by the gateway alongside the filtered
//! backend tools. Lets connected LLMs introspect the currently resolved
//! FeatureSet, see what tools exist unfiltered, and — gated by user
//! approval — reshape their own session's toolset (pin, create FS, bind
//! workspace, flip the Space's active FS).
//!
//! Design: the write tools are the token-savings feature. When a project
//! only needs 10 of 80 connected tools, the LLM can call
//! `mcpmux_pin_this_session` after reviewing the workspace, and the next
//! `tools/list` returns only the 10. Existing `tools/list_changed`
//! notification plumbing lands the reduced set in-session.
//!
//! Security: every write tool routes through [`approval::ApprovalBroker`]
//! which pops a native desktop dialog showing the concrete tool-list diff
//! before allowing the change. Headless gateways return `approval_required`.
//! Reads are unmetered.
//!
//! Namespace: all meta tools have names starting with `MCPMUX_PREFIX`
//! (`mcpmux_`) so the handler can route them before feature-set filtering.

pub mod approval;
mod bind_workspace;
mod diagnose_server;
mod diagnose_view;
pub mod diff;
pub mod disclosure_backend;
mod disclosure_read;
mod disclosure_search;
mod feature_set_tools;
mod invoke_alias;
pub mod invoke_backend;
mod invoke_payload_parse;
pub mod invoke_result_filter;
mod invoke_result_shaping;
pub mod invoke_tool;
mod list_servers;
mod meta_tool_common;
mod registry;
mod search_tools;
mod search_tools_index;
mod set_workspace_root;
mod token_budget;

pub use approval::{
    ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher, ApprovalRequest,
    ApprovalScope, ResolutionNotifier, META_TOOL_APPROVAL_EVENT, META_TOOL_APPROVAL_RESOLVED_EVENT,
};
pub use bind_workspace::BindCurrentWorkspaceTool;
pub use diagnose_server::DiagnoseServerTool;
pub use diff::ToolDiff;
pub use disclosure_backend::{pool_as_disclosure_backend, DisclosureBackend};
pub use feature_set_tools::{GetToolSchemaTool, ListFeatureSetsTool};
pub use invoke_backend::{routing_as_invoke_backend, InvokeToolBackend};
pub use invoke_tool::InvokeToolTool;
pub use list_servers::ListServersTool;
pub use registry::{
    feature_set_ids_fingerprint, MetaToolContext, MetaToolError, MetaToolRegistry,
    META_TOOLS_ENABLED_KEY,
};
pub use search_tools::SearchToolsTool;
pub use set_workspace_root::SetWorkspaceRootTool;
pub use token_budget::{measure_meta_tool_token_budget, MetaToolTokenBudget};

use std::path::PathBuf;

use crate::services::{EmbeddingService, ToolDiscoveryService};

/// Every built-in tool's name must start with this prefix so the handler
/// can intercept it before routing to backend servers.
pub const MCPMUX_PREFIX: &str = "mcpmux_";

/// Tools advertised in `tools/list` on every session. The remainder are
/// registered (callable) but hidden — agents reach them through the
/// error/hint recovery strings that name them when needed.
///
/// Core = the hot path every session: discover → schema → invoke + roster.
/// `mcpmux_set_workspace_root` is included here because it is the only
/// escape hatch when the automatic roots/list probe fails (PendingRoots
/// state) — without it in the list the LLM has no way to unblock itself.
pub const CORE_META_TOOLS: &[&str] = &[
    "mcpmux_search_tools",
    "mcpmux_invoke_tool",
    "mcpmux_get_tool_schema",
    "mcpmux_list_servers",
    "mcpmux_set_workspace_root",
    "mcpmux_bind_current_workspace",
];

/// Convenience: is this tool name one of ours?
pub fn is_meta_tool(name: &str) -> bool {
    name.starts_with(MCPMUX_PREFIX)
}

/// Factory wiring a fully-configured registry with every default tool.
///
/// Callers (ServiceContainer) construct one of these at gateway startup
/// and clone the Arc freely.
#[allow(clippy::too_many_arguments)]
pub fn build_default_registry(
    client_repo: std::sync::Arc<dyn mcpmux_core::InboundMcpClientRepository>,
    space_repo: std::sync::Arc<dyn mcpmux_core::SpaceRepository>,
    feature_set_repo: std::sync::Arc<dyn mcpmux_core::FeatureSetRepository>,
    binding_repo: std::sync::Arc<dyn mcpmux_core::WorkspaceBindingRepository>,
    server_feature_repo: std::sync::Arc<dyn mcpmux_core::ServerFeatureRepository>,
    installed_server_repo: std::sync::Arc<dyn mcpmux_core::InstalledServerRepository>,
    resolver: std::sync::Arc<crate::services::FeatureSetResolverService>,
    feature_service: std::sync::Arc<crate::pool::FeatureService>,
    invoke_backend: Option<std::sync::Arc<dyn invoke_backend::InvokeToolBackend>>,
    disclosure_backend: Option<std::sync::Arc<dyn disclosure_backend::DisclosureBackend>>,
    session_roots: std::sync::Arc<crate::services::SessionRootsRegistry>,
    approval_broker: std::sync::Arc<ApprovalBroker>,
    domain_event_tx: tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>,
    settings_repo: Option<std::sync::Arc<dyn mcpmux_core::AppSettingsRepository>>,
    server_manager: std::sync::Arc<crate::pool::ServerManager>,
    log_manager: std::sync::Arc<mcpmux_core::ServerLogManager>,
    data_dir: PathBuf,
    embedding_repo: std::sync::Arc<dyn mcpmux_core::EmbeddingRepository>,
) -> std::sync::Arc<MetaToolRegistry> {
    let tool_discovery =
        std::sync::Arc::new(ToolDiscoveryService::new(server_feature_repo.clone()));
    let resource_discovery = std::sync::Arc::new(crate::services::ResourceDiscoveryService::new(
        server_feature_repo.clone(),
    ));
    let prompt_discovery = std::sync::Arc::new(crate::services::PromptDiscoveryService::new(
        server_feature_repo.clone(),
    ));
    let search_cache = session_roots.search_cache();
    let embedding_store = std::sync::Arc::new(dashmap::DashMap::new());
    let embeddings = std::sync::Arc::new(EmbeddingService::new(data_dir));
    let ctx = MetaToolContext {
        client_repo,
        space_repo,
        feature_set_repo,
        binding_repo,
        server_feature_repo,
        installed_server_repo,
        resolver,
        feature_service,
        invoke_backend,
        tool_discovery,
        resource_discovery,
        prompt_discovery,
        disclosure_backend,
        session_roots,
        approval_broker,
        domain_event_tx,
        settings_repo,
        server_manager,
        log_manager,
        search_cache,
        embedding_store,
        embedding_repo,
        embeddings,
    };

    let mut registry = MetaToolRegistry::new(ctx);
    // Reads — no approval needed.
    registry.register(Box::new(feature_set_tools::ListFeatureSetsTool));
    registry.register(Box::new(list_servers::ListServersTool));
    registry.register(Box::new(search_tools::SearchToolsTool));
    registry.register(Box::new(feature_set_tools::GetToolSchemaTool));
    registry.register(Box::new(diagnose_server::DiagnoseServerTool));
    registry.register(Box::new(invoke_tool::InvokeToolTool));
    registry.register(Box::new(disclosure_search::SearchResourcesTool));
    registry.register(Box::new(disclosure_read::ReadResourceTool));
    registry.register(Box::new(disclosure_search::SearchPromptsTool));
    registry.register(Box::new(disclosure_read::FetchPromptTool));
    // Writes — gated by ApprovalBroker (bind-only; humans author bundles in UI).
    registry.register(Box::new(bind_workspace::BindCurrentWorkspaceTool));
    // Session root override — no approval (ephemeral in-memory only).
    registry.register(Box::new(set_workspace_root::SetWorkspaceRootTool));
    std::sync::Arc::new(registry)
}
