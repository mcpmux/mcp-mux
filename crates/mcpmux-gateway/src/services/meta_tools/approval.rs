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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, warn};
use uuid::Uuid;

use super::MetaToolError;
use crate::services::ANONYMOUS_CLIENT_ID;

/// Default timeout for a single approval prompt.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Rate limit: max pending approvals per (client_id) within the window.
const RATE_LIMIT_MAX_PENDING: usize = 10;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// User's decision on an approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    /// Allow this (client, tool, action, Space) for the rest of the gateway
    /// session (until McpMux restarts).
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

/// Payload delivered to the desktop UI so it can render a meaningful dialog.
///
/// Keep this narrow and JSON-serializable — it crosses the Tauri boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPayload {
    pub tool_name: String,
    /// Human summary the dialog puts above the diff. e.g.
    /// "Pin this connection to FeatureSet 'android-dev' (12 tools)".
    pub summary: String,
    /// Name of the Space this write targets, surfaced as a labeled chip so the
    /// user can see (and reject) a change aimed at a Space other than the one
    /// they expect — important now that a client may pass any `space_id`.
    /// `None` for writes with no single target Space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_name: Option<String>,
    /// Id of the Space this write targets. "Always allow" grants are scoped
    /// to it, so approving writes to one Space never approves another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Uuid>,
    /// Tool-list diff the dialog shows to make the change concrete.
    /// Optional because some writes (e.g. create_feature_set without
    /// activation) don't shift the caller's resolved toolset.
    pub diff: Option<serde_json::Value>,
    /// The tool's `action` argument (e.g. create / update / delete for
    /// `mcpmux_manage_feature_set`). "Always allow" grants are scoped to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Whether the dialog may offer "Always allow". Set by the broker: never
    /// for workspace binding, deletes, or tokenless connections.
    #[serde(default)]
    pub allow_always: bool,
    /// Raw arguments the LLM supplied; shown verbatim for auditability.
    pub raw_args: serde_json::Value,
    /// Does this change affect clients other than the caller? Dictates
    /// whether the dialog shows the "also affects other connections" warning.
    pub affects_other_clients: bool,
}

/// Data the broker hands to whoever listens for approval requests.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub client_id: String,
    pub payload: ApprovalPayload,
    /// UNIX seconds at which this request will time out if no response.
    pub expires_at_unix_secs: u64,
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

/// What an "always allow" grant covers: one client, one tool, one action of
/// that tool, one target Space.
pub type GrantKey = (String, String, Option<String>, Option<Uuid>);

/// A prompt waiting for the user. The grant scope is kept here, server-side,
/// so a response can't widen it.
struct PendingApproval {
    tx: oneshot::Sender<ApprovalDecision>,
    grant: GrantKey,
    /// Whether "always" may be stored for this request.
    allow_always: bool,
}

/// Whether a write may get a standing "always allow" grant. Moving a
/// workspace between Spaces and deleting are approved one at a time, and
/// tokenless connections all share one identity, so never get one.
fn standing_grant_allowed(client_id: &str, tool_name: &str, action: Option<&str>) -> bool {
    client_id != ANONYMOUS_CLIENT_ID
        && tool_name != "mcpmux_bind_current_workspace"
        && action != Some("delete")
}

/// The broker itself.
pub struct ApprovalBroker {
    /// Pending prompts keyed by request_id — the Tauri command
    /// `respond_to_meta_tool_approval` resolves these.
    pending: DashMap<String, PendingApproval>,
    /// Session-scoped always-allow grants, keyed by (client_id, tool_name,
    /// target Space). `client_id` is opaque (UUID for preset clients, URL for
    /// DCR clients); the broker only does equality lookups.
    always_allow: DashMap<GrantKey, ()>,
    /// (client_id) -> Vec<request_timestamp> for rate limiting.
    rate_limit: DashMap<String, Vec<Instant>>,
    /// Published to the desktop layer; `None` means headless.
    publisher: Mutex<Option<ApprovalPublisher>>,
    timeout: Duration,
    /// Whether write meta-tools require human approval at all. Default `true`
    /// (every write prompts). A user can turn this OFF in Settings to trust a
    /// local machine — then writes are auto-approved without a dialog. The
    /// authoritative value is **persisted** in app settings
    /// (`meta_tools.require_approval`); this in-memory flag is restored from
    /// there on every gateway start (the broker is recreated per start).
    require_approval: AtomicBool,
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
            timeout: DEFAULT_TIMEOUT,
            require_approval: AtomicBool::new(true),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set whether write meta-tools require approval. `false` = auto-approve
    /// every write (no dialog) — the user's explicit "trust this machine"
    /// choice. Persisted by the caller; applied to the broker here.
    pub fn set_require_approval(&self, required: bool) {
        self.require_approval.store(required, Ordering::Relaxed);
        if !required {
            warn!(
                "[ApprovalBroker] approval requirement DISABLED — meta-tool writes auto-approved"
            );
        }
    }

