//! Feature Resolution Service - SRP: Feature set resolution & permissions

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

use crate::services::PrefixCacheService;
use mcpmux_core::{
    FeatureSet, FeatureSetRepository, FeatureType, MemberMode, MemberType, ServerFeature,
    ServerFeatureRepository,
};

/// A catalog tool visible in discovery but not invokable until its FeatureSet is bound.
#[derive(Debug, Clone)]
pub struct InactiveDiscoveryEntry {
    pub feature: ServerFeature,
    pub bindable_feature_set_id: String,
}

/// Helper to apply include/exclude mode (DRY)
fn apply_mode_to_set(
    mode: MemberMode,
    feature_ids: impl Iterator<Item = String>,
    allowed: &mut HashSet<String>,
    excluded: &mut HashSet<String>,
) {
    match mode {
        MemberMode::Include => allowed.extend(feature_ids),
        MemberMode::Exclude => excluded.extend(feature_ids),
    }
}

/// Handles feature set resolution and permission evaluation
pub struct FeatureResolutionService {
    feature_repo: Arc<dyn ServerFeatureRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    prefix_cache: Arc<PrefixCacheService>,
}

impl FeatureResolutionService {
    pub fn new(
        feature_repo: Arc<dyn ServerFeatureRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        prefix_cache: Arc<PrefixCacheService>,
    ) -> Self {
        Self {
            feature_repo,
            feature_set_repo,
            prefix_cache,
        }
    }

    /// Get all available features for a space (optionally filtered by type)
    pub async fn get_all_features_for_space(
        &self,
        space_id: &str,
        filter_type: Option<FeatureType>,
    ) -> Result<Vec<ServerFeature>> {
        let all_features = self.feature_repo.list_for_space(space_id).await?;

        let mut result: Vec<ServerFeature> = all_features
            .into_iter()
            .filter(|f| f.is_available)
            .collect();

        if let Some(feature_type) = filter_type {
            result.retain(|f| f.feature_type == feature_type);
        }

        // Enrich with prefixes
        for feature in &mut result {
            let prefix = self
                .prefix_cache
                .get_prefix_for_server(space_id, &feature.server_id)
                .await;
            feature.server_alias = Some(prefix);
        }

        Ok(result)
    }

    /// Resolve feature set IDs to actual features (with optional type filter)
    pub async fn resolve_feature_sets(
        &self,
        space_id: &str,
        feature_set_ids: &[String],
        filter_type: Option<FeatureType>,
    ) -> Result<Vec<ServerFeature>> {
        let mut allowed_feature_ids: HashSet<String> = HashSet::new();
        let mut excluded_feature_ids: HashSet<String> = HashSet::new();

        let all_features = self.feature_repo.list_for_space(space_id).await?;

        debug!(
            "[FeatureResolution] Resolving {} feature sets for space {}",
            feature_set_ids.len(),
            space_id
        );

        // Tracks FeatureSet ids already entered on this resolution so a
        // composition cycle (A includes B, B includes A — creatable via two
        // legal UI calls, see add_feature_set_member's cycle guard) can't
        // recurse forever and hang/OOM the gateway on every list/call.
        let mut visited: HashSet<String> = HashSet::new();

        for fs_id in feature_set_ids {
            let feature_set = match self.feature_set_repo.get_with_members(fs_id).await? {
                Some(fs) => {
                    debug!(
                        "[FeatureResolution] Found feature set: id={}, type={:?}, server_id={:?}",
                        fs.id, fs.feature_set_type, fs.server_id
                    );
                    fs
                }
                None => {
                    debug!("[FeatureResolution] FeatureSet {} not found", fs_id);
                    continue;
                }
            };

            if !visited.insert(fs_id.clone()) {
                continue;
            }

            // Both Default and Custom sets use explicit members; the
            // resolution is identical — walk the members and build up
            // allow/exclude sets.
            self.resolve_members(
                &feature_set,
                &all_features,
                &mut allowed_feature_ids,
                &mut excluded_feature_ids,
                &mut visited,
            )
            .await?;
        }

        debug!(
            "[FeatureResolution] Filtering: all_features={}, allowed_ids={}, excluded_ids={}",
            all_features.len(),
            allowed_feature_ids.len(),
            excluded_feature_ids.len()
        );

        let mut result: Vec<ServerFeature> = all_features
            .into_iter()
            .filter(|f| {
                let in_allowed = allowed_feature_ids.contains(&f.id.to_string());
                let in_excluded = excluded_feature_ids.contains(&f.id.to_string());
                let passes = f.is_available && in_allowed && !in_excluded;
                if !passes && in_allowed {
                    debug!(
                        "[FeatureResolution] Feature {} (server={}) filtered out: is_available={}, in_allowed={}, in_excluded={}",
                        f.feature_name, f.server_id, f.is_available, in_allowed, in_excluded
                    );
                }
                passes
            })
            .collect();

        debug!(
            "[FeatureResolution] After filter: {} features",
            result.len()
        );

        // Apply type filter if specified (OCP)
        if let Some(feature_type) = filter_type {
            result.retain(|f| f.feature_type == feature_type);
        }

        // Enrich with prefixes
        for feature in &mut result {
            let prefix = self
                .prefix_cache
                .get_prefix_for_server(space_id, &feature.server_id)
                .await;
            feature.server_alias = Some(prefix);
        }

        Ok(result)
    }

