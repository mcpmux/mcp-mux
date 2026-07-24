//! Meta-tool `tools/list` token budget measurement (tests + `pnpm count-tokens`).

use std::sync::Arc;

use serde_json::{json, Value};

use super::bind_workspace::BindCurrentWorkspaceTool;
use super::diagnose_server::DiagnoseServerTool;
use super::disclosure_read::{FetchPromptTool, ReadResourceTool};
use super::disclosure_search::{SearchPromptsTool, SearchResourcesTool};
use super::feature_set_tools::{GetToolSchemaTool, ListFeatureSetsTool};
use super::invoke_tool::InvokeToolTool;
use super::list_servers::ListServersTool;
use super::registry::MetaTool;
use super::search_tools::SearchToolsTool;
use super::CORE_META_TOOLS;
use rmcp::model::Tool;

/// Every tool registered in [`super::build_default_registry`] (11 agent-facing tools).
pub const ALL_REGISTERED_META_TOOL_NAMES: &[&str] = &[
    "mcpmux_list_feature_sets",
    "mcpmux_list_servers",
    "mcpmux_search_tools",
    "mcpmux_get_tool_schema",
    "mcpmux_diagnose_server",
    "mcpmux_invoke_tool",
    "mcpmux_search_resources",
    "mcpmux_read_resource",
    "mcpmux_search_prompts",
    "mcpmux_fetch_prompt",
    "mcpmux_bind_current_workspace",
    "mcpmux_set_workspace_root",
];

/// Slim MCP tool object (name + description + inputSchema) — matches `pnpm count-tokens` / planning doc.
fn slim_tool_json(tool: &dyn MetaTool) -> Value {
    json!({
        "name": tool.name(),
        "description": tool.description(),
        "inputSchema": tool.input_schema(),
    })
}

/// Full `tools/list` entry as built by [`super::MetaToolRegistry::list_as_tools`].
fn list_as_tools_entry_json(tool: &dyn MetaTool) -> Value {
    let schema: serde_json::Map<String, Value> =
        serde_json::from_value(tool.input_schema()).unwrap_or_default();
    let mut rmcp_tool = Tool::new(tool.name(), tool.description(), Arc::new(schema));
    if tool.is_write() {
        let mut ann = rmcp_tool.annotations.unwrap_or_default();
        ann.destructive_hint = Some(true);
        ann.read_only_hint = Some(false);
        rmcp_tool.annotations = Some(ann);
    } else {
        let mut ann = rmcp_tool.annotations.unwrap_or_default();
        ann.read_only_hint = Some(true);
        rmcp_tool.annotations = Some(ann);
    }
    serde_json::to_value(&rmcp_tool).unwrap_or_else(|_| slim_tool_json(tool))
}

/// Unit structs for each registered meta tool (same set as `build_default_registry`).
fn all_registered_meta_tools() -> Vec<Box<dyn MetaTool>> {
    vec![
        Box::new(ListFeatureSetsTool),
        Box::new(ListServersTool),
        Box::new(SearchToolsTool),
        Box::new(GetToolSchemaTool),
        Box::new(DiagnoseServerTool),
        Box::new(InvokeToolTool),
        Box::new(SearchResourcesTool),
        Box::new(ReadResourceTool),
        Box::new(SearchPromptsTool),
        Box::new(FetchPromptTool),
        Box::new(BindCurrentWorkspaceTool),
    ]
}

/// Byte length of serialized tool entries for the given tool names.
fn serialized_bytes(tool_names: &[&str], entry_json: fn(&dyn MetaTool) -> Value) -> usize {
    let name_set: std::collections::HashSet<&str> = tool_names.iter().copied().collect();
    all_registered_meta_tools()
        .into_iter()
        .filter(|t| name_set.contains(t.name()))
        .map(|t| entry_json(t.as_ref()).to_string().len())
        .sum()
}

