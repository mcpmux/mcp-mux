//! Native-dialog approval broker for meta-tool writes.
//!
//! When an LLM calls a write meta tool (e.g. `mcpmux_pin_this_session`),
//! the gateway needs human sign-off before mutating state. The broker
//! bridges that: the tool calls [`ApprovalBroker::request_approval`], which
//! emits a Tauri event the desktop app listens for, awaits a response on a
//! oneshot channel, and returns [`ApprovalDecision`] — Allow (once/always)
//! or Deny (user-denied / timeout / rate-limited / no-desktop).
//!
//! Two non-obvious bits:
//!
//!   * If no desktop is attached (headless CLI, tests without the subscriber
//!     wired), [`ApprovalBroker::request_approval`] returns
//!     [`MetaToolError::ApprovalRequiredNoDesktop`] immediately — a write
//!     without an approver is a silent deny, which is the safe failure mode.
//!
//!   * "Always allow" entries are **session-only** (in-memory `DashMap`,
//!     not persisted). A gateway restart re-prompts. This is a deliberate
//!     security default — auto-approved writes deserve a fresh nod on every
//!     launch. Users can still tick the checkbox once per session.
//!
//! Client identity is treated as an opaque `String` (the OAuth client_id
//! from the JWT — a UUID for the legacy preset-clients path, a
//! client_metadata URL for DCR-registered clients like Claude Code). The
//! broker doesn't parse it; equality + hashing is enough.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, warn};
use uuid::Uuid;

use super::super::MetaToolError;
use super::approval_types::{ApprovalPayload, ApprovalRequest};

/// Tauri / admin SSE channel for pending meta-tool approval dialogs.
pub const META_TOOL_APPROVAL_EVENT: &str = "meta-tool-approval-request";

/// Tauri / admin SSE channel emitted when an approval is resolved (approved
/// or denied) from any surface. Both Tauri and browser dialogs listen for
/// this to auto-dismiss when the other surface acts first.
pub const META_TOOL_APPROVAL_RESOLVED_EVENT: &str = "meta-tool-approval-resolved";

/// Callback invoked by the broker whenever an approval is resolved.
///
/// Receives `(request_id, decision)`. Used to broadcast the resolution to
/// all attached surfaces so orphaned dialogs can self-dismiss.
pub type ResolutionNotifier = Arc<
    dyn Fn(String, ApprovalDecision) -> futures::future::BoxFuture<'static, ()>
        + Send
        + Sync
        + 'static,
>;

/// Default timeout for a single approval prompt.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Rate limit: max approval *requests* per `client_id` within
/// `RATE_LIMIT_WINDOW` (a sliding request-rate window, not a count of
/// currently-pending dialogs).
const RATE_LIMIT_MAX_PENDING: usize = 10;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// User's decision on an approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    /// Allow this (client, tool) pair for the rest of the gateway session.
    AlwaysForThisSessionAndClient,
    Deny,
}

/// Scope of an "always allow" grant. Session-only for now; `Persisted` is
/// reserved for a future settings-backed opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    Once,
    SessionClient,
    #[allow(dead_code)]
    Persisted,
}

/// Subscribe-once handler the desktop layer attaches so broker requests
/// reach the Tauri event bus.
///
/// `respond` closure returns `true` when the listener accepted delivery,
/// `false` when no desktop was attached — which the broker treats as
/// "headless gateway, deny".
pub type ApprovalPublisher = Arc<
    dyn Fn(ApprovalRequest) -> futures::future::BoxFuture<'static, bool> + Send + Sync + 'static,
>;

/// The broker itself.
pub struct ApprovalBroker {
    /// Pending oneshot senders keyed by request_id — the Tauri command
    /// `respond_to_meta_tool_approval` resolves these.
    pending: DashMap<String, oneshot::Sender<ApprovalDecision>>,
    /// Session-scoped always-allow grants, keyed by (client_id, tool_name).
    /// `client_id` is opaque (UUID for preset clients, URL for DCR clients);
    /// the broker only does equality lookups.
    always_allow: DashMap<(String, String), ()>,
    /// (client_id) -> Vec<request_timestamp> for rate limiting.
    rate_limit: DashMap<String, Vec<Instant>>,
    /// Published to the desktop layer; `None` means headless.
    publisher: Mutex<Option<ApprovalPublisher>>,
    /// Called after every `respond()` so all surfaces (Tauri + browser) can
    /// dismiss orphaned dialogs for the resolved request_id.
    resolution_notifier: Mutex<Option<ResolutionNotifier>>,
    timeout: Duration,
}

