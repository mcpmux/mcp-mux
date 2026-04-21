//! Before/after diff of the caller's resolved tool list.
//!
//! Used by write meta tools to build a concrete "you'll go from N tools to
//! M tools" preview for the approval dialog.

use serde::Serialize;
use uuid::Uuid;

use crate::pool::FeatureService;

/// Tool-list diff between two FeatureSet resolutions, both relative to the
/// same Space. Every field is a list of fully-qualified tool names
/// (e.g. `github.create_issue`).
#[derive(Debug, Clone, Serialize, Default)]
pub struct ToolDiff {
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl ToolDiff {
    /// Compute `after − before` for the caller's Space, each given as an
    /// optional FeatureSet id. `None` means "deny" (empty toolset), which
    /// is a valid before/after state.
    ///
    /// Uses the shared [`FeatureService`] so the math matches what the
    /// client actually receives on a subsequent `list_tools` call.
    pub async fn compute(
        feature_service: &FeatureService,
        space_id: Uuid,
        before_fs_id: Option<Uuid>,
        after_fs_id: Option<Uuid>,
    ) -> anyhow::Result<ToolDiff> {
        let before = Self::tools_for(feature_service, space_id, before_fs_id).await?;
        let after = Self::tools_for(feature_service, space_id, after_fs_id).await?;

        let before_set: std::collections::HashSet<&String> = before.iter().collect();
        let after_set: std::collections::HashSet<&String> = after.iter().collect();
        let added: Vec<String> = after
            .iter()
            .filter(|t| !before_set.contains(t))
            .cloned()
            .collect();
        let removed: Vec<String> = before
            .iter()
            .filter(|t| !after_set.contains(t))
            .cloned()
            .collect();

        Ok(ToolDiff {
            before,
            after,
            added,
            removed,
        })
    }

    async fn tools_for(
        feature_service: &FeatureService,
        space_id: Uuid,
        fs_id: Option<Uuid>,
    ) -> anyhow::Result<Vec<String>> {
        let Some(fs) = fs_id else { return Ok(vec![]) };
        let space_id_str = space_id.to_string();
        let ids = [fs.to_string()];
        let features = feature_service
            .get_tools_for_grants(&space_id_str, &ids)
            .await?;
        Ok(features
            .iter()
            .filter(|f| f.is_available)
            .map(|f| f.qualified_name())
            .collect())
    }
}