/// Tiktoken-style token estimate from UTF-8 bytes (cl100k_base proxy: bytes / 4).
fn tiktoken_estimate_from_bytes(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

/// Claude context estimate (planning doc: tiktoken × 1.1).
fn claude_estimate(tiktoken: usize) -> usize {
    ((tiktoken as f64) * 1.1).ceil() as usize
}

/// Measured budgets for advertised core vs full registered surface (slim MCP JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaToolTokenBudget {
    pub core_bytes: usize,
    pub full_bytes: usize,
    pub core_tiktoken: usize,
    pub full_tiktoken: usize,
    pub core_claude_est: usize,
    pub full_claude_est: usize,
    pub saved_tiktoken: usize,
    pub saved_claude_est: usize,
    /// Serialized rmcp `Tool` entries (includes annotations) — upper bound on wire size.
    pub core_rmcp_bytes: usize,
    pub full_rmcp_bytes: usize,
}

/// Compute token budgets for core-only vs all registered meta tools.
pub fn measure_meta_tool_token_budget() -> MetaToolTokenBudget {
    let core_bytes = serialized_bytes(CORE_META_TOOLS, slim_tool_json);
    let full_bytes = serialized_bytes(ALL_REGISTERED_META_TOOL_NAMES, slim_tool_json);
    let core_rmcp_bytes = serialized_bytes(CORE_META_TOOLS, list_as_tools_entry_json);
    let full_rmcp_bytes =
        serialized_bytes(ALL_REGISTERED_META_TOOL_NAMES, list_as_tools_entry_json);
    let core_tiktoken = tiktoken_estimate_from_bytes(core_bytes);
    let full_tiktoken = tiktoken_estimate_from_bytes(full_bytes);
    let core_claude_est = claude_estimate(core_tiktoken);
    let full_claude_est = claude_estimate(full_tiktoken);
    MetaToolTokenBudget {
        core_bytes,
        full_bytes,
        core_tiktoken,
        full_tiktoken,
        core_claude_est,
        full_claude_est,
        saved_tiktoken: full_tiktoken.saturating_sub(core_tiktoken),
        saved_claude_est: full_claude_est.saturating_sub(core_claude_est),
        core_rmcp_bytes,
        full_rmcp_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_tools_token_budget_report() {
        let budget = measure_meta_tool_token_budget();
        let line = format!(
            "META_TOOL_TOKEN_REPORT core_bytes={} full_bytes={} core_tiktoken={} full_tiktoken={} core_claude_est={} full_claude_est={} saved_tiktoken={} saved_claude_est={} core_rmcp_bytes={} full_rmcp_bytes={}",
            budget.core_bytes,
            budget.full_bytes,
            budget.core_tiktoken,
            budget.full_tiktoken,
            budget.core_claude_est,
            budget.full_claude_est,
            budget.saved_tiktoken,
            budget.saved_claude_est,
            budget.core_rmcp_bytes,
            budget.full_rmcp_bytes,
        );
        println!("{line}");

        assert_eq!(CORE_META_TOOLS.len(), 6);
        assert_eq!(ALL_REGISTERED_META_TOOL_NAMES.len(), 12);
        assert!(
            budget.core_claude_est < budget.full_claude_est,
            "core must be smaller than full: {budget:?}"
        );
        assert!(
            budget.saved_claude_est >= 500,
            "expected at least ~500 Claude-est token savings, got {budget:?}"
        );
        // Regression guardrails (re-measured via `pnpm count-tokens`, Jun 2026; limits doubled).
        assert!(
            budget.core_claude_est <= 3000,
            "slim core advertised budget grew unexpectedly: {budget:?}"
        );
        assert!(
            budget.full_claude_est <= 5200,
            "slim full registered budget grew unexpectedly: {budget:?}"
        );
    }

    #[test]
    fn core_tools_subset_of_registered() {
        for name in CORE_META_TOOLS {
            assert!(
                ALL_REGISTERED_META_TOOL_NAMES.contains(name),
                "{name} missing from ALL_REGISTERED_META_TOOL_NAMES"
            );
        }
    }
}
