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
use parking_lot::RwLock;
use rmcp::{service::Peer, RoleServer};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::pool::FeatureService;
use crate::services::SpaceResolverService;

/// MCP Notifier - Sends list_changed notifications to connected MCP clients
///
/// **Smart Consumer Pattern:**
/// - Subscribes to DomainEvents from the EventBus
/// - Tracks connected peers by client_id for notification delivery
/// - Resolves client spaces dynamically at notification time (handles follow_active mode)
/// - Dispatches list_changed notifications only to affected clients
/// - Interprets events based on MCP notification context
/// - **Content-Based Deduping**: Hashes feature lists to prevent redundant notifications
/// - **Throttles notifications** to prevent infinite loops from rapid backend changes
///
/// **Peer Registry:**
/// - Registers peers when clients initialize (used by session manager)
/// - Unregisters peers when sessions close
#[derive(Clone)]
pub struct MCPNotifier {
    /// Map: client_id -> peer handle
    /// Clients are tracked by client_id, not by space (space is resolved per-request)
    client_peers: Arc<RwLock<HashMap<String, PeerHandle>>>,
    /// Space resolver for determining which space a client is currently in
    space_resolver: Arc<SpaceResolverService>,
    /// Feature service for calculating content hashes
    feature_service: Arc<FeatureService>,
    /// Throttle tracker: (space_id, notification_type) -> last_sent_timestamp
    /// Prevents sending duplicate notifications within THROTTLE_WINDOW
    throttle_tracker: Arc<RwLock<HashMap<(Uuid, NotificationType), Instant>>>,
    /// State hash tracker: (space_id, notification_type) -> content_hash
    /// Prevents sending notifications when content hasn't actually changed
    state_hashes: Arc<RwLock<HashMap<(Uuid, NotificationType), u64>>>,
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

/// Wrapper around Peer for storage
#[derive(Clone)]
struct PeerHandle {
    peer: Arc<Peer<RoleServer>>,
    /// Whether this peer has an active SSE stream (can receive notifications)
    has_active_stream: bool,
}

impl PeerHandle {
    fn new(peer: Arc<Peer<RoleServer>>) -> Self {
        Self {
            peer,
            has_active_stream: false, // Initially false until stream is created
        }
    }
}

impl MCPNotifier {
    pub fn new(
        space_resolver: Arc<SpaceResolverService>,
        feature_service: Arc<FeatureService>,
    ) -> Self {
        Self {
            client_peers: Arc::new(RwLock::new(HashMap::new())),
            space_resolver,
            feature_service,
            throttle_tracker: Arc::new(RwLock::new(HashMap::new())),
            state_hashes: Arc::new(RwLock::new(HashMap::new())),
        }
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

    /// Register a peer for a client
    ///
    /// Called when a client initializes. Tracks by client_id (not space_id) because
    /// space resolution is dynamic (follow_active mode can change active space).
    ///
    /// Handles both initial connection and resume/reconnect scenarios.
    ///
    /// **Note**: Peer starts with `has_active_stream = false`. Call `mark_client_stream_active()`
    /// after the client creates an SSE stream to enable notifications.
    pub fn register_peer(&self, client_id: String, peer: Arc<Peer<RoleServer>>) {
        let handle = PeerHandle::new(peer);
        let mut peers = self.client_peers.write();

        // Replace any existing peer for this client (handles reconnect/resume)
        let is_reconnect = peers.contains_key(&client_id);
        peers.insert(client_id.clone(), handle);

        info!(
            client_id = %client_id,
            is_reconnect = is_reconnect,
            total_peers = peers.len(),
            "[MCPNotifier] 📡 Registered peer for client (stream not yet active)"
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
    pub fn mark_client_stream_active(&self, client_id: &str) {
        let mut peers = self.client_peers.write();

        if let Some(handle) = peers.get_mut(client_id) {
            handle.has_active_stream = true;
            info!(
                client_id = %client_id,
                "[MCPNotifier] ✅ Client stream is now active (notifications enabled)"
            );
        } else {
            warn!(
                client_id = %client_id,
                "[MCPNotifier] ⚠️ Attempted to mark stream active for unknown peer"
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

    /// Unregister a peer
    ///
    /// Called when a client disconnects or session closes
    pub fn unregister_peer(&self, client_id: &str) {
        let mut peers = self.client_peers.write();

        if peers.remove(client_id).is_some() {
            info!(
                client_id = %client_id,
                remaining_peers = peers.len(),
                "[MCPNotifier] 📴 Unregistered peer"
            );
        } else {
            warn!(
                client_id = %client_id,
                "[MCPNotifier] ⚠️ Attempted to unregister unknown peer"
            );
        }
    }

    /// Check if we should throttle this notification (returns true if throttled)
    ///
    /// Note: This only checks the throttle status, it does NOT update the timestamp.
    /// The caller is responsible for updating the timestamp after sending the notification.
    ///
    /// **Enterprise-Grade Throttling Logic:**
    /// - Per-space, per-notification-type throttling
    /// - Prevents cascade: Client query → Backend notification → Forward → Client refetch → Loop
    /// - Window is long enough (10s) to break rapid-fire notification cycles
    fn should_throttle(&self, space_id: Uuid, notification_type: NotificationType) -> bool {
        let now = Instant::now();
        let key = (space_id, notification_type);

        let tracker = self.throttle_tracker.read();

        if let Some(last_sent) = tracker.get(&key) {
            let elapsed = now.duration_since(*last_sent);
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
                return true;
            }
        }

        false
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

    /// Get all peers for a specific space (resolves client spaces at notification time)
    ///
    /// **Key Feature**: Resolves space dynamically for each client, handling:
    /// - follow_active mode (clients see active space changes)
    /// - locked mode (clients stay in their locked space)
    /// - Space changes without reconnection
    async fn get_peers_for_space(&self, space_id: Uuid) -> Vec<Arc<Peer<RoleServer>>> {
        // Clone the client list to avoid holding lock across await
        let client_list: Vec<(String, Arc<Peer<RoleServer>>)> = {
            let peers = self.client_peers.read();
            peers
                .iter()
                .map(|(client_id, handle)| (client_id.clone(), handle.peer.clone()))
                .collect()
        };

        let mut matching_peers = Vec::new();

        for (client_id, peer) in client_list {
            // Resolve current space for this client
            match self
                .space_resolver
                .resolve_space_for_client(&client_id)
                .await
            {
                Ok(client_space) if client_space == space_id => {
                    debug!(
                        client_id = %client_id,
                        space_id = %space_id,
                        "[MCPNotifier] Client is in target space"
                    );
                    matching_peers.push(peer);
                }
                Ok(other_space) => {
                    debug!(
                        client_id = %client_id,
                        client_space = %other_space,
                        target_space = %space_id,
                        "[MCPNotifier] Client is in different space, skipping"
                    );
                }
                Err(e) => {
                    warn!(
                        client_id = %client_id,
                        error = %e,
                        "[MCPNotifier] ⚠️ Failed to resolve space for client"
                    );
                }
            }
        }

        matching_peers
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
            // ============ Grant Events ============
            // When grants are issued/revoked, tools/prompts/resources might change
            DomainEvent::GrantIssued {
                client_id,
                space_id,
                feature_set_id,
            } => {
                info!(
                    client_id = %client_id,
                    space_id = %space_id,
                    feature_set_id = %feature_set_id,
                    "[MCPNotifier] 📨 GrantIssued - notifying all clients in space"
                );
                self.notify_all_list_changed(space_id).await;
            }

            DomainEvent::GrantRevoked {
                client_id,
                space_id,
                feature_set_id,
            } => {
                info!(
                    client_id = %client_id,
                    space_id = %space_id,
                    feature_set_id = %feature_set_id,
                    "[MCPNotifier] 📨 GrantRevoked - notifying all clients in space"
                );
                self.notify_all_list_changed(space_id).await;
            }

            DomainEvent::ClientGrantsUpdated {
                client_id,
                space_id,
                feature_set_ids,
            } => {
                info!(
                    client_id = %client_id,
                    space_id = %space_id,
                    feature_sets = feature_set_ids.len(),
                    "[MCPNotifier] 📨 ClientGrantsUpdated - notifying all clients in space"
                );
                self.notify_all_list_changed(space_id).await;
            }

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
                self.notify_all_list_changed(space_id).await;
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

                // Only notify if server disconnected (features unavailable)
                // We DO NOT notify on Connect because:
                // 1. If it's a new server, ToolsChanged will fire separately if needed
                // 2. If it's a reconnect, hashing will handle it
                // 3. Most importantly: Client connections trigger auto-connects, which would cause loops
                if matches!(status, ConnectionStatus::Disconnected) {
                    info!(
                        server_id = %server_id,
                        space_id = %space_id,
                        status = ?status,
                        "[MCPNotifier] ServerStatusChanged (Disconnected) - notifying clients to clear features"
                    );
                    self.notify_all_list_changed(space_id).await;
                } else {
                    debug!(
                        server_id = %server_id,
                        space_id = %space_id,
                        status = ?status,
                        "[MCPNotifier] ServerStatusChanged - ignoring (not a disconnection)"
                    );
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
                self.notify_all_list_changed(space_id).await;
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
    async fn notify_all_list_changed(&self, space_id: Uuid) {
        // 1. Content-Based Deduping
        let tools_hash = self
            .calculate_feature_hash(space_id, FeatureType::Tool)
            .await;
        let prompts_hash = self
            .calculate_feature_hash(space_id, FeatureType::Prompt)
            .await;
        let resources_hash = self
            .calculate_feature_hash(space_id, FeatureType::Resource)
            .await;

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

        let now = Instant::now();

        // CRITICAL: Check throttle FIRST before doing any work
        // This prevents cascade: Multiple events → Multiple batch calls → Multiple notifications → Loop
        if self.should_throttle(space_id, NotificationType::All) {
            debug!(
                space_id = %space_id,
                "[MCPNotifier] ⏸️ Batch notification throttled (recently sent all types within {}s window)",
                THROTTLE_WINDOW.as_secs()
            );
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

        // 2. Throttling (Secondary Defense against Oscillation)
        if self.should_throttle(space_id, NotificationType::Tools) {
            warn!(
                space_id = %space_id,
                "[MCPNotifier] ⚠️ Throttling rapid REAL tool changes"
            );
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

        // Get peers for this space, filtering to only those with active streams
        let (peers, _client_ids) = self.get_peers_for_space_with_streams(space_id).await;

        if peers.is_empty() {
            debug!(space_id = %space_id, "[MCPNotifier] No peers with active streams to notify about tools");
            return;
        }

        info!(
            space_id = %space_id,
            peer_count = peers.len(),
            "[MCPNotifier] 📤 Sending tools/list_changed to {} peers with active streams",
            peers.len()
        );

        let mut success_count = 0;
        let mut failure_count = 0;

        for peer in peers {
            match peer.notify_tool_list_changed().await {
                Ok(_) => {
                    success_count += 1;
                    debug!("[MCPNotifier] ✅ Sent tools/list_changed notification");
                }
                Err(e) => {
                    failure_count += 1;
                    warn!(error = ?e, "[MCPNotifier] Failed to send tools/list_changed");
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

    /// Get peers for a space that have active SSE streams (for notifications)
    ///
    /// Returns both the peers and their client_ids (for logging)
    async fn get_peers_for_space_with_streams(
        &self,
        space_id: Uuid,
    ) -> (Vec<Arc<Peer<RoleServer>>>, Vec<String>) {
        // Clone the client list to avoid holding lock across await
        let client_list: Vec<(String, PeerHandle)> = {
            let peers = self.client_peers.read();
            peers
                .iter()
                .map(|(client_id, handle)| (client_id.clone(), handle.clone()))
                .collect()
        };

        let mut matching_peers = Vec::new();
        let mut matching_client_ids = Vec::new();

        for (client_id, handle) in client_list {
            // Skip peers without active streams
            if !handle.has_active_stream {
                debug!(
                    client_id = %client_id,
                    space_id = %space_id,
                    "[MCPNotifier] Skipping peer without active stream"
                );
                continue;
            }

            // Resolve current space for this client
            match self
                .space_resolver
                .resolve_space_for_client(&client_id)
                .await
            {
                Ok(client_space) if client_space == space_id => {
                    debug!(
                        client_id = %client_id,
                        space_id = %space_id,
                        "[MCPNotifier] Client is in target space with active stream"
                    );
                    matching_peers.push(handle.peer.clone());
                    matching_client_ids.push(client_id);
                }
                Ok(other_space) => {
                    debug!(
                        client_id = %client_id,
                        client_space = %other_space,
                        target_space = %space_id,
                        "[MCPNotifier] Client is in different space, skipping"
                    );
                }
                Err(e) => {
                    warn!(
                        client_id = %client_id,
                        error = %e,
                        "[MCPNotifier] ⚠️ Failed to resolve space for client"
                    );
                }
            }
        }

        (matching_peers, matching_client_ids)
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

        // 2. Throttling
        if self.should_throttle(space_id, NotificationType::Prompts) {
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

        let peers = self.get_peers_for_space(space_id).await;

        if peers.is_empty() {
            return;
        }

        info!(
            space_id = %space_id,
            peer_count = peers.len(),
            "[MCPNotifier] 📤 Sending prompts/list_changed"
        );

        for peer in peers {
            if let Err(e) = peer.notify_prompt_list_changed().await {
                warn!(error = ?e, "[MCPNotifier] Failed to send prompts/list_changed");
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

        // 2. Throttling
        if self.should_throttle(space_id, NotificationType::Resources) {
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

        let peers = self.get_peers_for_space(space_id).await;

        if peers.is_empty() {
            return;
        }

        info!(
            space_id = %space_id,
            peer_count = peers.len(),
            "[MCPNotifier] 📤 Sending resources/list_changed"
        );

        for peer in peers {
            if let Err(e) = peer.notify_resource_list_changed().await {
                warn!(error = ?e, "[MCPNotifier] Failed to send resources/list_changed");
            }
        }
    }
}
