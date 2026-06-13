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
pub mod diff;
mod registry;
mod tools;

pub use approval::{
    ApprovalBroker, ApprovalDecision, ApprovalPayload, ApprovalPublisher, ApprovalRequest,
    ApprovalScope,
};
pub use diff::ToolDiff;
pub use registry::{MetaToolContext, MetaToolError, MetaToolRegistry};

/// Every built-in tool's name must start with this prefix so the handler
/// can intercept it before routing to backend servers.
pub const MCPMUX_PREFIX: &str = "mcpmux_";

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
    resolver: std::sync::Arc<crate::services::FeatureSetResolverService>,
    feature_service: std::sync::Arc<crate::pool::FeatureService>,
    session_roots: std::sync::Arc<crate::services::SessionRootsRegistry>,
    approval_broker: std::sync::Arc<ApprovalBroker>,
    domain_event_tx: tokio::sync::broadcast::Sender<mcpmux_core::DomainEvent>,
    settings_repo: Option<std::sync::Arc<dyn mcpmux_core::AppSettingsRepository>>,
    builtin_config_repo: Option<std::sync::Arc<dyn mcpmux_core::SpaceBuiltinConfigRepository>>,
) -> std::sync::Arc<MetaToolRegistry> {
    let ctx = MetaToolContext {
        client_repo,
        space_repo,
        feature_set_repo,
        binding_repo,
        server_feature_repo,
        resolver,
        feature_service,
        session_roots,
        approval_broker,
        domain_event_tx,
        settings_repo,
        builtin_config_repo,
    };

    let mut registry = MetaToolRegistry::new(ctx);
    // Reads — no approval needed.
    registry.register(Box::new(tools::ListAllToolsTool));
    registry.register(Box::new(tools::ListFeatureSetsTool));
    // Both `describe_resolution` and `describe_workspace` were removed by
    // user request — the read surface is just the two list_* tools above,
    // which an LLM can stitch into the same picture without an extra hop.
    // Writes — gated by ApprovalBroker.
    registry.register(Box::new(tools::ManageFeatureSetTool));
    registry.register(Box::new(tools::BindCurrentWorkspaceTool));
    std::sync::Arc::new(registry)
}