impl Default for ApprovalBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalBroker {
    pub fn new() -> Self {
        Self {
            pending: DashMap::new(),
            always_allow: DashMap::new(),
            rate_limit: DashMap::new(),
            publisher: Mutex::new(None),
            resolution_notifier: Mutex::new(None),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Attach the desktop subscriber. Call once at app startup.
    pub async fn set_publisher(&self, publisher: ApprovalPublisher) {
        *self.publisher.lock().await = Some(publisher);
    }

    /// Attach the resolution notifier. Call once alongside `set_publisher`.
    ///
    /// The notifier is called after every `respond()` so all surfaces (Tauri
    /// and browser SSE) can dismiss orphaned dialogs for the resolved
    /// `request_id`.
    pub async fn set_resolution_notifier(&self, notifier: ResolutionNotifier) {
        *self.resolution_notifier.lock().await = Some(notifier);
    }

    /// For tests / headless scenarios: pre-approve everything from a
    /// specific client.
    #[cfg(test)]
    pub fn insert_always_allow(&self, client_id: &str, tool_name: &str) {
        self.always_allow
            .insert((client_id.to_string(), tool_name.to_string()), ());
    }

    /// Resolve a pending approval. Called from Tauri command or admin HTTP
    /// handler when the user clicks a dialog button. `scope` converts "allow"
    /// into an optional always-allow entry.
    ///
    /// After resolving the oneshot, fires the `resolution_notifier` so every
    /// attached surface (Tauri + browser SSE) can dismiss its dialog for this
    /// `request_id`, preventing orphaned dialogs when both surfaces are open.
    pub fn respond(
        &self,
        request_id: &str,
        client_id: &str,
        tool_name: &str,
        decision: ApprovalDecision,
    ) -> bool {
        // Persist always-allow before firing the waiter so a racing second
        // call from the same client sees it.
        if matches!(decision, ApprovalDecision::AlwaysForThisSessionAndClient) {
            self.always_allow
                .insert((client_id.to_string(), tool_name.to_string()), ());
        }
        let resolved = if let Some((_, tx)) = self.pending.remove(request_id) {
            tx.send(decision).is_ok()
        } else {
            warn!(
                %request_id,
                "[ApprovalBroker] respond() for unknown/expired request",
            );
            false
        };

        // Notify all surfaces so orphaned dialogs can self-dismiss.
        // Fire-and-forget: clone the notifier out of the Mutex synchronously
        // (try_lock) to avoid async in a sync fn. If the lock is contended
        // the notification is skipped — the dialog will close on timeout.
        if resolved {
            if let Ok(guard) = self.resolution_notifier.try_lock() {
                if let Some(ref notifier) = *guard {
                    let fut = notifier(request_id.to_string(), decision);
                    tokio::spawn(fut);
                }
            }
        }

        resolved
    }

    /// List currently pending (unresolved) approvals. Useful for UI recovery
    /// when the dialog is closed mid-request.
    pub fn list_pending_ids(&self) -> Vec<String> {
        self.pending.iter().map(|e| e.key().clone()).collect()
    }

    /// List always-allow grants (for the UI to display + revoke).
    pub fn list_always_allow(&self) -> Vec<(String, String)> {
        self.always_allow.iter().map(|e| e.key().clone()).collect()
    }

    /// Revoke an always-allow entry.
    pub fn revoke_always_allow(&self, client_id: &str, tool_name: &str) -> bool {
        self.always_allow
            .remove(&(client_id.to_string(), tool_name.to_string()))
            .is_some()
    }

    /// Core entry point for write meta tools.
    ///
    /// Order of checks:
    ///   1. Always-allow hit → immediate `AllowOnce` (no dialog).
    ///   2. Rate limit overflow → `RateLimited`.
    ///   3. No publisher attached → `ApprovalRequiredNoDesktop`.
    ///   4. Emit + wait → Allow / Deny / Timeout.
    pub async fn request_approval(
        &self,
        client_id: &str,
        tool_name: &str,
        payload: ApprovalPayload,
    ) -> Result<ApprovalDecision, MetaToolError> {
        // 1. Always-allow short-circuit.
        if self
            .always_allow
            .contains_key(&(client_id.to_string(), tool_name.to_string()))
        {
            debug!(
                %client_id,
                tool = tool_name,
                "[ApprovalBroker] always-allow hit; approving without dialog",
            );
            return Ok(ApprovalDecision::AllowOnce);
        }

        // 2. Rate limit.
        self.prune_rate_limit(client_id);
        let pending_for_client = self
            .rate_limit
            .get(client_id)
            .map(|e| e.value().len())
            .unwrap_or(0);
        if pending_for_client >= RATE_LIMIT_MAX_PENDING {
            warn!(
                %client_id,
                tool = tool_name,
                pending = pending_for_client,
                "[ApprovalBroker] rate-limited",
            );
            return Err(MetaToolError::RateLimited);
        }
        self.rate_limit
            .entry(client_id.to_string())
            .or_default()
            .push(Instant::now());

        // 3. Require an attached publisher.
        let publisher = match self.publisher.lock().await.clone() {
            Some(p) => p,
            None => {
                warn!(
                    %client_id,
                    tool = tool_name,
                    "[ApprovalBroker] no publisher attached; failing approval",
                );
                return Err(MetaToolError::ApprovalRequiredNoDesktop);
            }
        };

        // 4. Emit + wait on oneshot.
        let request_id = Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now() + chrono::Duration::from_std(self.timeout).unwrap();
        let request = ApprovalRequest {
            request_id: request_id.clone(),
            client_id: client_id.to_string(),
            payload,
            expires_at_unix_secs: expires_at.timestamp() as u64,
        };

        let (tx, rx) = oneshot::channel();
        self.pending.insert(request_id.clone(), tx);

        let delivered = publisher(request.clone()).await;
        if !delivered {
            // Publisher disavowed delivery — treat like "no desktop".
            self.pending.remove(&request_id);
            return Err(MetaToolError::ApprovalRequiredNoDesktop);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(decision)) => match decision {
                ApprovalDecision::Deny => Err(MetaToolError::ApprovalDenied),
                other => Ok(other),
            },
            Ok(Err(_)) => {
                // Sender dropped without deciding — treat as deny.
                Err(MetaToolError::ApprovalDenied)
            }
            Err(_) => {
                self.pending.remove(&request_id);
                Err(MetaToolError::ApprovalTimedOut)
            }
        }
    }

    fn prune_rate_limit(&self, client_id: &str) {
        if let Some(mut entry) = self.rate_limit.get_mut(client_id) {
            let cutoff = Instant::now() - RATE_LIMIT_WINDOW;
            entry.retain(|t| *t > cutoff);
        }
    }
}

#[cfg(test)]
#[path = "approval_broker_tests.rs"]
mod tests;
