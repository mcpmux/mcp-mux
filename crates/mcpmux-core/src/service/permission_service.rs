//! Permission Service - resolves effective features from granted feature sets
//!
//! This service computes which features a client can access based on their
//! granted feature sets and the feature set composition rules.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::domain::{FeatureSet, FeatureSetType, MemberMode, MemberType, ServerFeature};
use crate::repository::{
    InboundMcpClientRepository, FeatureSetRepository, ServerFeatureRepository,
};

/// Resolved permissions for a client in a space
#[derive(Debug, Clone, Default)]
pub struct ResolvedPermissions {
    /// Feature IDs that are allowed (from server_features table)
    pub allowed_feature_ids: HashSet<String>,
    /// Whether this permission set grants all features
    pub grants_all: bool,
    /// Server IDs that grant all features (for server-all type)
    pub all_from_servers: HashSet<String>,
}

impl ResolvedPermissions {
    /// Check if a feature is allowed
    pub fn allows_feature(&self, feature_id: &str, server_id: Option<&str>) -> bool {
        if self.grants_all {
            return true;
        }
        if let Some(sid) = server_id {
            if self.all_from_servers.contains(sid) {
                return true;
            }
        }
        self.allowed_feature_ids.contains(feature_id)
    }

    /// Check if a tool is allowed by name and server
    pub fn allows_tool(&self, tool_name: &str, server_id: &str) -> bool {
        if self.grants_all {
            return true;
        }
        if self.all_from_servers.contains(server_id) {
            return true;
        }
        // Check by qualified name (server_id/tool_name)
        let qualified = format!("{}/{}", server_id, tool_name);
        self.allowed_feature_ids.contains(&qualified)
    }
}

/// Service for resolving permissions
pub struct PermissionService {
    client_repository: Arc<dyn InboundMcpClientRepository>,
    feature_set_repository: Arc<dyn FeatureSetRepository>,
    server_feature_repository: Arc<dyn ServerFeatureRepository>,
}

impl PermissionService {
    /// Create a new permission service
    pub fn new(
        client_repository: Arc<dyn InboundMcpClientRepository>,
        feature_set_repository: Arc<dyn FeatureSetRepository>,
        server_feature_repository: Arc<dyn ServerFeatureRepository>,
    ) -> Self {
        Self {
            client_repository,
            feature_set_repository,
            server_feature_repository,
        }
    }

    /// Resolve effective permissions for a client in a space
    pub async fn resolve_permissions(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> Result<ResolvedPermissions> {
        let mut result = ResolvedPermissions::default();

        // Get granted feature set IDs
        let granted_ids = self
            .client_repository
            .get_grants_for_space(client_id, space_id)
            .await?;

        if granted_ids.is_empty() {
            debug!(
                client_id = %client_id,
                space_id = %space_id,
                "No grants found for client"
            );
            return Ok(result);
        }

        // Resolve each feature set
        for fs_id in &granted_ids {
            self.resolve_feature_set(fs_id, space_id, &mut result, &mut HashSet::new())
                .await?;
        }

        debug!(
            client_id = %client_id,
            space_id = %space_id,
            grants_all = %result.grants_all,
            feature_count = %result.allowed_feature_ids.len(),
            server_all_count = %result.all_from_servers.len(),
            "Resolved permissions"
        );

        Ok(result)
    }

    /// Recursively resolve a feature set
    fn resolve_feature_set<'a>(
        &'a self,
        feature_set_id: &'a str,
        space_id: &'a str,
        result: &'a mut ResolvedPermissions,
        visited: &'a mut HashSet<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
        // Prevent infinite recursion
        if visited.contains(feature_set_id) {
            warn!(
                feature_set_id = %feature_set_id,
                "Circular reference detected in feature set composition"
            );
            return Ok(());
        }
        visited.insert(feature_set_id.to_string());

        // Get the feature set with members
        let feature_set = match self
            .feature_set_repository
            .get_with_members(feature_set_id)
            .await?
        {
            Some(fs) => fs,
            None => {
                warn!(
                    feature_set_id = %feature_set_id,
                    "Feature set not found"
                );
                return Ok(());
            }
        };

        // Handle based on type
        match feature_set.feature_set_type {
            FeatureSetType::All => {
                // All features in the space
                result.grants_all = true;
                debug!(feature_set_id = %feature_set_id, "Resolved as All type - grants all");
            }
            FeatureSetType::Default => {
                // Resolve members of the Default set
                self.resolve_members(&feature_set, space_id, result, visited)
                    .await?;
            }
            FeatureSetType::ServerAll => {
                // All features from a specific server
                if let Some(ref server_id) = feature_set.server_id {
                    result.all_from_servers.insert(server_id.clone());
                    debug!(
                        feature_set_id = %feature_set_id,
                        server_id = %server_id,
                        "Resolved as ServerAll type"
                    );
                }
            }
            FeatureSetType::Custom => {
                // Resolve members recursively
                self.resolve_members(&feature_set, space_id, result, visited)
                    .await?;
            }
        }

        Ok(())
        })
    }

    /// Resolve members of a feature set
    fn resolve_members<'a>(
        &'a self,
        feature_set: &'a FeatureSet,
        space_id: &'a str,
        result: &'a mut ResolvedPermissions,
        visited: &'a mut HashSet<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            for member in &feature_set.members {
                match member.mode {
                    MemberMode::Include => {
                        match member.member_type {
                            MemberType::FeatureSet => {
                                // Recursively resolve nested feature set
                                self.resolve_feature_set(&member.member_id, space_id, result, visited)
                                    .await?;
                            }
                            MemberType::Feature => {
                                // Add individual feature
                                result.allowed_feature_ids.insert(member.member_id.clone());
                            }
                        }
                    }
                    MemberMode::Exclude => {
                        // For exclusions, remove from allowed set
                        result.allowed_feature_ids.remove(&member.member_id);
                    }
                }
            }
            Ok(())
        })
    }

    /// Get all allowed features for a client in a space
    pub async fn get_allowed_features(
        &self,
        client_id: &Uuid,
        space_id: &str,
    ) -> Result<Vec<ServerFeature>> {
        let permissions = self.resolve_permissions(client_id, space_id).await?;

        // Get all features in the space
        let all_features = self
            .server_feature_repository
            .list_for_space(space_id)
            .await?;

        // Filter based on permissions
        let allowed: Vec<ServerFeature> = if permissions.grants_all {
            all_features
        } else {
            all_features
                .into_iter()
                .filter(|f| {
                    permissions.allows_feature(&f.id.to_string(), Some(&f.server_id))
                        || permissions.all_from_servers.contains(&f.server_id)
                        || permissions.allowed_feature_ids.contains(&f.id.to_string())
                })
                .collect()
        };

        Ok(allowed)
    }

    /// Check if a client can access a specific tool
    pub async fn can_access_tool(
        &self,
        client_id: &Uuid,
        space_id: &str,
        tool_name: &str,
        server_id: &str,
    ) -> Result<bool> {
        let permissions = self.resolve_permissions(client_id, space_id).await?;
        Ok(permissions.allows_tool(tool_name, server_id))
    }
}
