//! In-memory tool index for meta-gateway search and schema lookup.
//!
//! Built from Space [`ServerFeature`] rows and filtered to the caller's
//! invokable tool set before search/schema operations run.

use std::sync::Arc;

use mcpmux_core::ServerFeatureRepository;

#[path = "tool_discovery_index.rs"]
mod tool_discovery_index;
#[path = "tool_discovery_search.rs"]
mod tool_discovery_search;
#[path = "tool_discovery_types.rs"]
mod tool_discovery_types;

pub use tool_discovery_index::entry_content_hash;
pub use tool_discovery_types::{
    DetailLevel, SearchContext, SearchToolsResult, ToolIndex, ToolIndexEntry,
};

/// Service that builds and queries a tool index for a Space.
pub struct ToolDiscoveryService {
    server_feature_repo: Arc<dyn ServerFeatureRepository>,
}

impl ToolDiscoveryService {
    /// Create a discovery service backed by the Space feature repository.
    pub fn new(server_feature_repo: Arc<dyn ServerFeatureRepository>) -> Self {
        Self {
            server_feature_repo,
        }
    }
}

#[cfg(test)]
#[path = "tool_discovery_tests.rs"]
mod tests;
