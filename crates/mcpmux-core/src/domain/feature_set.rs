//! FeatureSet entity - permission bundles for tools/prompts/resources
//!
//! Each featureset is scoped to a space and is one of two types:
//! - Default: auto-created per space; the fallback when no workspace binding applies
//! - Custom: user-defined composition of features and other featuresets

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The type of a FeatureSet.
///
/// `Default` is auto-created once per space and acts as the no-binding
/// fallback. `Custom` sets are always user-created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum FeatureSetType {
    /// Auto-created per space. The fallback FS when no WorkspaceBinding matches.
    Default,
    /// User-defined featureset.
    #[default]
    Custom,
}

impl FeatureSetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "default" => Some(Self::Default),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Mode for including or excluding a member
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MemberMode {
    #[default]
    Include,
    Exclude,
}

impl MemberMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::Exclude => "exclude",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "include" => Some(Self::Include),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }
}

/// Type of member in a featureset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberType {
    /// Another featureset (composition)
    FeatureSet,
    /// A specific feature (tool, prompt, or resource)
    Feature,
}

impl MemberType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FeatureSet => "feature_set",
            Self::Feature => "feature",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "feature_set" => Some(Self::FeatureSet),
            "feature" => Some(Self::Feature),
            _ => None,
        }
    }
}

/// A member of a featureset (either another featureset or a feature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSetMember {
    /// Unique identifier for this membership
    pub id: String,
    /// The featureset this member belongs to
    pub feature_set_id: String,
    /// Type of member
    pub member_type: MemberType,
    /// ID of the member (feature ID for Feature, featureset ID for FeatureSet)
    pub member_id: String,
    /// Include or exclude
    pub mode: MemberMode,
}

impl FeatureSetMember {
    /// Create a new member that includes a feature
    pub fn include_feature(feature_set_id: &str, feature_id: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.to_string(),
            member_type: MemberType::Feature,
            member_id: feature_id.to_string(),
            mode: MemberMode::Include,
        }
    }

    /// Create a new member that excludes a feature
    pub fn exclude_feature(feature_set_id: &str, feature_id: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.to_string(),
            member_type: MemberType::Feature,
            member_id: feature_id.to_string(),
            mode: MemberMode::Exclude,
        }
    }

    /// Create a new member that includes another featureset
    pub fn include_featureset(feature_set_id: &str, included_featureset_id: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            feature_set_id: feature_set_id.to_string(),
            member_type: MemberType::FeatureSet,
            member_id: included_featureset_id.to_string(),
            mode: MemberMode::Include,
        }
    }
}

/// FeatureSet defines a bundle of permissions using explicit feature selection.
///
/// Scoped to a space. Can contain other featuresets (composition) or specific
/// features (tools, prompts, resources). The `Default` type is auto-created per
/// space; its effective members can be edited by the user just like a Custom set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSet {
    /// Unique identifier
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Description of what this set allows
    pub description: Option<String>,

    /// Icon (emoji or URL)
    pub icon: Option<String>,

    /// The space this featureset belongs to (None = global/builtin)
    pub space_id: Option<String>,

    /// The type of featureset
    #[serde(default)]
    pub feature_set_type: FeatureSetType,

    /// For ServerAll type, the server ID
    pub server_id: Option<String>,

    /// Whether this is a built-in (non-editable) set
    pub is_builtin: bool,

    /// Soft delete flag
    #[serde(default)]
    pub is_deleted: bool,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Members (populated when fetching with members)
    #[serde(default)]
    pub members: Vec<FeatureSetMember>,
}