    /// Whether write meta-tools currently require approval (default `true`).
    pub fn require_approval_enabled(&self) -> bool {
        self.require_approval.load(Ordering::Relaxed)
    }

    /// Attach the desktop subscriber. Call once at app startup.
    pub async fn set_publisher(&self, publisher: ApprovalPublisher) {
        *self.publisher.lock().await = Some(publisher);
    }

    /// For tests / headless scenarios: pre-approve everything from a
    /// specific client.
    #[cfg(test)]
    pub fn insert_always_allow(
        &self,
        client_id: &str,
        tool_name: &str,
        action: Option<&str>,
        space_id: Option<Uuid>,
    ) {
        self.always_allow.insert(
            (
                client_id.to_string(),
                tool_name.to_string(),
                action.map(str::to_string),
                space_id,
            ),
            (),
        );
    }

    /// Resolve a pending approval. Called from Tauri command when the user
    /// clicks a dialog button. `scope` converts "allow" into an optional
    /// always-allow entry.
    pub fn respond(
        &self,
        request_id: &str,
        client_id: &str,
        tool_name: &str,
        decision: ApprovalDecision,
    ) -> bool {
        let Some((_, pending)) = self.pending.remove(request_id) else {
            warn!(
                %request_id,
                "[ApprovalBroker] respond() for unknown/expired request",
            );
            return false;
        };
        let (grant_client, grant_tool, _, _) = &pending.grant;
        if grant_client != client_id || grant_tool != tool_name {
            warn!(
                %request_id,
                "[ApprovalBroker] response names a different client/tool than the request; using the request's",
            );
        }
        // Persist always-allow before firing the waiter so a racing second
        // call from the same client sees it — only where a standing grant is
        // allowed (see `standing_grant_allowed`); otherwise it counts as once.
        if matches!(decision, ApprovalDecision::AlwaysForThisSessionAndClient)
            && pending.allow_always
        {
            self.always_allow.insert(pending.grant.clone(), ());
        }
        pending.tx.send(decision).is_ok()
    }

    /// List currently pending (unresolved) approvals. Useful for UI recovery
    /// when the dialog is closed mid-request.
    pub fn list_pending_ids(&self) -> Vec<String> {
        self.pending.iter().map(|e| e.key().clone()).collect()
    }

    /// List always-allow grants (for the UI to display + revoke).
    pub fn list_always_allow(&self) -> Vec<GrantKey> {
        self.always_allow.iter().map(|e| e.key().clone()).collect()
    }

    /// Revoke one always-allow grant, exactly as listed.
    pub fn revoke_always_allow(&self, grant: &GrantKey) -> bool {
        self.always_allow.remove(grant).is_some()
    }

