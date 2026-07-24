//! WorkspaceAppearance entity for unmapped workspace roots.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Appearance metadata keyed by normalized workspace root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceAppearance {
    pub workspace_root: String,
    pub icon: String,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceAppearance {
    /// Create a new workspace appearance record.
    pub fn new(workspace_root: impl Into<String>, icon: impl Into<String>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            icon: icon.into(),
            updated_at: Utc::now(),
        }
    }
}