impl FeatureSet {
    /// Create a new custom FeatureSet for a space
    pub fn new_custom(name: impl Into<String>, space_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            description: None,
            icon: None,
            space_id: Some(space_id.into()),
            feature_set_type: FeatureSetType::Custom,
            server_id: None,
            is_builtin: false,
            is_deleted: false,
            created_at: now,
            updated_at: now,
            members: vec![],
        }
    }

    /// Create the "Default" featureset for a space.
    ///
    /// Uses a deterministic id (`fs_default_<space_id>`) so repositories can
    /// upsert this row without having to remember a mapping.
    pub fn new_default(space_id: impl Into<String>) -> Self {
        let space_id = space_id.into();
        let now = Utc::now();
        Self {
            id: format!("fs_default_{}", space_id),
            name: "Default".to_string(),
            description: Some("The fallback feature set for this space".to_string()),
            icon: Some("⭐".to_string()),
            space_id: Some(space_id),
            feature_set_type: FeatureSetType::Default,
            server_id: None,
            is_builtin: true,
            is_deleted: false,
            created_at: now,
            updated_at: now,
            members: vec![],
        }
    }

    /// Add description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add icon
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Check if this featureset is the "Default" type for a space
    pub fn is_default_type(&self) -> bool {
        self.feature_set_type == FeatureSetType::Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default_featureset() {
        let fs = FeatureSet::new_default("space_123");
        assert_eq!(fs.id, "fs_default_space_123");
        assert_eq!(fs.feature_set_type, FeatureSetType::Default);
        assert!(fs.is_builtin);
        assert!(fs.is_default_type());
    }

    #[test]
    fn test_new_custom_featureset() {
        let fs = FeatureSet::new_custom("My Custom Set", "space_123");
        assert_eq!(fs.feature_set_type, FeatureSetType::Custom);
        assert!(!fs.is_builtin);
    }

    #[test]
    fn test_feature_set_member() {
        let member = FeatureSetMember::include_feature("fs_123", "feature_abc");
        assert_eq!(member.member_type, MemberType::Feature);
        assert_eq!(member.mode, MemberMode::Include);
    }

    // FeatureSetType parse tests
    #[test]
    fn test_feature_set_type_parse() {
        assert_eq!(
            FeatureSetType::parse("default"),
            Some(FeatureSetType::Default)
        );
        assert_eq!(
            FeatureSetType::parse("custom"),
            Some(FeatureSetType::Custom)
        );
        assert_eq!(FeatureSetType::parse("invalid"), None);
        assert_eq!(FeatureSetType::parse(""), None);
        // Legacy variants no longer exist
        assert_eq!(FeatureSetType::parse("all"), None);
        assert_eq!(FeatureSetType::parse("server-all"), None);
    }

    #[test]
    fn test_feature_set_type_as_str() {
        assert_eq!(FeatureSetType::Default.as_str(), "default");
        assert_eq!(FeatureSetType::Custom.as_str(), "custom");
    }

    #[test]
    fn test_feature_set_type_roundtrip() {
        for fs_type in [FeatureSetType::Default, FeatureSetType::Custom] {
            let s = fs_type.as_str();
            let parsed = FeatureSetType::parse(s).expect("should parse");
            assert_eq!(parsed, fs_type);
        }
    }

    // MemberMode parse tests
    #[test]
    fn test_member_mode_parse() {
        assert_eq!(MemberMode::parse("include"), Some(MemberMode::Include));
        assert_eq!(MemberMode::parse("exclude"), Some(MemberMode::Exclude));
        assert_eq!(MemberMode::parse("invalid"), None);
    }

    #[test]
    fn test_member_mode_as_str() {
        assert_eq!(MemberMode::Include.as_str(), "include");
        assert_eq!(MemberMode::Exclude.as_str(), "exclude");
    }

    // MemberType parse tests
    #[test]
    fn test_member_type_parse() {
        assert_eq!(
            MemberType::parse("feature_set"),
            Some(MemberType::FeatureSet)
        );
        assert_eq!(MemberType::parse("feature"), Some(MemberType::Feature));
        assert_eq!(MemberType::parse("invalid"), None);
    }

    #[test]
    fn test_member_type_as_str() {
        assert_eq!(MemberType::FeatureSet.as_str(), "feature_set");
        assert_eq!(MemberType::Feature.as_str(), "feature");
    }

    // Member construction tests
    #[test]
    fn test_exclude_feature_member() {
        let member = FeatureSetMember::exclude_feature("fs_123", "feature_xyz");
        assert_eq!(member.feature_set_id, "fs_123");
        assert_eq!(member.member_id, "feature_xyz");
        assert_eq!(member.member_type, MemberType::Feature);
        assert_eq!(member.mode, MemberMode::Exclude);
    }

    #[test]
    fn test_include_featureset_member() {
        let member = FeatureSetMember::include_featureset("fs_parent", "fs_child");
        assert_eq!(member.feature_set_id, "fs_parent");
        assert_eq!(member.member_id, "fs_child");
        assert_eq!(member.member_type, MemberType::FeatureSet);
        assert_eq!(member.mode, MemberMode::Include);
    }

    // Builder pattern tests
    #[test]
    fn test_featureset_with_description() {
        let fs = FeatureSet::new_custom("Test", "space").with_description("A test description");
        assert_eq!(fs.description, Some("A test description".to_string()));
    }

    #[test]
    fn test_featureset_with_icon() {
        let fs = FeatureSet::new_custom("Test", "space").with_icon("🔧");
        assert_eq!(fs.icon, Some("🔧".to_string()));
    }

    #[test]
    fn test_featureset_chained_builders() {
        let fs = FeatureSet::new_custom("Test", "space")
            .with_icon("🔧")
            .with_description("Tools for testing");

        assert_eq!(fs.name, "Test");
        assert_eq!(fs.icon, Some("🔧".to_string()));
        assert_eq!(fs.description, Some("Tools for testing".to_string()));
        assert_eq!(fs.space_id, Some("space".to_string()));
    }
}