    /// Core entry point for write meta tools.
    ///
    /// Order of checks:
    ///   0. Approval requirement disabled (user opt-out) → `AllowOnce`.
    ///   1. Always-allow hit → immediate `AllowOnce` (no dialog).
    ///   2. Rate limit overflow → `RateLimited`.
    ///   3. No publisher attached → `ApprovalRequiredNoDesktop`.
    ///   4. Emit + wait → Allow / Deny / Timeout.
    pub async fn request_approval(
        &self,
        client_id: &str,
        tool_name: &str,
        mut payload: ApprovalPayload,
    ) -> Result<ApprovalDecision, MetaToolError> {
        // 0. Global "require approval" switch OFF — the user has opted to
        //    auto-approve every write on this (trusted, local) machine.
        if !self.require_approval.load(Ordering::Relaxed) {
            debug!(
                %client_id,
                tool = tool_name,
                "[ApprovalBroker] approval requirement disabled; approving without dialog",
            );
            return Ok(ApprovalDecision::AllowOnce);
        }

        // 1. Always-allow short-circuit (same client, tool, action and
        //    target Space).
        let grant: GrantKey = (
            client_id.to_string(),
            tool_name.to_string(),
            payload.action.clone(),
            payload.space_id,
        );
        let allow_always = standing_grant_allowed(client_id, tool_name, payload.action.as_deref());
        payload.allow_always = allow_always;
        if self.always_allow.contains_key(&grant) {
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
        self.pending.insert(
            request_id.clone(),
            PendingApproval {
                tx,
                grant,
                allow_always,
            },
        );

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
mod tests {
    use super::*;
    use futures::FutureExt;

    fn make_payload() -> ApprovalPayload {
        payload_for(None)
    }

    fn payload_for(space_id: Option<Uuid>) -> ApprovalPayload {
        payload_with(None, space_id)
    }

    fn payload_with(action: Option<&str>, space_id: Option<Uuid>) -> ApprovalPayload {
        ApprovalPayload {
            tool_name: "mcpmux_pin_this_session".into(),
            summary: "test".into(),
            space_name: None,
            space_id,
            diff: None,
            action: action.map(str::to_string),
            allow_always: false,
            raw_args: serde_json::json!({}),
            affects_other_clients: false,
        }
    }

    #[tokio::test]
    async fn no_publisher_returns_no_desktop_error() {
        let broker = ApprovalBroker::new();
        let err = broker
            .request_approval(
                &Uuid::new_v4().to_string(),
                "mcpmux_pin_this_session",
                make_payload(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, MetaToolError::ApprovalRequiredNoDesktop));
    }

    #[tokio::test]
    async fn always_allow_short_circuits() {
        let broker = ApprovalBroker::new();
        let client_id = Uuid::new_v4().to_string();
        broker.insert_always_allow(&client_id, "mcpmux_pin_this_session", None, None);
        let d = broker
            .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
            .await
            .unwrap();
        assert_eq!(d, ApprovalDecision::AllowOnce);
    }

    #[tokio::test]
    async fn require_approval_off_auto_approves_without_publisher() {
        // Default is ON (require approval).
        let broker = ApprovalBroker::new();
        assert!(broker.require_approval_enabled());

        // OFF → writes auto-approve even with no desktop attached (which would
        // otherwise be ApprovalRequiredNoDesktop).
        broker.set_require_approval(false);
        assert!(!broker.require_approval_enabled());
        let d = broker
            .request_approval(
                &Uuid::new_v4().to_string(),
                "mcpmux_manage_feature_set",
                make_payload(),
            )
            .await
            .unwrap();
        assert_eq!(d, ApprovalDecision::AllowOnce);

        // Back ON → no publisher → safe headless deny again.
        broker.set_require_approval(true);
        let err = broker
            .request_approval(
                &Uuid::new_v4().to_string(),
                "mcpmux_manage_feature_set",
                make_payload(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, MetaToolError::ApprovalRequiredNoDesktop));
    }

    #[tokio::test]
    async fn url_client_id_works() {
        // Regression for the bug where DCR-registered clients (which use
        // a client_metadata URL as their client_id) couldn't get past the
        // approval flow because we tried to parse the URL as a UUID.
        let broker = ApprovalBroker::new();
        let url_client_id = "https://claude.ai/oauth/claude-code-client-metadata";
        broker.insert_always_allow(url_client_id, "mcpmux_pin_this_session", None, None);
        let d = broker
            .request_approval(url_client_id, "mcpmux_pin_this_session", make_payload())
            .await
            .unwrap();
        assert_eq!(d, ApprovalDecision::AllowOnce);
    }

    #[tokio::test]
    async fn publisher_allow_resolves() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let broker_clone = broker.clone();
        let client_id = Uuid::new_v4().to_string();

        // Publisher responds asynchronously with Allow.
        let publisher: ApprovalPublisher = Arc::new(move |req| {
            let b = broker_clone.clone();
            async move {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    b.respond(
                        &req.request_id,
                        &req.client_id,
                        &req.payload.tool_name,
                        ApprovalDecision::AllowOnce,
                    );
                });
                true
            }
            .boxed()
        });
        broker.set_publisher(publisher).await;

        let decision = broker
            .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
            .await
            .unwrap();
        assert_eq!(decision, ApprovalDecision::AllowOnce);
    }

