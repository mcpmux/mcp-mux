//! MCP Notifier - Sends list_changed notifications to MCP clients
//!
//! Smart consumer that listens to DomainEvents and dispatches MCP notifications
//! to connected inbound clients (Cursor, VS Code, Claude Desktop).
//!
//! **Dual Responsibility:**
//! - **Peer Registry**: Manages peer lifecycle (register/unregister) for session management
//! - **Smart Consumer**: Subscribes to DomainEvents and sends notifications to registered peers

/// **DEBUG KILL SWITCH**: Set to `true` to disable ALL list_changed notifications
///
/// Use this to diagnose if notifications are causing client reconnection loops.
/// When enabled, events are still received but no notifications are sent to clients.
const DISABLE_ALL_NOTIFICATIONS: bool = false;

use mcpmux_core::{DomainEvent, FeatureType};
use parking_lot::{Mutex, RwLock};
use rmcp::{service::Peer, RoleServer};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::pool::FeatureService;
use crate::services::{EmbeddingWarmer, FeatureSetResolverService};

/// MCP Notifier — sends `list_changed` notifications to connected sessions.
///
/// **Session-keyed registry.** A single OAuth client (Cursor, Claude
/// Desktop) can hold multiple concurrent MCP sessions, and each session
/// can resolve to a *different* (Space, FeatureSet) via WorkspaceBinding
/// — two VS Code windows on different folders are the canonical case.
/// Indexing by `mcp-session-id` lets us notify the right session(s)
/// without over-notifying the others, and matches the request-side
/// routing model (resolver consults session_id, not client_id).
///
/// **Fanout uses the same resolver as the request handlers.** When an
/// event implies "FS X may have changed for any session resolving to it",
/// we re-run the resolver per session and notify the ones whose resolved
/// FS list contains X (or whose resolved space matches, depending on the
/// trigger). This is what closes the "FS edit doesn't reflect until
/// reconnect" loophole.
///
/// **Other duties (unchanged):**
/// - Listens to DomainEvents from the EventBus.
/// - Throttles per (space_id, notification_type) to prevent flapping.
/// - Hashes feature lists to dedupe spurious notifications.
#[derive(Clone)]
pub struct MCPNotifier {
    /// Map: `mcp-session-id` → session handle.
    sessions: Arc<RwLock<HashMap<String, SessionEntry>>>,
    /// FeatureSet resolver — same one the request handlers use. Consulted
    /// per session to decide whether a notification applies.
    feature_set_resolver: Arc<FeatureSetResolverService>,
    /// Feature service for calculating content hashes
    feature_service: Arc<FeatureService>,
    /// Throttle tracker: (space_id, notification_type) -> last_sent_timestamp
    /// Prevents sending duplicate notifications within THROTTLE_WINDOW
    throttle_tracker: Arc<RwLock<HashMap<(Uuid, NotificationType), Instant>>>,
    /// State hash tracker: (space_id, notification_type) -> content_hash
    /// Prevents sending notifications when content hasn't actually changed
    state_hashes: Arc<RwLock<HashMap<(Uuid, NotificationType), u64>>>,
    /// Keys that currently have a trailing-edge retry task scheduled.
    ///
    /// The throttle must *defer*, never *drop*: two real mutations inside
    /// one throttle window used to lose the second notification, leaving
    /// clients on the intermediate state until an unrelated event fired.
    /// When a send is throttled we schedule exactly one retry for the end
    /// of the window; this set dedupes concurrent schedulers.
    pending_retries: Arc<Mutex<HashSet<(Uuid, NotificationType)>>>,
    /// Optional background embedding warmer, triggered on server connect /
    /// feature-refresh events so `mcpmux_search_tools` has warm vectors.
    embedding_warmer: Arc<RwLock<Option<Arc<EmbeddingWarmer>>>>,
}

/// Type of list_changed notification for throttling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NotificationType {
    Tools,
    Prompts,
    Resources,
    All, // For notify_all_list_changed
}

/// Minimum time between notifications of the same type for the same space
/// Prevents infinite loops when backend servers emit rapid list_changed notifications
///
/// **Note**: With content-based deduping (hashing), this window can be short (1s).
/// Hashing prevents redundant notifications (startup loops), while this throttle
/// prevents rapid state oscillation (flapping).
const THROTTLE_WINDOW: Duration = Duration::from_secs(1);

/// One registered MCP session — the gateway's view of a single live
/// `mcp-session-id`. The peer is what we push notifications to; the
/// `client_id` is kept for per-client fanout (e.g. on grant change).
#[derive(Clone)]
struct SessionEntry {
    peer: Arc<Peer<RoleServer>>,
    client_id: String,
    /// True once the SSE stream for this session is open and notifications
    /// will actually deliver. Sessions register on `initialize`; the
    /// stream-active flag flips when the gateway opens the SSE side.
    has_active_stream: bool,
}

impl SessionEntry {
    fn new(client_id: String, peer: Arc<Peer<RoleServer>>) -> Self {
        Self {
            peer,
            client_id,
            has_active_stream: false,
        }
    }
}

