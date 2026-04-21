//! Integration tests for McpMux core flows
//!
//! Tests the complete inbound/outbound MCP flows:
//! - Feature grant resolution (Space → FeatureSet → Features)
//! - Feature routing (qualified names, prefix resolution)
//! - MCP request handling (tools, resources, prompts)
//!
//! NOTE: Authorization tests that require InboundClientRepository
//! are in the database tests since they need the real SQLite implementation.

mod feature_grants;
mod feature_routing;
mod feature_set_resolver;
mod mcp_flows;
mod meta_tools;
