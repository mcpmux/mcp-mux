//! Feature Resolution Service - SRP: Feature set resolution & permissions

use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

use crate::services::PrefixCacheService;
use mcpmux_core::{
    FeatureSet, FeatureSetRepository, FeatureSetType, FeatureType, MemberMode, MemberType,
    ServerFeature, ServerFeatureRepository,
};

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
        let mut has_all_grant = false;

        let all_features = self.feature_repo.list_for_space(space_id).await?;

        debug!(
            "[FeatureResolution] Resolving {} feature sets for space {}",
            feature_set_ids.len(),
            space_id
        );

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

            match feature_set.feature_set_type {
                FeatureSetType::All => {
                    has_all_grant = true;
                }
                FeatureSetType::Default => {
                    // Default feature set uses explicit members only
                    // Empty default = no features (secure by default)
                    self.resolve_members(
                        &feature_set,
                        &all_features,
                        &mut allowed_feature_ids,
                        &mut excluded_feature_ids,
                    )
                    .await?;
                }
                FeatureSetType::ServerAll => {
                    if let Some(ref server_id) = feature_set.server_id {
                        debug!(
                            "[FeatureResolution] ServerAll: querying features for server_id={} in space={}",
                            server_id, space_id
                        );
                        let server_features = self
                            .feature_repo
                            .list_for_server(space_id, server_id)
                            .await?;
                        debug!(
                            "[FeatureResolution] ServerAll: found {} features for server {}",
                            server_features.len(),
                            server_id
                        );
                        for f in &server_features {
                            debug!(
                                "[FeatureResolution] ServerAll: adding feature id={}, name={}, available={}",
                                f.id, f.feature_name, f.is_available
                            );
                            allowed_feature_ids.insert(f.id.to_string());
                        }
                    } else {
                        debug!("[FeatureResolution] ServerAll: feature_set.server_id is None!");
                    }
                }
                FeatureSetType::Custom => {
                    self.resolve_members(
                        &feature_set,
                        &all_features,
                        &mut allowed_feature_ids,
                        &mut excluded_feature_ids,
                    )
                    .await?;
                }
            }
        }

        // Apply filters
        debug!(
            "[FeatureResolution] Filtering: has_all_grant={}, all_features={}, allowed_ids={}, excluded_ids={}",
            has_all_grant, all_features.len(), allowed_feature_ids.len(), excluded_feature_ids.len()
        );

        let mut result: Vec<ServerFeature> = if has_all_grant {
            all_features
                .into_iter()
                .filter(|f| f.is_available)
                .collect()
        } else {
            all_features
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
                .collect()
        };

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

    async fn resolve_members(
        &self,
        feature_set: &FeatureSet,
        all_features: &[ServerFeature],
        allowed: &mut HashSet<String>,
        excluded: &mut HashSet<String>,
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
                    if let Some(nested_fs) = self
                        .feature_set_repo
                        .get_with_members(&member.member_id)
                        .await?
                    {
                        match nested_fs.feature_set_type {
                            FeatureSetType::All => {
                                let ids = all_features
                                    .iter()
                                    .filter(|f| f.is_available)
                                    .map(|f| f.id.to_string());
                                apply_mode_to_set(member.mode, ids, allowed, excluded);
                            }
                            FeatureSetType::ServerAll => {
                                if let Some(ref server_id) = nested_fs.server_id {
                                    let ids = all_features
                                        .iter()
                                        .filter(|f| f.server_id == *server_id && f.is_available)
                                        .map(|f| f.id.to_string());
                                    apply_mode_to_set(member.mode, ids, allowed, excluded);
                                }
                            }
                            _ => {
                                Box::pin(self.resolve_members(
                                    &nested_fs,
                                    all_features,
                                    allowed,
                                    excluded,
                                ))
                                .await?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
