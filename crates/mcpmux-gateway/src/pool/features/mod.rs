//! Feature Services - Modular architecture for MCP feature management
//!
//! Each service has its own file following SRP.

mod conversion;
mod discovery;
mod resolution;
mod routing;
mod facade;

// Re-export public types
pub use conversion::{convert_to_feature, resource_to_feature};
pub use discovery::FeatureDiscoveryService;
pub use resolution::FeatureResolutionService;
pub use routing::FeatureRoutingService;
pub use facade::FeatureService;

use mcpmux_core::ServerFeature;

/// Discovered features from an MCP server connection
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CachedFeatures {
    pub tools: Vec<ServerFeature>,
    pub prompts: Vec<ServerFeature>,
    pub resources: Vec<ServerFeature>,
}

impl CachedFeatures {
    pub fn total_count(&self) -> usize {
        self.tools.len() + self.prompts.len() + self.resources.len()
    }
    
    pub fn all_features(&self) -> Vec<ServerFeature> {
        let mut all = Vec::with_capacity(self.total_count());
        all.extend(self.tools.iter().cloned());
        all.extend(self.prompts.iter().cloned());
        all.extend(self.resources.iter().cloned());
        all
    }
}