    /// Resolve the set of feature IDs marked `surfaced` (promoted into the
    /// client's `tools/list`) across the given FeatureSets, recursing into
    /// nested sets.
    pub async fn resolve_surfaced_feature_ids(
        &self,
        feature_set_ids: &[String],
    ) -> Result<HashSet<String>> {
        let mut surfaced = HashSet::new();
        for fs_id in feature_set_ids {
            let Some(feature_set) = self.feature_set_repo.get_with_members(fs_id).await? else {
                continue;
            };
            self.collect_surfaced_members(&feature_set, &mut surfaced)
                .await?;
        }
        Ok(surfaced)
    }

    async fn collect_surfaced_members(
        &self,
        feature_set: &FeatureSet,
        surfaced: &mut HashSet<String>,
    ) -> Result<()> {
        for member in &feature_set.members {
            match member.member_type {
                MemberType::Feature => {
                    if member.mode == MemberMode::Include && member.surfaced {
                        surfaced.insert(member.member_id.clone());
                    }
                }
                MemberType::FeatureSet => {
                    if let Some(nested_fs) = self
                        .feature_set_repo
                        .get_with_members(&member.member_id)
                        .await?
                    {
                        Box::pin(self.collect_surfaced_members(&nested_fs, surfaced)).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Tools granted by some FeatureSet in the Space but not in `invokable_keys`.
    ///
    /// Used by meta-tool discovery (`include_inactive`); first matching FeatureSet
    /// wins when multiple bundles contain the same tool.
    pub async fn list_inactive_tools_for_discovery(
        &self,
        space_id: &str,
        invokable_keys: &HashSet<(String, String)>,
        query_id: Option<&str>,
    ) -> Result<Vec<InactiveDiscoveryEntry>> {
        let started = Instant::now();

        let all_features = self.feature_repo.list_for_space(space_id).await?;
        let features_by_id: HashMap<String, ServerFeature> = all_features
            .iter()
            .map(|feature| (feature.id.to_string(), feature.clone()))
            .collect();

        let sets = self.feature_set_repo.list_by_space(space_id).await?;
        let mut sets: Vec<_> = sets.into_iter().filter(|fs| !fs.is_deleted).collect();
        // Prefer custom bundles over the auto-seeded Default when both grant the same tool.
        sets.sort_by(|a, b| {
            a.is_builtin
                .cmp(&b.is_builtin)
                .then_with(|| a.name.cmp(&b.name))
        });

        let mut by_key: HashMap<(String, String), InactiveDiscoveryEntry> = HashMap::new();

        // Pass 1: flat `feature` include members (hot path — equivalent to the JOIN scan).
        for fs in &sets {
            Self::collect_inactive_from_flat_includes(
                &mut by_key,
                &features_by_id,
                fs,
                invokable_keys,
            );
        }

        // Pass 2: nested FeatureSet members and exclude rules (rare composed bundles).
        for fs in &sets {
            if !Self::feature_set_needs_resolution_pass(fs) {
                continue;
            }
            let mut allowed_feature_ids: HashSet<String> = HashSet::new();
            let mut excluded_feature_ids: HashSet<String> = HashSet::new();
            let mut visited: HashSet<String> = HashSet::new();
            self.resolve_members(
                fs,
                &all_features,
                &mut allowed_feature_ids,
                &mut excluded_feature_ids,
                &mut visited,
            )
            .await?;
            Self::merge_inactive_from_feature_ids(
                &mut by_key,
                &features_by_id,
                &allowed_feature_ids,
                &excluded_feature_ids,
                &fs.id,
                invokable_keys,
            );
        }

        let mut entries: Vec<_> = by_key.into_values().collect();
        for entry in &mut entries {
            let prefix = self
                .prefix_cache
                .get_prefix_for_server(space_id, &entry.feature.server_id)
                .await;
            entry.feature.server_alias = Some(prefix);
        }

        debug!(
            query_id,
            inactive_entries = entries.len(),
            total_ms = started.elapsed().as_millis() as u64,
            "[search] inactive scan complete"
        );

        entries.sort_by_key(|entry| entry.feature.qualified_name());
        Ok(entries)
    }

    /// Whether a FeatureSet needs the second-pass member-resolution walk.
    fn feature_set_needs_resolution_pass(feature_set: &FeatureSet) -> bool {
        feature_set.members.iter().any(|member| {
            member.member_type == MemberType::FeatureSet
                || (member.member_type == MemberType::Feature && member.mode == MemberMode::Exclude)
        })
    }

    /// Collect inactive tools from flat `feature` include members on one FeatureSet.
    fn collect_inactive_from_flat_includes(
        by_key: &mut HashMap<(String, String), InactiveDiscoveryEntry>,
        features_by_id: &HashMap<String, ServerFeature>,
        feature_set: &FeatureSet,
        invokable_keys: &HashSet<(String, String)>,
    ) {
        for member in &feature_set.members {
            if member.member_type != MemberType::Feature || member.mode != MemberMode::Include {
                continue;
            }
            let Some(feature) = features_by_id.get(&member.member_id) else {
                continue;
            };
            if !feature.is_available || feature.feature_type != FeatureType::Tool {
                continue;
            }
            let key = (feature.server_id.clone(), feature.feature_name.clone());
            if invokable_keys.contains(&key) {
                continue;
            }
            by_key.entry(key).or_insert_with(|| InactiveDiscoveryEntry {
                feature: feature.clone(),
                bindable_feature_set_id: feature_set.id.clone(),
            });
        }
    }

    /// Merge inactive tools from resolved feature IDs; first FeatureSet row wins.
    fn merge_inactive_from_feature_ids(
        by_key: &mut HashMap<(String, String), InactiveDiscoveryEntry>,
        features_by_id: &HashMap<String, ServerFeature>,
        allowed_feature_ids: &HashSet<String>,
        excluded_feature_ids: &HashSet<String>,
        bindable_feature_set_id: &str,
        invokable_keys: &HashSet<(String, String)>,
    ) {
        for feature_id in allowed_feature_ids {
            if excluded_feature_ids.contains(feature_id) {
                continue;
            }
            let Some(feature) = features_by_id.get(feature_id) else {
                continue;
            };
            if !feature.is_available || feature.feature_type != FeatureType::Tool {
                continue;
            }
            let key = (feature.server_id.clone(), feature.feature_name.clone());
            if invokable_keys.contains(&key) {
                continue;
            }
            by_key.entry(key).or_insert_with(|| InactiveDiscoveryEntry {
                feature: feature.clone(),
                bindable_feature_set_id: bindable_feature_set_id.to_string(),
            });
        }
    }

    async fn resolve_members(
        &self,
        feature_set: &FeatureSet,
        all_features: &[ServerFeature],
        allowed: &mut HashSet<String>,
        excluded: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        for member in &feature_set.members {
            match member.member_type {
                MemberType::Feature => {
                    apply_mode_to_set(
                        member.mode,
                        std::iter::once(member.member_id.clone()),
                        allowed,
                        excluded,
                    );
                }
                MemberType::FeatureSet => {
                    // Composition: recurse into the nested FS, walking its
                    // members the same way. Both Default and Custom sets
                    // are purely member-driven now. `visited` breaks cycles
                    // (and prunes diamond re-visits) so a cyclic composition
                    // can't loop forever on this hot routing path.
                    if !visited.insert(member.member_id.clone()) {
                        warn!(
                            "[FeatureResolution] Skipping already-visited FeatureSet {} (composition cycle or diamond)",
                            member.member_id
                        );
                        continue;
                    }
                    if let Some(nested_fs) = self
                        .feature_set_repo
                        .get_with_members(&member.member_id)
                        .await?
                    {
                        Box::pin(self.resolve_members(
                            &nested_fs,
                            all_features,
                            allowed,
                            excluded,
                            visited,
                        ))
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }
}