impl MCPNotifier {
    pub fn new(
        feature_set_resolver: Arc<FeatureSetResolverService>,
        feature_service: Arc<FeatureService>,
    ) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            feature_set_resolver,
            feature_service,
            throttle_tracker: Arc::new(RwLock::new(HashMap::new())),
            state_hashes: Arc::new(RwLock::new(HashMap::new())),
            pending_retries: Arc::new(Mutex::new(HashSet::new())),
            embedding_warmer: Arc::new(RwLock::new(None)),
        }
    }

    /// Attach an embedding warmer that runs on server connect / feature-refresh
    /// events so the hybrid `mcpmux_search_tools` ranking has warm vectors.
    pub fn set_embedding_warmer(&self, warmer: Arc<EmbeddingWarmer>) {
        *self.embedding_warmer.write() = Some(warmer);
    }

    /// Calculate hash of all available features of a given type in a space
    /// Used for content-based deduping
    async fn calculate_feature_hash(&self, space_id: Uuid, feature_type: FeatureType) -> u64 {
        let features = self
            .feature_service
            .get_all_features_for_space(&space_id.to_string(), Some(feature_type))
            .await
            .unwrap_or_default();

        let mut hasher = DefaultHasher::new();
        // Sort IDs to ensure stable hash regardless of DB order
        let mut sorted_ids: Vec<_> = features.iter().map(|f| &f.id).collect();
        sorted_ids.sort();

        for id in sorted_ids {
            id.hash(&mut hasher);
        }

        // Also hash the server aliases to capture renames/topology changes
        let mut sorted_aliases: Vec<_> = features
            .iter()
            .filter_map(|f| f.server_alias.as_ref())
            .collect();
        sorted_aliases.sort();

        for alias in sorted_aliases {
            alias.hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Register a session for notification delivery.
    ///
    /// Called from `on_initialized` once per `mcp-session-id`. The same
    /// client may register multiple sessions concurrently (two VS Code
    /// windows on different folders share one OAuth `client_id`); the
    /// session-keyed map keeps them independent.
    ///
    /// **Note**: starts with `has_active_stream = false`. Call
    /// [`mark_session_stream_active`](Self::mark_session_stream_active)
    /// after the SSE stream opens.
    pub fn register_session(
        &self,
        session_id: String,
        client_id: String,
        peer: Arc<Peer<RoleServer>>,
    ) {
        let entry = SessionEntry::new(client_id.clone(), peer);
        let mut sessions = self.sessions.write();
        let is_reconnect = sessions.contains_key(&session_id);
        sessions.insert(session_id.clone(), entry);
        info!(
            %session_id,
            %client_id,
            is_reconnect,
            total_sessions = sessions.len(),
            "[MCPNotifier] 📡 Registered session (stream not yet active)"
        );
    }

    /// Mark that a client has an active SSE stream and can receive notifications
    ///
    /// This should be called when a client successfully creates an SSE stream.
    /// Notifications will only be sent to clients with active streams.
    ///
    /// Also pre-populates the feature hash for the client's space to prevent
    /// spurious "first notification" issues. Without this, the first `list_changed`
    /// event would always be forwarded (no hash to compare against), potentially
    /// causing client reconnection loops.
    pub fn mark_session_stream_active(&self, session_id: &str) {
        let mut sessions = self.sessions.write();
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.has_active_stream = true;
            info!(
                %session_id,
                client_id = %entry.client_id,
                "[MCPNotifier] ✅ Session stream is now active (notifications enabled)"
            );
        } else {
            warn!(
                %session_id,
                "[MCPNotifier] ⚠️ Attempted to mark stream active for unknown session"
            );
        }
    }

    /// Pre-populate feature hashes for a space
    ///
    /// This should be called when a client connects to ensure the first
    /// `list_changed` notification is properly deduplicated. Without this,
    /// the first notification would always pass through (no previous hash).
    pub async fn prime_hashes_for_space(&self, space_id: Uuid) {
        let tools_hash = self
            .calculate_feature_hash(space_id, FeatureType::Tool)
            .await;
        let prompts_hash = self
            .calculate_feature_hash(space_id, FeatureType::Prompt)
            .await;
        let resources_hash = self
            .calculate_feature_hash(space_id, FeatureType::Resource)
            .await;

        let mut hashes = self.state_hashes.write();

        // Only insert if not already present (don't overwrite existing hashes)
        hashes
            .entry((space_id, NotificationType::Tools))
            .or_insert(tools_hash);
        hashes
            .entry((space_id, NotificationType::Prompts))
            .or_insert(prompts_hash);
        hashes
            .entry((space_id, NotificationType::Resources))
            .or_insert(resources_hash);

        debug!(
            space_id = %space_id,
            tools = tools_hash,
            prompts = prompts_hash,
            resources = resources_hash,
            "[MCPNotifier] 🔐 Primed feature hashes for space"
        );
    }

    /// Unregister a session.
    ///
    /// Called when a client disconnects or the session closes.
    pub fn unregister_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write();
        if let Some(removed) = sessions.remove(session_id) {
            info!(
                %session_id,
                client_id = %removed.client_id,
                remaining_sessions = sessions.len(),
                "[MCPNotifier] 📴 Unregistered session"
            );
        } else {
            warn!(
                %session_id,
                "[MCPNotifier] ⚠️ Attempted to unregister unknown session"
            );
        }
    }

    /// Time left in the throttle window for this key, or `None` when a send
    /// is allowed right now. Callers that get `Some(remaining)` must defer
    /// via [`Self::schedule_trailing_retry`] rather than dropping the
    /// notification.
    fn throttle_remaining(
        &self,
        space_id: Uuid,
        notification_type: NotificationType,
    ) -> Option<Duration> {
        let tracker = self.throttle_tracker.read();
        let last_sent = tracker.get(&(space_id, notification_type))?;
        let elapsed = Instant::now().duration_since(*last_sent);
        if elapsed < THROTTLE_WINDOW {
            debug!(
                space_id = %space_id,
                notification_type = ?notification_type,
                elapsed_secs = elapsed.as_secs_f64(),
                window_secs = THROTTLE_WINDOW.as_secs_f64(),
                "[MCPNotifier] ⏸️ Throttling {} notification (sent {:.1}s ago, window: {:.1}s)",
                match notification_type {
                    NotificationType::Tools => "tools/list_changed",
                    NotificationType::Prompts => "prompts/list_changed",
                    NotificationType::Resources => "resources/list_changed",
                    NotificationType::All => "batch (all types)",
                },
                elapsed.as_secs_f64(),
                THROTTLE_WINDOW.as_secs_f64()
            );
            Some(THROTTLE_WINDOW - elapsed)
        } else {
            None
        }
    }

    /// Schedule a one-shot re-dispatch for the end of the throttle window.
    ///
    /// At most one retry per `(space, type)` is in flight at a time. The
    /// `All` retry re-enters `notify_all_list_changed` with `force=true`
    /// because the dropped trigger may itself have been forced (binding /
    /// grant writes change the *effective* list without changing the
    /// space-level content hash); the typed retries re-run the normal
    /// hash-gated path, so a retry whose content settled back is dropped
    /// by the dedup rather than spamming the client.
    fn schedule_trailing_retry(
        &self,
        space_id: Uuid,
        notification_type: NotificationType,
        remaining: Duration,
    ) {
        {
            let mut pending = self.pending_retries.lock();
            if !pending.insert((space_id, notification_type)) {
                return; // a retry for this key is already scheduled
            }
        }
        debug!(
            space_id = %space_id,
            notification_type = ?notification_type,
            delay_ms = remaining.as_millis() as u64,
            "[MCPNotifier] ⏲️ Deferring throttled notification to window end"
        );
        let this = self.clone();
        tokio::spawn(async move {
            // Small epsilon past the window end so the re-check passes.
            tokio::time::sleep(remaining + Duration::from_millis(50)).await;
            this.pending_retries
                .lock()
                .remove(&(space_id, notification_type));
            match notification_type {
                NotificationType::All => this.notify_all_list_changed(space_id, true).await,
                NotificationType::Tools => this.notify_tools_list_changed(space_id).await,
                NotificationType::Prompts => this.notify_prompts_list_changed(space_id).await,
                NotificationType::Resources => this.notify_resources_list_changed(space_id).await,
            }
        });
    }

    /// Mark all notification types as just sent for a space
    /// Used by notify_all_list_changed() to prevent individual notifications from firing
    /// immediately after a batch notification
    fn mark_all_notification_types_sent(&self, space_id: Uuid, timestamp: Instant) {
        let mut tracker = self.throttle_tracker.write();
        tracker.insert((space_id, NotificationType::Tools), timestamp);
        tracker.insert((space_id, NotificationType::Prompts), timestamp);
        tracker.insert((space_id, NotificationType::Resources), timestamp);
        tracker.insert((space_id, NotificationType::All), timestamp);
    }

    /// Lazy GC for dead sessions.
    ///
    /// rmcp's `ServerHandler` doesn't expose a session-close callback, and
    /// the streamable-HTTP session manager owns the close path internally.
    /// What we *do* have on every `Peer<R>` is `is_transport_closed()` —
    /// it flips true once the underlying transport has terminated. So we
    /// reap lazily: every fanout / probe pass scans for closed peers and
    /// removes them from both `sessions` and `session_roots`.
    ///
    /// Returns the ids that were reaped (for logging / metrics). Callers
    /// pass the live (snapshot) list of `(session_id, peer)` they were
    /// about to iterate; this mutates `self.sessions` and the
    /// `feature_set_resolver`'s session registry.
    fn reap_dead_sessions(&self, snapshot: &[(String, Arc<Peer<RoleServer>>)]) -> Vec<String> {
        let dead: Vec<String> = snapshot
            .iter()
            .filter_map(|(sid, peer)| {
                if peer.is_transport_closed() {
                    Some(sid.clone())
                } else {
                    None
                }
            })
            .collect();
        if dead.is_empty() {
            return dead;
        }
        {
            let mut sessions = self.sessions.write();
            for sid in &dead {
                sessions.remove(sid);
            }
        }
        // Also clean the session_roots registry the resolver consults so
        // it doesn't keep returning stale roots / capability flags for
        // sessions that no longer exist.
        for sid in &dead {
            self.feature_set_resolver.session_roots().remove(sid);
        }
        info!(
            reaped = dead.len(),
            "[MCPNotifier] 🧹 reaped dead sessions (transport closed)"
        );
        dead
    }

    /// Start listening to domain events and notifying peers
    ///
    /// Spawns a background task that listens to DomainEvents and calls
    /// appropriate notification methods.
    pub fn start(self: Arc<Self>, mut event_rx: broadcast::Receiver<DomainEvent>) {
        let notifier = self.clone();
        tokio::spawn(async move {
            info!(
                "[MCPNotifier] ✅ Started listening for DomainEvents (throttle window: {}s)",
                THROTTLE_WINDOW.as_secs()
            );

            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        // Only log at trace level to reduce noise during startup
                        notifier.handle_event(event).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(
                            skipped_events = skipped,
                            "[MCPNotifier] ⚠️ Lagged behind, skipped {} events", skipped
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("[MCPNotifier] ❌ Event channel closed, stopping");
                        break;
                    }
                }
            }
        });
    }

    /// Handle a single domain event (SMART CONSUMER)
    ///
    /// Interprets domain events and decides what MCP notifications to send.
    /// This is enterprise-grade: consumers interpret events based on their context,
    /// not producers dictating what to do.
    async fn handle_event(&self, event: DomainEvent) {
        // Only handle events that affect MCP capabilities
        if !event.affects_mcp_capabilities() {
            trace!(
                event_type = event.type_name(),
                "[MCPNotifier] ⏭️ Skipping event (does not affect MCP capabilities)"
            );
            return;
        }

        match event {
            DomainEvent::FeatureSetMembersChanged {
                space_id,
                feature_set_id,
                ..
            } => {
                info!(
                    space_id = %space_id,
                    feature_set_id = %feature_set_id,
                    "[MCPNotifier] 📨 FeatureSetMembersChanged - notifying all clients in space"
                );
                self.notify_all_list_changed(space_id, true).await;
            }

            // Per-client grant changed — only the rootless-fallback path
            // consumes these grants, so we only need to notify peers
            // registered under this client_id. Bypass the space-wide fanout
            // (which would over-notify roots-capable peers in the space
            // whose resolution didn't change).
            DomainEvent::ClientGrantChanged {
                client_id,
                space_id,
            } => {
                info!(
                    %client_id,
                    %space_id,
                    "[MCPNotifier] 📨 ClientGrantChanged - notifying peer for this client"
                );
                self.notify_peer_lists_changed(&client_id).await;
            }

            // A workspace binding was created / updated / deleted. Notify
            // EVERY registered session, not just the ones resolving into the
            // event's space: a binding delete (or root edit) moves the
            // affected sessions OUT of that space — re-resolving them after
            // the mutation lands them in their fallback space, so a
            // space-filtered fanout can never reach exactly the sessions
            // whose toolset just changed. Binding writes are rare and
            // session counts are small; the blunt broadcast is cheaper than
            // reconstructing pre-mutation resolutions.
            DomainEvent::WorkspaceBindingChanged {
                space_id,
                workspace_root,
            } => {
                info!(
                    space_id = %space_id,
                    workspace_root = %workspace_root,
                    "[MCPNotifier] 📨 WorkspaceBindingChanged - notifying ALL sessions"
                );
                self.notify_all_sessions_lists_changed().await;
            }

            // A Space's built-in-server config changed (a built-in server or one
            // of its tools was toggled for this Space). Every session resolving
            // to this Space may now see a different tool list — re-push
            // list_changed to that Space's peers. `force=true` skips the
            // content hash because the change is in the meta-tool namespace,
            // which the per-space feature hash doesn't observe.
            DomainEvent::BuiltinServerConfigChanged { space_id } => {
                info!(
                    %space_id,
                    "[MCPNotifier] 📨 BuiltinServerConfigChanged - notifying clients in space"
                );
                self.notify_all_list_changed(space_id, true).await;
            }

            // A FeatureSet was deleted. Bindings/grants referencing it now
            // resolve to a smaller tool set, but the session still resolves
            // into the FS's Space, so the space-scoped fanout reaches it.
            DomainEvent::FeatureSetDeleted { space_id, .. } => {
                info!(
                    %space_id,
                    "[MCPNotifier] 📨 FeatureSetDeleted - notifying clients in space"
                );
                self.notify_all_list_changed(space_id, true).await;
            }

            // A Space was deleted. Its bindings cascade away, so affected
            // sessions re-resolve OUT of this (now-gone) space — a
            // space-filtered fanout can't reach them. Notify ALL sessions,
            // same rationale as WorkspaceBindingChanged.
            DomainEvent::SpaceDeleted { space_id } => {
                info!(
                    %space_id,
                    "[MCPNotifier] 📨 SpaceDeleted - notifying ALL sessions"
                );
                self.notify_all_sessions_lists_changed().await;
            }

            // ============ Backend Server Notifications (Pass-through with Throttling) ============
            // IMPORTANT: These events come from backend MCP servers. Some servers are "chatty" and
            // emit list_changed when queried (not just when features actually change). Our throttling
            // prevents infinite loops: Client query → Backend notification → Forward → Client refetch → Loop
            DomainEvent::ToolsChanged {
                server_id,
                space_id,
            } => {
                debug!(
                    server_id = %server_id,
                    space_id = %space_id,
                    "[MCPNotifier] 📨 ToolsChanged event from backend server {} (will check throttle)",
                    server_id
                );
                self.notify_tools_list_changed(space_id).await;
            }

            DomainEvent::PromptsChanged {
                server_id,
                space_id,
            } => {
                debug!(
                    server_id = %server_id,
                    space_id = %space_id,
                    "[MCPNotifier] 📨 PromptsChanged event from backend server {} (will check throttle)",
                    server_id
                );
                self.notify_prompts_list_changed(space_id).await;
            }

            DomainEvent::ResourcesChanged {
                server_id,
                space_id,
            } => {
                debug!(
                    server_id = %server_id,
                    space_id = %space_id,
                    "[MCPNotifier] 📨 ResourcesChanged event from backend server {} (will check throttle)",
                    server_id
                );
                self.notify_resources_list_changed(space_id).await;
            }

            // ============ Server Status Events ============
            DomainEvent::ServerStatusChanged {
                server_id,
                space_id,
                status,
                ..
            } => {
                use mcpmux_core::ConnectionStatus;

                // Disconnect AND reconnect both flip the per-feature
                // `is_available` flag, which `get_all_features_for_space`
                // filters on — so the content hash actually changes both
                // ways. We notify on each so the client's effective tool
                // list reflects "configured but unavailable" features
                // dropping out (on Disconnect) and coming back in (on
                // Connect). `force=false` lets the hash dedup absorb the
                // intermediate transient states (Connecting / Refreshing /
                // AuthRequired) without spamming.
                //
                // Loop concern (the old comment): a client `tools/list`
                // query that triggers a lazy backend connect would chain
                // Connected -> list_changed -> client refetch. Hashing
                // breaks that chain on the second iteration: the second
                // refetch sees the same hash as the first and dedupes.
                let should_notify = matches!(
                    status,
                    ConnectionStatus::Connected | ConnectionStatus::Disconnected
                );
                if should_notify {
                    info!(
                        server_id = %server_id,
                        space_id = %space_id,
                        status = ?status,
                        "[MCPNotifier] ServerStatusChanged ({:?}) - re-checking effective list",
                        status,
                    );
                    self.notify_all_list_changed(space_id, false).await;
                } else {
                    debug!(
                        server_id = %server_id,
                        space_id = %space_id,
                        status = ?status,
                        "[MCPNotifier] ServerStatusChanged - transient state, no notify"
                    );
                }

                if matches!(status, ConnectionStatus::Connected) {
                    let warmer = self.embedding_warmer.read().clone();
                    if let Some(warmer) = warmer {
                        warmer.warm_server(space_id, server_id.clone());
                    }
                }
            }

            DomainEvent::ServerFeaturesRefreshed {
                server_id,
                space_id,
                added,
                removed,
                ..
            } => {
                // Only log at debug - this fires for every server during startup
                debug!(
                    server_id = %server_id,
                    space_id = %space_id,
                    added = added.len(),
                    removed = removed.len(),
                    "[MCPNotifier] ServerFeaturesRefreshed"
                );
                self.notify_all_list_changed(space_id, false).await;
                let warmer = self.embedding_warmer.read().clone();
                if let Some(warmer) = warmer {
                    warmer.warm_server(space_id, server_id.clone());
                }
            }

            // Other events that affect MCP capabilities are handled above
            _ => {
                debug!(
                    event_type = event.type_name(),
                    "[MCPNotifier] Unhandled MCP-affecting event"
                );
            }
        }
    }

    /// Notify all peers in a space about all list types (tools/prompts/resources)
    ///
    /// **CRITICAL THROTTLING**: This method has aggressive throttling to prevent infinite loops.
    /// The 30-second window ensures that even if multiple backend servers emit notifications
    /// in rapid succession (e.g., when clients query them), we only forward one batch notification.
    ///
    /// **Important**: This method handles throttling at the batch level and marks
    /// all individual notification types as sent, preventing double-notifications
    /// when individual DomainEvent::ToolsChanged/etc. events arrive shortly after.
    ///
    /// **`force` parameter**: When `true`, skips content-based hash dedup. Used for
    /// grant-related events where the total features in the space haven't changed but
    /// the *effective* features visible to clients have (due to grant/feature set changes).
    /// The hash is computed from all features in the space, so it can't detect grant changes.
    async fn notify_all_list_changed(&self, space_id: Uuid, force: bool) {
        // 1. Content-Based Deduping (skipped when force=true)
        let tools_hash = self
            .calculate_feature_hash(space_id, FeatureType::Tool)
            .await;
        let prompts_hash = self
            .calculate_feature_hash(space_id, FeatureType::Prompt)
            .await;
        let resources_hash = self
            .calculate_feature_hash(space_id, FeatureType::Resource)
            .await;

        if !force {
            let any_changed = {
                let hashes = self.state_hashes.read();
                let t_changed = hashes
                    .get(&(space_id, NotificationType::Tools))
                    .is_none_or(|&h| h != tools_hash);
                let p_changed = hashes
                    .get(&(space_id, NotificationType::Prompts))
                    .is_none_or(|&h| h != prompts_hash);
                let r_changed = hashes
                    .get(&(space_id, NotificationType::Resources))
                    .is_none_or(|&h| h != resources_hash);
                t_changed || p_changed || r_changed
            };

            if !any_changed {
                debug!(space_id = %space_id, "[MCPNotifier] 🛑 Batch content unchanged, skipping");
                return;
            }
        } else {
            info!(space_id = %space_id, "[MCPNotifier] 🔓 Force-sending (grant/feature set change, bypassing hash dedup)");
        }

        let now = Instant::now();

        // CRITICAL: Check throttle FIRST before doing any work
        // This prevents cascade: Multiple events → Multiple batch calls → Multiple notifications → Loop
        // Throttled ≠ dropped: defer one re-send to the window end so the
        // second of two rapid mutations still reaches clients.
        if let Some(remaining) = self.throttle_remaining(space_id, NotificationType::All) {
            self.schedule_trailing_retry(space_id, NotificationType::All, remaining);
            return;
        }

        // Update the "All" throttle timestamp IMMEDIATELY to prevent concurrent calls
        // from also sending notifications
        {
            let mut tracker = self.throttle_tracker.write();
            tracker.insert((space_id, NotificationType::All), now);
        }

        info!(
            space_id = %space_id,
            window_secs = THROTTLE_WINDOW.as_secs(),
            "[MCPNotifier] 📤 Sending batch notification (tools + prompts + resources) - will throttle for {}s",
            THROTTLE_WINDOW.as_secs()
        );

        // Send all three types directly (bypassing individual throttles since we're
        // in a batch operation). Mark timestamps after sending to suppress subsequent
        // individual notifications.
        self.send_tools_list_changed(space_id, now).await;
        self.send_prompts_list_changed(space_id, now).await;
        self.send_resources_list_changed(space_id, now).await;

        // Update all hashes to prevent subsequent individual notifications
        {
            let mut hashes = self.state_hashes.write();
            hashes.insert((space_id, NotificationType::Tools), tools_hash);
            hashes.insert((space_id, NotificationType::Prompts), prompts_hash);
            hashes.insert((space_id, NotificationType::Resources), resources_hash);
        }

        // Mark all notification types as sent to suppress individual notifications
        // that might arrive shortly after this batch (within the throttle window)
        self.mark_all_notification_types_sent(space_id, now);

        info!(
            space_id = %space_id,
            "[MCPNotifier] ✅ Batch notification complete - all types marked as sent (throttled for {}s)",
            THROTTLE_WINDOW.as_secs()
        );
    }

    /// Notify all peers in a space that tools list has changed (with throttling and deduping)
    async fn notify_tools_list_changed(&self, space_id: Uuid) {
        // 1. Content-Based Deduping (Primary Defense)
        // Calculate current hash of tools
        let current_hash = self
            .calculate_feature_hash(space_id, FeatureType::Tool)
            .await;

        // Check against last known hash
        {
            let hashes = self.state_hashes.read();
            if let Some(&last_hash) = hashes.get(&(space_id, NotificationType::Tools)) {
                if last_hash == current_hash {
                    debug!(
                        space_id = %space_id,
                        hash = current_hash,
                        "[MCPNotifier] 🛑 Tools content unchanged, skipping notification"
                    );
                    return;
                }
            }
        }

        // 2. Throttling (Secondary Defense against Oscillation) — defer,
        // don't drop: the content hash above already proved a real change.
        if let Some(remaining) = self.throttle_remaining(space_id, NotificationType::Tools) {
            warn!(
                space_id = %space_id,
                "[MCPNotifier] ⚠️ Throttling rapid REAL tool changes (deferred to window end)"
            );
            self.schedule_trailing_retry(space_id, NotificationType::Tools, remaining);
            return;
        }

        let now = Instant::now();
        self.send_tools_list_changed(space_id, now).await;

        // 3. Update State (only after successful send)
        {
            let mut hashes = self.state_hashes.write();
            hashes.insert((space_id, NotificationType::Tools), current_hash);
        }

        {
            let mut tracker = self.throttle_tracker.write();
            tracker.insert((space_id, NotificationType::Tools), now);
        }
    }

    /// Internal method to actually send tools/list_changed notification (no throttling)
    async fn send_tools_list_changed(&self, space_id: Uuid, _timestamp: Instant) {
        // DEBUG: Kill switch to disable all notifications
        if DISABLE_ALL_NOTIFICATIONS {
            trace!(space_id = %space_id, "[MCPNotifier] 🚫 NOTIFICATIONS DISABLED - skipping tools/list_changed");
            return;
        }

        // Get sessions in this space with active streams, paired with
        // their session_id + client_id for per-push log attribution.
        let targets = self.get_peers_for_space_with_streams(space_id).await;

        if targets.is_empty() {
            debug!(
                space_id = %space_id,
                "[MCPNotifier] No sessions with active streams to notify about tools"
            );
            return;
        }

        info!(
            space_id = %space_id,
            session_count = targets.len(),
            "[MCPNotifier] 📤 Sending tools/list_changed to {} session(s) with active streams",
            targets.len()
        );

        let mut success_count = 0;
        let mut failure_count = 0;

        for (session_id, client_id, peer) in targets {
            match peer.notify_tool_list_changed().await {
                Ok(_) => {
                    success_count += 1;
                    debug!(
                        %session_id,
                        %client_id,
                        %space_id,
                        "[MCPNotifier] ✅ Sent tools/list_changed to session"
                    );
                }
                Err(e) => {
                    failure_count += 1;
                    warn!(
                        %session_id,
                        %client_id,
                        error = ?e,
                        "[MCPNotifier] Failed to send tools/list_changed to session"
                    );
                }
            }
        }

        if failure_count > 0 {
            warn!(
                space_id = %space_id,
                success = success_count,
                failed = failure_count,
                "[MCPNotifier] Completed tools/list_changed notifications with {} failures",
                failure_count
            );
        }
    }

    /// Get the sessions in `space_id` that have an active SSE stream and
    /// can therefore actually receive a notification.
    ///
    /// Session-keyed: iterates `sessions`, re-runs the FeatureSet resolver
    /// per session (same path as the request handlers), and returns the
    /// `(session_id, client_id, peer)` triples whose session resolves into
    /// `space_id`. Threading session_id through to the call site lets the
    /// log lines on each `peer.notify_*_list_changed()` prove *which*
    /// session got the push — important for verifying that two windows of
    /// the same client routing into different spaces don't cross-talk.
    async fn get_peers_for_space_with_streams(
        &self,
        space_id: Uuid,
    ) -> Vec<(String, String, Arc<Peer<RoleServer>>)> {
        let session_list: Vec<(String, String, Arc<Peer<RoleServer>>)> = {
            let sessions = self.sessions.read();
            sessions
                .iter()
                .filter(|(_, e)| e.has_active_stream)
                .map(|(sid, entry)| (sid.clone(), entry.client_id.clone(), entry.peer.clone()))
                .collect()
        };

        let dead = self.reap_dead_sessions(
            &session_list
                .iter()
                .map(|(sid, _, peer)| (sid.clone(), peer.clone()))
                .collect::<Vec<_>>(),
        );
        let dead_set: std::collections::HashSet<&str> = dead.iter().map(String::as_str).collect();

        let mut matching = Vec::new();

        for (session_id, client_id, peer) in session_list {
            if dead_set.contains(session_id.as_str()) {
                continue;
            }
            match self
                .feature_set_resolver
                .resolve(Some(&session_id), Some(&client_id), None)
                .await
            {
                Ok(resolved) if resolved.space_id == Some(space_id) => {
                    debug!(
                        %session_id,
                        %client_id,
                        %space_id,
                        "[MCPNotifier] Session in target space with active stream"
                    );
                    matching.push((session_id, client_id, peer));
                }
                Ok(resolved) => {
                    debug!(
                        %session_id,
                        %client_id,
                        resolved_space = ?resolved.space_id,
                        target_space = %space_id,
                        "[MCPNotifier] Session in different space, skipping"
                    );
                }
                Err(e) => {
                    warn!(
                        %session_id,
                        %client_id,
                        error = %e,
                        "[MCPNotifier] ⚠️ Failed to resolve space for session"
                    );
                }
            }
        }

        matching
    }

    /// Notify all peers in a space that prompts list has changed (with throttling and deduping)
    async fn notify_prompts_list_changed(&self, space_id: Uuid) {
        // 1. Content-Based Deduping
        let current_hash = self
            .calculate_feature_hash(space_id, FeatureType::Prompt)
            .await;

        {
            let hashes = self.state_hashes.read();
            if let Some(&last_hash) = hashes.get(&(space_id, NotificationType::Prompts)) {
                if last_hash == current_hash {
                    debug!(space_id = %space_id, "[MCPNotifier] 🛑 Prompts content unchanged");
                    return;
                }
            }
        }

        // 2. Throttling — defer, don't drop (content hash proved a change).
        if let Some(remaining) = self.throttle_remaining(space_id, NotificationType::Prompts) {
            self.schedule_trailing_retry(space_id, NotificationType::Prompts, remaining);
            return;
        }

        let now = Instant::now();
        self.send_prompts_list_changed(space_id, now).await;

        // 3. Update State
        self.state_hashes
            .write()
            .insert((space_id, NotificationType::Prompts), current_hash);
        self.throttle_tracker
            .write()
            .insert((space_id, NotificationType::Prompts), now);
    }

    /// Internal method to actually send prompts/list_changed notification (no throttling)
    async fn send_prompts_list_changed(&self, space_id: Uuid, _timestamp: Instant) {
        // DEBUG: Kill switch to disable all notifications
        if DISABLE_ALL_NOTIFICATIONS {
            trace!(space_id = %space_id, "[MCPNotifier] 🚫 NOTIFICATIONS DISABLED - skipping prompts/list_changed");
            return;
        }

        let targets = self.get_peers_for_space_with_streams(space_id).await;

        if targets.is_empty() {
            return;
        }

        info!(
            space_id = %space_id,
            session_count = targets.len(),
            "[MCPNotifier] 📤 Sending prompts/list_changed to {} session(s)",
            targets.len()
        );

        for (session_id, client_id, peer) in targets {
            match peer.notify_prompt_list_changed().await {
                Ok(_) => debug!(
                    %session_id,
                    %client_id,
                    %space_id,
                    "[MCPNotifier] ✅ Sent prompts/list_changed to session"
                ),
                Err(e) => warn!(
                    %session_id,
                    %client_id,
                    error = ?e,
                    "[MCPNotifier] Failed to send prompts/list_changed to session"
                ),
            }
        }
    }

    /// Notify all peers in a space that resources list has changed (with throttling and deduping)
    async fn notify_resources_list_changed(&self, space_id: Uuid) {
        // 1. Content-Based Deduping
        let current_hash = self
            .calculate_feature_hash(space_id, FeatureType::Resource)
            .await;

        {
            let hashes = self.state_hashes.read();
            if let Some(&last_hash) = hashes.get(&(space_id, NotificationType::Resources)) {
                if last_hash == current_hash {
                    debug!(space_id = %space_id, "[MCPNotifier] 🛑 Resources content unchanged");
                    return;
                }
            }
        }

        // 2. Throttling — defer, don't drop (content hash proved a change).
        if let Some(remaining) = self.throttle_remaining(space_id, NotificationType::Resources) {
            self.schedule_trailing_retry(space_id, NotificationType::Resources, remaining);
            return;
        }

        let now = Instant::now();
        self.send_resources_list_changed(space_id, now).await;

        // 3. Update State
        self.state_hashes
            .write()
            .insert((space_id, NotificationType::Resources), current_hash);
        self.throttle_tracker
            .write()
            .insert((space_id, NotificationType::Resources), now);
    }

    /// Internal method to actually send resources/list_changed notification (no throttling)
    async fn send_resources_list_changed(&self, space_id: Uuid, _timestamp: Instant) {
        // DEBUG: Kill switch to disable all notifications
        if DISABLE_ALL_NOTIFICATIONS {
            trace!(space_id = %space_id, "[MCPNotifier] 🚫 NOTIFICATIONS DISABLED - skipping resources/list_changed");
            return;
        }

        let targets = self.get_peers_for_space_with_streams(space_id).await;

        if targets.is_empty() {
            return;
        }

        info!(
            space_id = %space_id,
            session_count = targets.len(),
            "[MCPNotifier] 📤 Sending resources/list_changed to {} session(s)",
            targets.len()
        );

        for (session_id, client_id, peer) in targets {
            match peer.notify_resource_list_changed().await {
                Ok(_) => debug!(
                    %session_id,
                    %client_id,
                    %space_id,
                    "[MCPNotifier] ✅ Sent resources/list_changed to session"
                ),
                Err(e) => warn!(
                    %session_id,
                    %client_id,
                    error = ?e,
                    "[MCPNotifier] Failed to send resources/list_changed to session"
                ),
            }
        }
    }

    /// Send all three list_changed notifications to a single peer, bypassing
    /// the space-level hash dedup and throttle.
    ///
    /// Called when a *specific session's* feature-set resolution flips —
    /// e.g. workspace roots arrive after `initialize` and now match a
    /// binding, so the client's effective tool set differs from what it
    /// just fetched. The space-wide bridge can't catch this on its own:
    /// its hash is per-space, not per-resolved-FS, so a flip from the
    /// fallback FS to a bound FS doesn't change the space hash even though
    /// the client's view changed.
    pub async fn notify_peer_lists_changed(&self, client_id: &str) {
        if DISABLE_ALL_NOTIFICATIONS {
            trace!(%client_id, "[MCPNotifier] 🚫 disabled — skipping peer list_changed");
            return;
        }

        // A single client may hold several active sessions (multi-window
        // editors, parallel CLI invocations). Push the notification on
        // every active session for that client_id; client-side dedup is
        // their problem, but missing a session would be ours.
        let snapshot: Vec<(String, Arc<Peer<RoleServer>>)> = {
            let sessions = self.sessions.read();
            sessions
                .iter()
                .filter(|(_, e)| e.client_id == client_id && e.has_active_stream)
                .map(|(sid, e)| (sid.clone(), e.peer.clone()))
                .collect()
        };
        let dead = self.reap_dead_sessions(&snapshot);
        let dead_set: std::collections::HashSet<&str> = dead.iter().map(String::as_str).collect();
        let live: Vec<(String, Arc<Peer<RoleServer>>)> = snapshot
            .into_iter()
            .filter(|(sid, _)| !dead_set.contains(sid.as_str()))
            .collect();

        if live.is_empty() {
            debug!(
                %client_id,
                "[MCPNotifier] no active session — skipping peer list_changed"
            );
            return;
        }

        info!(
            %client_id,
            session_count = live.len(),
            "[MCPNotifier] 📤 per-client list_changed (resolution flipped or grant edited)"
        );

        for (session_id, peer) in &live {
            Self::push_lists_to_session(session_id, client_id, peer).await;
        }
    }

    /// Force-push all three list_changed notifications to EVERY registered
    /// session with an active stream, regardless of which space each
    /// session currently resolves into. Bypasses the space-level hash
    /// dedup and throttle: the trigger (a workspace-binding write) changes
    /// the *resolution*, which the per-space content hash cannot observe.
    async fn notify_all_sessions_lists_changed(&self) {
        if DISABLE_ALL_NOTIFICATIONS {
            trace!("[MCPNotifier] 🚫 disabled — skipping all-session list_changed");
            return;
        }

        let snapshot: Vec<(String, String, Arc<Peer<RoleServer>>)> = {
            let sessions = self.sessions.read();
            sessions
                .iter()
                .filter(|(_, e)| e.has_active_stream)
                .map(|(sid, e)| (sid.clone(), e.client_id.clone(), e.peer.clone()))
                .collect()
        };
        let dead = self.reap_dead_sessions(
            &snapshot
                .iter()
                .map(|(sid, _, peer)| (sid.clone(), peer.clone()))
                .collect::<Vec<_>>(),
        );
        let dead_set: HashSet<&str> = dead.iter().map(String::as_str).collect();
        let live: Vec<_> = snapshot
            .into_iter()
            .filter(|(sid, _, _)| !dead_set.contains(sid.as_str()))
            .collect();

        if live.is_empty() {
            debug!("[MCPNotifier] no active sessions — skipping all-session list_changed");
            return;
        }

        info!(
            session_count = live.len(),
            "[MCPNotifier] 📤 all-session list_changed (workspace binding changed)"
        );

        for (session_id, client_id, peer) in &live {
            Self::push_lists_to_session(session_id, client_id, peer).await;
        }
    }

    /// Push all three list_changed notifications to one session, logging
    /// each outcome. Shared by the per-client and all-session fanouts.
    async fn push_lists_to_session(session_id: &str, client_id: &str, peer: &Peer<RoleServer>) {
        match peer.notify_tool_list_changed().await {
            Ok(_) => debug!(
                %session_id,
                %client_id,
                "[MCPNotifier] ✅ Sent tools/list_changed to session (per-peer)"
            ),
            Err(e) => warn!(
                %session_id,
                %client_id,
                error = ?e,
                "[MCPNotifier] failed tools/list_changed"
            ),
        }
        match peer.notify_prompt_list_changed().await {
            Ok(_) => debug!(
                %session_id,
                %client_id,
                "[MCPNotifier] ✅ Sent prompts/list_changed to session (per-peer)"
            ),
            Err(e) => warn!(
                %session_id,
                %client_id,
                error = ?e,
                "[MCPNotifier] failed prompts/list_changed"
            ),
        }
        match peer.notify_resource_list_changed().await {
            Ok(_) => debug!(
                %session_id,
                %client_id,
                "[MCPNotifier] ✅ Sent resources/list_changed to session (per-peer)"
            ),
            Err(e) => warn!(
                %session_id,
                %client_id,
                error = ?e,
                "[MCPNotifier] failed resources/list_changed"
            ),
        }
    }
}
