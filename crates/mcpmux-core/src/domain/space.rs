//! Space entity - isolated environment for MCP configuration

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Space represents an isolated environment with its own credentials and server configs.
///
/// Examples: "Work", "Personal", "Client Project"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    /// Unique identifier
    pub id: Uuid,

    /// Human-readable name
    pub name: String,

    /// Optional emoji or icon URL
    pub icon: Option<String>,

    /// Description of the space
    pub description: Option<String>,

    /// Whether this is the default space
    pub is_default: bool,

    /// Sort order for display
    pub sort_order: i32,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl Space {
    /// Create a new space with default values
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            icon: None,
            description: None,
            is_default: false,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a new space with an icon
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Create a new space with a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Mark as default space
    pub fn set_default(mut self) -> Self {
        self.is_default = true;
        self
    }
}

impl Default for Space {
    fn default() -> Self {
        Self::new("Default")
    }
}

/// A base directory claimed by a Space.
///
/// Any reported workspace root at or under `path` is scoped to `space_id`
/// (see [`crate::domain::path_is_within`]). `path` is stored already-normalized
/// via [`crate::domain::normalize_workspace_root`], and is globally unique —
/// the same folder can't be a base dir of two Spaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceBaseDir {
    /// Unique identifier for this base-dir row.
    pub id: String,
    /// The Space that owns this base directory.
    pub space_id: String,
    /// Normalized absolute path.
    pub path: String,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_space_creation() {
        let space = Space::new("Work")
            .with_icon("💼")
            .with_description("Work projects");

        assert_eq!(space.name, "Work");
        assert_eq!(space.icon, Some("💼".to_string()));
        assert!(!space.is_default);
    }
}
