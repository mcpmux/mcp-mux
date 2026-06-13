//! Built-in MCP servers that McpMux ships itself.
//!
//! Distinct from user-installed servers ("My Servers") — these are bundled and
//! exposed through the gateway. Each built-in server is enabled/disabled and
//! has its individual tools toggled **per Space** (see
//! [`crate::SpaceBuiltinConfigRepository`]).
//!
//! Today there is one concrete built-in server — "Tool Optimization" (the
//! `mcpmux_*` self-management tools). Memory / Skills / Plugins are planned and
//! slot into the same framework.
//!
//! These descriptors are the single source of truth for built-in server ids,
//! names, and which tools belong to which server. The gateway registers the
//! executable tool implementations against the same ids; the desktop UI renders
//! per-Space toggles from these descriptors.

/// Stable id for the self-management ("Tool Optimization") built-in server.
/// Used as the per-Space config key and to match the gateway's registered
/// `mcpmux_*` tools.
pub const TOOL_OPTIMIZATION_SERVER_ID: &str = "tool-optimization";

/// One tool exposed by a built-in server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinToolDescriptor {
    /// Wire name the client sees (e.g. `mcpmux_list_all_tools`).
    pub name: &'static str,
    /// Short human description.
    pub description: &'static str,
    /// Whether the tool mutates state (gated behind a native approval dialog).
    pub write: bool,
}

/// A built-in MCP server bundled with McpMux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinServerDescriptor {
    /// Stable id, used as the per-Space config key.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// One-line description.
    pub description: &'static str,
    /// Whether this server is enabled when a Space has no stored override.
    pub default_enabled: bool,
    /// The tools this server exposes.
    pub tools: Vec<BuiltinToolDescriptor>,
}

/// Every built-in server McpMux ships. The single source of truth for ids and
/// tool sets — keep the gateway registry and the desktop UI in sync via this.
pub fn builtin_servers() -> Vec<BuiltinServerDescriptor> {
    vec![BuiltinServerDescriptor {
        id: TOOL_OPTIMIZATION_SERVER_ID,
        name: "Tool Optimization",
        description: "Lets the AI keep its toolset lean: browse every available tool, \
                      assemble a focused feature set, and pin it to the current folder — \
                      each with your approval. Reads are silent; writes need approval.",
        default_enabled: true,
        tools: vec![
            BuiltinToolDescriptor {
                name: "mcpmux_list_all_tools",
                description: "Browse every tool available in the resolved Space, unfiltered.",
                write: false,
            },
            BuiltinToolDescriptor {
                name: "mcpmux_list_feature_sets",
                description: "See the feature sets defined in the Space.",
                write: false,
            },
            BuiltinToolDescriptor {
                name: "mcpmux_manage_feature_set",
                description: "Create, update, or delete a custom feature set of chosen tools.",
                write: true,
            },
            BuiltinToolDescriptor {
                name: "mcpmux_bind_current_workspace",
                description:
                    "Map the current folder to a feature set so it persists (re-run to rebind).",
                write: true,
            },
        ],
    }]
}

/// Look up a built-in server descriptor by id.
pub fn builtin_server(id: &str) -> Option<BuiltinServerDescriptor> {
    builtin_servers().into_iter().find(|s| s.id == id)
}