    #[tokio::test]
    async fn publisher_deny_returns_denied_error() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let broker_clone = broker.clone();
        let client_id = Uuid::new_v4().to_string();

        let publisher: ApprovalPublisher = Arc::new(move |req| {
            let b = broker_clone.clone();
            async move {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    b.respond(
                        &req.request_id,
                        &req.client_id,
                        &req.payload.tool_name,
                        ApprovalDecision::Deny,
                    );
                });
                true
            }
            .boxed()
        });
        broker.set_publisher(publisher).await;

        let err = broker
            .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
            .await
            .unwrap_err();
        assert!(matches!(err, MetaToolError::ApprovalDenied));
    }

    #[tokio::test]
    async fn publisher_timeout() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(50)));
        // Publisher accepts delivery but never responds.
        let publisher: ApprovalPublisher = Arc::new(move |_req| async move { true }.boxed());
        broker.set_publisher(publisher).await;

        let err = broker
            .request_approval(
                &Uuid::new_v4().to_string(),
                "mcpmux_pin_this_session",
                make_payload(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, MetaToolError::ApprovalTimedOut));
    }

    #[tokio::test]
    async fn always_scope_persists_across_calls() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let broker_clone = broker.clone();
        let client_id = Uuid::new_v4().to_string();

        let publisher: ApprovalPublisher = Arc::new(move |req| {
            let b = broker_clone.clone();
            async move {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    b.respond(
                        &req.request_id,
                        &req.client_id,
                        &req.payload.tool_name,
                        ApprovalDecision::AlwaysForThisSessionAndClient,
                    );
                });
                true
            }
            .boxed()
        });
        broker.set_publisher(publisher).await;

        // First call → dialog, returns AlwaysForThisSessionAndClient.
        let d1 = broker
            .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
            .await
            .unwrap();
        assert_eq!(d1, ApprovalDecision::AlwaysForThisSessionAndClient);

        // Second call → short-circuits via always-allow entry.
        let d2 = broker
            .request_approval(&client_id, "mcpmux_pin_this_session", make_payload())
            .await
            .unwrap();
        assert_eq!(d2, ApprovalDecision::AllowOnce);
    }

    /// A publisher that answers every prompt with `decision`, counting prompts.
    fn answering(
        broker: &Arc<ApprovalBroker>,
        decision: ApprovalDecision,
        prompts: Arc<std::sync::atomic::AtomicUsize>,
    ) -> ApprovalPublisher {
        let broker = broker.clone();
        Arc::new(move |req| {
            let b = broker.clone();
            let prompts = prompts.clone();
            async move {
                prompts.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    b.respond(
                        &req.request_id,
                        &req.client_id,
                        &req.payload.tool_name,
                        decision,
                    );
                });
                true
            }
            .boxed()
        })
    }

    #[tokio::test]
    async fn always_allow_is_scoped_to_the_target_space() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let prompts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        broker
            .set_publisher(answering(
                &broker,
                ApprovalDecision::AlwaysForThisSessionAndClient,
                prompts.clone(),
            ))
            .await;
        let client_id = Uuid::new_v4().to_string();
        let (space_a, space_b) = (Uuid::new_v4(), Uuid::new_v4());

        broker
            .request_approval(
                &client_id,
                "mcpmux_manage_feature_set",
                payload_for(Some(space_a)),
            )
            .await
            .unwrap();
        broker
            .request_approval(
                &client_id,
                "mcpmux_manage_feature_set",
                payload_for(Some(space_a)),
            )
            .await
            .unwrap();
        assert_eq!(prompts.load(Ordering::SeqCst), 1, "same Space: granted");

        broker
            .request_approval(
                &client_id,
                "mcpmux_manage_feature_set",
                payload_for(Some(space_b)),
            )
            .await
            .unwrap();
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            2,
            "another Space prompts again"
        );
    }

    #[tokio::test]
    async fn tokenless_connections_never_get_a_standing_grant() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let prompts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        broker
            .set_publisher(answering(
                &broker,
                ApprovalDecision::AlwaysForThisSessionAndClient,
                prompts.clone(),
            ))
            .await;

        for _ in 0..2 {
            broker
                .request_approval(
                    ANONYMOUS_CLIENT_ID,
                    "mcpmux_manage_feature_set",
                    make_payload(),
                )
                .await
                .unwrap();
        }
        assert_eq!(prompts.load(Ordering::SeqCst), 2);
        assert!(broker.list_always_allow().is_empty());
    }

    #[tokio::test]
    async fn always_allow_is_scoped_to_the_action() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let prompts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        broker
            .set_publisher(answering(
                &broker,
                ApprovalDecision::AlwaysForThisSessionAndClient,
                prompts.clone(),
            ))
            .await;
        let client_id = Uuid::new_v4().to_string();
        let space = Some(Uuid::new_v4());
        let request = |action: &'static str| {
            let broker = broker.clone();
            let client_id = client_id.clone();
            async move {
                broker
                    .request_approval(
                        &client_id,
                        "mcpmux_manage_feature_set",
                        payload_with(Some(action), space),
                    )
                    .await
                    .unwrap();
            }
        };

        request("create").await;
        request("create").await;
        assert_eq!(prompts.load(Ordering::SeqCst), 1, "create: granted");
        request("update").await;
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            2,
            "update isn't covered by create"
        );
        request("delete").await;
        request("delete").await;
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            4,
            "delete prompts every time"
        );
        let actions: Vec<_> = broker
            .list_always_allow()
            .into_iter()
            .map(|(_, _, action, _)| action)
            .collect();
        assert!(
            !actions.contains(&Some("delete".to_string())),
            "{actions:?}"
        );
    }

    #[tokio::test]
    async fn workspace_binding_never_gets_a_standing_grant() {
        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let offered = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = offered.clone();
        let responder = broker.clone();
        broker
            .set_publisher(Arc::new(move |req: ApprovalRequest| {
                seen.lock().unwrap().push(req.payload.allow_always);
                let broker = responder.clone();
                async move {
                    tokio::spawn(async move {
                        broker.respond(
                            &req.request_id,
                            &req.client_id,
                            &req.payload.tool_name,
                            ApprovalDecision::AlwaysForThisSessionAndClient,
                        );
                    });
                    true
                }
                .boxed()
            }))
            .await;
        let client_id = Uuid::new_v4().to_string();

        for tool in [
            "mcpmux_bind_current_workspace",
            "mcpmux_bind_current_workspace",
        ] {
            broker
                .request_approval(&client_id, tool, payload_for(Some(Uuid::nil())))
                .await
                .unwrap();
        }
        broker
            .request_approval(
                &client_id,
                "mcpmux_pin_this_session",
                payload_for(Some(Uuid::nil())),
            )
            .await
            .unwrap();

        // The dialog is told not to offer "always" for binding, and nothing
        // is stored even if a response claims it.
        assert_eq!(*offered.lock().unwrap(), [false, false, true]);
        let tools: Vec<_> = broker
            .list_always_allow()
            .into_iter()
            .map(|(_, tool, _, _)| tool)
            .collect();
        assert_eq!(tools, ["mcpmux_pin_this_session"]);
    }

    #[tokio::test]
    async fn revoking_removes_only_that_grant() {
        let broker = ApprovalBroker::new();
        let (a, b) = (Some(Uuid::new_v4()), Some(Uuid::new_v4()));
        broker.insert_always_allow("c", "mcpmux_manage_feature_set", Some("create"), a);
        broker.insert_always_allow("c", "mcpmux_manage_feature_set", Some("create"), b);
        assert!(broker.revoke_always_allow(&(
            "c".into(),
            "mcpmux_manage_feature_set".into(),
            Some("create".into()),
            a
        )));
        assert_eq!(broker.list_always_allow().len(), 1);
        assert_eq!(broker.list_always_allow()[0].3, b);
    }

    #[tokio::test]
    async fn responses_to_unknown_requests_grant_nothing() {
        let broker = ApprovalBroker::new();
        assert!(!broker.respond(
            "no-such-request",
            "some-client",
            "mcpmux_manage_feature_set",
            ApprovalDecision::AlwaysForThisSessionAndClient,
        ));
        assert!(broker.list_always_allow().is_empty());
    }
}
