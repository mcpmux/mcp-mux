//! Grant Service.
//!
//! Two responsibilities, both centred on emitting domain events so MCPNotifier
//! can broadcast `list_changed` notifications:
//!
//! 1. **Per-client FeatureSet grants** — used by the resolver's rootless-fallback
//!    path. When a client has not declared the MCP `roots` capability (or has
//!    no workspace context), the resolver consults `client_grants` for that
//!    `(client_id, space_id)` pair. Grant/revoke flows here update the table
//!    *and* fire `ClientGrantChanged` so any open peer for that client
//!    re-fetches its tool list under the new permission set.
//! 2. **FeatureSet membership change broadcast** — when individual features are
//!    added or removed inside a FeatureSet, fire `FeatureSetMembersChanged`
//!    for the same notifier path.
//!
//! Routing for roots-capable clients flows through `WorkspaceBinding` and is
//! handled by the resolver directly — this service is not on that path.

use anyhow::Result;
use mcpmux_core::{DomainEvent, FeatureSetRepository};
use mcpmux_storage::InboundClientRepository;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

/// Grant management with automatic event emission.
pub struct GrantService {
    /// OAuth client grant repository (concrete; storage-owned).
    client_repo: Arc<InboundClientRepository>,
    /// Feature set lookup for member-change notifications.
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    /// Domain event broadcaster.
    event_tx: broadcast::Sender<DomainEvent>,
}

impl GrantService {
    pub fn new(
        client_repo: Arc<InboundClientRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        event_tx: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            client_repo,
            feature_set_repo,
            event_tx,
        }
    }

    /// Grant a feature set to a client in a space.
    ///
    /// Idempotent — re-granting an existing pair is a no-op at the DB layer
    /// (`INSERT OR IGNORE`) but still fires the event so any peer that
    /// missed an earlier notification gets a fresh `list_changed`.
    pub async fn grant_feature_set(
        &self,
        client_id: &str,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let space_uuid = Uuid::parse_str(space_id)?;

        info!(
            %client_id,
            %space_id,
            %feature_set_id,
            "[GrantService] granting feature set"
        );

        self.client_repo
            .grant_feature_set(client_id, space_id, feature_set_id)
            .await?;

        let _ = self.event_tx.send(DomainEvent::ClientGrantChanged {
            client_id: client_id.to_string(),
            space_id: space_uuid,
        });

        Ok(())
    }

    /// Revoke a feature set from a client in a space.
    pub async fn revoke_feature_set(
        &self,
        client_id: &str,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let space_uuid = Uuid::parse_str(space_id)?;

        info!(
            %client_id,
            %space_id,
            %feature_set_id,
            "[GrantService] revoking feature set"
        );

        self.client_repo
            .revoke_feature_set(client_id, space_id, feature_set_id)
            .await?;

        let _ = self.event_tx.send(DomainEvent::ClientGrantChanged {
            client_id: client_id.to_string(),
            space_id: space_uuid,
        });

        Ok(())
    }

    /// Read the granted feature_set_ids for a (client, space) pair.
    pub async fn get_grants_for_space(
        &self,
        client_id: &str,
        space_id: &str,
    ) -> Result<Vec<String>> {
        self.client_repo
            .get_grants_for_space(client_id, space_id)
            .await
    }

    /// Emit a `FeatureSetMembersChanged` event for the given feature set.
    ///
    /// Call this after adding or removing members so every peer subscribed
    /// to the resulting FS re-fetches its tool/prompt/resource list.
    pub async fn notify_feature_set_modified(
        &self,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let space_uuid = Uuid::parse_str(space_id)?;

        info!(
            %space_id,
            %feature_set_id,
            "[GrantService] feature set modified — emitting domain event"
        );

        match self.feature_set_repo.get(feature_set_id).await? {
            Some(feature_set) => {
                if feature_set.space_id.as_deref() != Some(space_id) {
                    warn!(
                        "[GrantService] FS {} belongs to space {:?}, not {}",
                        feature_set_id, feature_set.space_id, space_id
                    );
                    return Ok(());
                }

                let _ = self.event_tx.send(DomainEvent::FeatureSetMembersChanged {
                    space_id: space_uuid,
                    feature_set_id: feature_set_id.to_string(),
                    added_count: 0,
                    removed_count: 0,
                });

                Ok(())
            }
            None => {
                warn!("[GrantService] FS {} not found", feature_set_id);
                Ok(())
            }
        }
    }
}
