//! Grant Service
//!
//! Centralized service for managing client feature set grants.
//!
//! **Responsibility (SRP):**
//! - Grant/revoke feature sets to clients
//! - Emit list_changed notifications automatically for ALL grant changes
//! - Ensure DRY - single place for grant logic + notifications
//!
//! **Design:**
//! - UI/Tauri commands call this service for ALL grant operations
//! - Service updates DB + emits events (no manual notification calls needed)
//! - Notifications work for: default grants, custom grants, individual features, batch updates

use anyhow::Result;
use mcpmux_core::{DomainEvent, FeatureSetRepository};
use mcpmux_storage::InboundClientRepository;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

/// Centralized service for grant management with automatic event emission
///
/// **SOLID & Domain-Driven Design:**
/// - **SRP**: Single responsibility - manage grants + emit domain events
/// - **DIP**: Depends on abstractions (FeatureSetRepository trait)
/// - **Domain Events**: Emits what happened, not what to do (consumers decide)
///
/// **Enterprise Pattern:**
/// - Uses domain events (GrantIssued, etc.) instead of implementation-specific events
/// - Consumers (MCPNotifier, UI) interpret events based on their context
/// - Testable, extensible, and follows event-driven architecture principles
pub struct GrantService {
    /// OAuth client grant repository (concrete for simplicity)
    client_repo: Arc<InboundClientRepository>,
    /// Feature set validation (trait for flexibility)
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    /// Domain event broadcaster (decoupled from consumers)
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

    /// Grant a feature set to a client in a space
    ///
    /// Emits FeatureSetGranted domain event for consumers to handle.
    pub async fn grant_feature_set(
        &self,
        client_id: &str,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let space_uuid = Uuid::parse_str(space_id)?;

        info!(
            client_id = %client_id,
            space_id = %space_id,
            feature_set_id = %feature_set_id,
            "[GrantService] Granting feature set"
        );

        // Update database
        self.client_repo
            .grant_feature_set(client_id, space_id, feature_set_id)
            .await?;

        info!("[GrantService] Feature set granted successfully");

        // Emit domain event (what happened, not what to do)
        let _ = self.event_tx.send(DomainEvent::GrantIssued {
            client_id: client_id.to_string(),
            space_id: space_uuid,
            feature_set_id: feature_set_id.to_string(),
        });

        Ok(())
    }

    /// Revoke a feature set from a client in a space
    ///
    /// Emits FeatureSetRevoked domain event for consumers to handle.
    pub async fn revoke_feature_set(
        &self,
        client_id: &str,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let space_uuid = Uuid::parse_str(space_id)?;

        info!(
            client_id = %client_id,
            space_id = %space_id,
            feature_set_id = %feature_set_id,
            "[GrantService] Revoking feature set"
        );

        // Update database
        self.client_repo
            .revoke_feature_set(client_id, space_id, feature_set_id)
            .await?;

        info!("[GrantService] Feature set revoked successfully");

        // Emit domain event (what happened, not what to do)
        let _ = self.event_tx.send(DomainEvent::GrantRevoked {
            client_id: client_id.to_string(),
            space_id: space_uuid,
            feature_set_id: feature_set_id.to_string(),
        });

        Ok(())
    }

    /// Notify when a feature set's contents are modified
    ///
    /// Call this after adding/removing features to/from a feature set.
    /// Emits FeatureSetModified domain event for consumers to handle.
    pub async fn notify_feature_set_modified(
        &self,
        space_id: &str,
        feature_set_id: &str,
    ) -> Result<()> {
        let space_uuid = Uuid::parse_str(space_id)?;

        info!(
            space_id = %space_id,
            feature_set_id = %feature_set_id,
            "[GrantService] Feature set modified - emitting domain event"
        );

        // Verify feature set exists
        match self.feature_set_repo.get(feature_set_id).await? {
            Some(feature_set) => {
                // Ensure the feature set belongs to the specified space
                if feature_set.space_id.as_deref() != Some(space_id) {
                    warn!(
                        "[GrantService] Feature set {} belongs to space {:?}, not {}",
                        feature_set_id, feature_set.space_id, space_id
                    );
                    return Ok(()); // Silently skip
                }

                // Emit domain event (what happened, not what to do)
                // Note: We don't track exact counts here since this is a generic modified signal
                let _ = self.event_tx.send(DomainEvent::FeatureSetMembersChanged {
                    space_id: space_uuid,
                    feature_set_id: feature_set_id.to_string(),
                    added_count: 0, // Generic modification signal
                    removed_count: 0,
                });

                Ok(())
            }
            None => {
                warn!("[GrantService] Feature set {} not found", feature_set_id);
                Ok(()) // Silently skip
            }
        }
    }
}
