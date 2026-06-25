//! Machine entity — a physical or logical host that reports workspace roots.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A registered machine (e.g. homelab box, laptop, cloud agent).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Machine {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Machine {
    /// Create a new machine with default timestamps and no icon/hostname.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            icon: None,
            hostname: None,
            created_at: now,
            updated_at: now,
        }
    }
}
