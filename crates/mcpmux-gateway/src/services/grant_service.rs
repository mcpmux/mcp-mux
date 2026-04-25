//! Feature set change broadcaster.
//!
//! Emits `FeatureSetMembersChanged` domain events so the MCP notifier can
//! broadcast `list_changed` notifications after any member edit. This used
//! to host grant/revoke plumbing too, but per-client grants have been
//! removed — routing now flows purely through WorkspaceBinding + each
//! Space's Default feature set.

use anyhow::Result;
use mcpmux_core::{DomainEvent, FeatureSetRepository};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

/// Emits domain events for FeatureSet membership edits.
///
/// Named `GrantService` for historical reasons (older callers expect the
/// symbol) — functionally it's just a thin notifier around
/// `FeatureSetMembersChanged`.
pub struct GrantService {
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    event_tx: broadcast::Sender<DomainEvent>,
}

impl GrantService {
    pub fn new(
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        event_tx: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            feature_set_repo,
            event_tx,
        }
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
            space_id = %space_id,
            feature_set_id = %feature_set_id,
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
