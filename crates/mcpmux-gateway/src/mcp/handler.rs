//! McpMux Gateway MCP Handler
//!
//! Implements the MCP ServerHandler trait to expose aggregated tools, prompts,
//! and resources from multiple backend MCP servers.

use anyhow::Result;
use rmcp::{
    model::*,
    service::{NotificationContext, RequestContext},
    ErrorData as McpError, RoleServer, ServerHandler,
};
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::context::{extract_oauth_context, extract_session_id, OAuthContext};
use crate::consumers::MCPNotifier;
use crate::server::ServiceContainer;

/// McpMux Gateway Handler
///
/// Routes MCP requests to appropriate backend services:
/// - Authorization via FeatureService (grants, spaces)
/// - Tool/prompt/resource routing via PoolService
/// - Server management via ServerManager
#[derive(Clone)]
pub struct McpMuxGatewayHandler {
    pub services: Arc<ServiceContainer>,
    pub notification_bridge: Arc<MCPNotifier>,
}

impl McpMuxGatewayHandler {
    pub fn new(services: Arc<ServiceContainer>, notification_bridge: Arc<MCPNotifier>) -> Self {
        Self {
            services,
            notification_bridge,
        }
    }

    /// Extract OAuth context from request extensions, with session fallback
    ///
    /// Tries to get OAuth context from headers first (injected by middleware).
    /// If headers are missing (e.g., client reconnected without auth), falls back
    /// to session metadata stored during initialization.
    fn get_oauth_context(&self, extensions: &Extensions) -> Result<OAuthContext> {
        // Try to get from headers first (preferred path)
        match extract_oauth_context(extensions) {
            Ok(ctx) => Ok(ctx),
            Err(e) => {
                // OAuth headers missing - client may need to re-authenticate
                // Note: This path should not be reachable since oauth_middleware blocks
                // requests without valid Authorization header
                warn!("OAuth headers missing: {}", e);

                Err(anyhow::anyhow!(
                    "OAuth context not available: headers missing. \
                     This should not happen - oauth_middleware should have blocked this request."
                ))
            }
        }
    }

    /// Negotiate protocol version between client and server.
    /// Returns the highest version both parties support.
    fn negotiate_protocol_version(&self, client_version_str: &str) -> ProtocolVersion {
        let our_max_version = ProtocolVersion::LATEST;
        let our_max_str = our_max_version.to_string();

        if client_version_str > our_max_str.as_str() {
            // Client is newer - respond with our maximum
            debug!(
                client_version = %client_version_str,
                our_max = %our_max_str,
                "Client uses newer protocol, negotiating down"
            );
            our_max_version
        } else {
            // Client version is compatible - use their version
            // Deserialize client version into ProtocolVersion
            serde_json::from_value(serde_json::Value::String(client_version_str.to_string()))
                .unwrap_or(our_max_version)
        }
    }

    /// Log resolver decision, emit `WorkspaceNeedsBinding` when a session
    /// reports roots but no binding matched (`source=Default`), and — when
    /// the session's resolved FS *flipped* from a prior value — fire a
    /// per-peer `list_changed` so the client re-pulls its tools.
    ///
    /// `notifier` is optional: callers from contexts where peer notification
    /// doesn't apply (e.g. rootless init paths) can pass `None`.
    ///
    /// Rootless sessions never trigger the binding prompt — there's nothing
    /// to bind (caller passes `root_for_prompt = None`).
    async fn log_and_notify_resolution(
        services: &std::sync::Arc<crate::server::ServiceContainer>,
        notifier: Option<&MCPNotifier>,
        client_id: &str,
        session_id: Option<&str>,
        root_for_prompt: Option<&str>,
    ) {
        let resolver = &services.feature_set_resolver;
        match resolver.resolve(session_id, Some(client_id)).await {
            Ok(resolved) => {
                info!(
                    %client_id,
                    session_id = session_id.unwrap_or("<none>"),
                    feature_set_ids = ?resolved.feature_set_ids,
                    space_id = resolved.space_id.map(|u| u.to_string()).unwrap_or_else(|| "<none>".into()),
                    source = ?resolved.source,
                    "[FeatureSetResolver] resolved",
                );

                // Track the resolved FS fingerprint per session so we can
                // detect flips. The very first sighting (no prior entry)
                // counts as a flip — that's the case where the client's
                // `tools/list` at init saw an empty/pending list but roots
                // arriving later may have landed on a binding. Firing once
                // on first sight is safe (idempotent re-list); the dedup
                // protects against repeated identical resolutions.
                if let (Some(sid), Some(notifier)) = (session_id, notifier) {
                    let changed = services
                        .session_roots
                        .record_resolution(sid, resolved.fingerprint().as_deref());
                    if changed {
                        notifier.notify_peer_lists_changed(client_id).await;
                    }
                }

                // Prompt only when the session reported a root but no
                // binding matched (`Deny` with a non-empty root_for_prompt).
                // PendingRoots / ClientGrant / WorkspaceBinding never
                // trigger the prompt.
                let should_prompt =
                    matches!(resolved.source, crate::services::ResolutionSource::Deny);
                if let (true, Some(sid), Some(space_id), Some(root)) = (
                    should_prompt,
                    session_id,
                    resolved.space_id,
                    root_for_prompt,
                ) {
                    services.gateway_state.read().await.emit_domain_event(
                        mcpmux_core::DomainEvent::WorkspaceNeedsBinding {
                            client_id: client_id.to_string(),
                            session_id: sid.to_string(),
                            space_id,
                            workspace_root: root.to_string(),
                        },
                    );
                }
            }
            Err(e) => {
                warn!(
                    %client_id,
                    error = %e,
                    "[FeatureSetResolver] resolve failed",
                );
            }
        }
    }

    /// Resolve the (Space, FeatureSet ids) the gateway should route a
    /// session through. The OAuth-context space is *not* used for routing
    /// — when a `WorkspaceBinding` matches, the binding's target space is
    /// authoritative and may differ from the OAuth-bound space (this is
    /// the whole point of workspace-root routing). Pass the returned
    /// `space_id` to every `feature_service.get_*_for_grants` /
    /// `routing_service.call_tool` invocation; otherwise the lookup queries
    /// the wrong space and returns 0 matches.
    async fn resolve_routing(
        &self,
        session_id: Option<&str>,
        client_id: &str,
    ) -> Result<(uuid::Uuid, Vec<String>), McpError> {
        let resolved = self
            .services
            .authorization_service
            .resolve(session_id, Some(client_id))
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to resolve: {e}"), None))?;
        let space_id = resolved.space_id.ok_or_else(|| {
            McpError::internal_error("No space resolved (no default space configured)", None)
        })?;
        Ok((space_id, resolved.feature_set_ids))
    }

    /// On-demand `roots/list` probe for sessions that initialized as
    /// roots-capable but have no roots yet — typically because the first
    /// `list_roots()` from `on_initialized` raced this request, or its
    /// retries are still mid-backoff after a transient failure.
    ///
    /// Without this, a roots-capable client that fires `tools/list`
    /// immediately after `notifications/initialized` resolves to
    /// `PendingRoots` and gets only the meta tools — even though we'd
    /// have the right answer milliseconds later. The 300 ms timeout
    /// caps the latency cost of bridging that gap; in steady state
    /// (`session_roots.get(sid)` already populated) this is a no-op
    /// early-return.
    ///
    /// Rate-limited per session to once per second so a burst of
    /// `tools/list` + `prompts/list` + `resources/list` doesn't fan out
    /// three parallel `peer.list_roots()` calls.
    async fn ensure_roots_probed(
        &self,
        peer: &rmcp::service::Peer<RoleServer>,
        session_id: Option<&str>,
        client_id: &str,
    ) {
        let Some(sid) = session_id else { return };
        // Fast path: already have a definitive answer (Some(roots),
        // possibly empty). No probe needed.
        if self.services.session_roots.get(sid).is_some() {
            return;
        }
        // Skip the probe only when we *know* this session is rootless
        // (`Some(false)`). When capability is unknown (`None` — we
        // haven't observed `notifications/initialized` for this session
        // yet, e.g. tools/list racing in before the notification's
        // handler completed), still try to probe: the worst case is one
        // wasted call on a genuinely rootless client where peer.list_roots()
        // returns method-not-found, vs. a stuck PendingRoots / empty
        // response on a client that *would* report roots if asked.
        if self.services.session_roots.is_roots_capable(sid) == Some(false) {
            return;
        }
        // Cool-down after a recent failed probe so we don't hammer a
        // peer whose previous list_roots() errored. Doesn't apply
        // when a probe is currently *running* — that's the
        // probe_lock's job below.
        if self
            .services
            .session_roots
            .should_throttle_probe(sid, std::time::Duration::from_secs(1))
        {
            return;
        }

        // Single-flight: serialize concurrent probes per session so a
        // burst of three list calls (tools/list + prompts/list +
        // resources/list within milliseconds) doesn't fan out three
        // upstream `peer.list_roots()` calls. The first request enters
        // the critical section, fires the probe, populates
        // session_roots; the second and third await the same lock,
        // then re-check session_roots and exit early.
        //
        // Without this, the followers used to skip the probe entirely
        // (boolean `claim_probe` flag) and resolve to PendingRoots —
        // exactly the empty-tools-list bug Claude Code's VS Code
        // extension was hitting.
        let lock = self.services.session_roots.probe_lock(sid);
        let _guard = lock.lock().await;

        // Recheck after acquiring the lock — the predecessor probe may
        // have already populated the registry.
        if self.services.session_roots.get(sid).is_some() {
            return;
        }

        const PROBE_BUDGET: std::time::Duration = std::time::Duration::from_millis(300);
        let outcome = tokio::time::timeout(PROBE_BUDGET, peer.list_roots()).await;
        // Stamp completion regardless of success/failure so the
        // sequential cool-down kicks in for the next caller.
        self.services.session_roots.mark_probe_completed(sid);
        match outcome {
            Ok(Ok(result)) => {
                let uris: Vec<String> = result.roots.iter().map(|r| r.uri.to_string()).collect();
                self.services
                    .session_roots
                    .set(sid, uris.iter().map(|s| s.as_str()));
                debug!(
                    %client_id,
                    session_id = %sid,
                    roots = ?uris,
                    "[FeatureSetResolver] on-demand probe populated roots",
                );
                // Notify the UI / re-emit `WorkspaceNeedsBinding` if the
                // session now resolves to Deny because of an unbound
                // root. Fire-and-forget so the request itself isn't
                // blocked on the desktop event bus.
                let services = self.services.clone();
                let notifier = self.notification_bridge.clone();
                let client_id = client_id.to_string();
                let session_id = sid.to_string();
                let root_for_prompt = uris
                    .into_iter()
                    .filter(|r| !r.is_empty())
                    .max_by_key(|r| r.len());
                tokio::spawn(async move {
                    services
                        .gateway_state
                        .read()
                        .await
                        .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);
                    Self::log_and_notify_resolution(
                        &services,
                        Some(&notifier),
                        &client_id,
                        Some(&session_id),
                        root_for_prompt.as_deref(),
                    )
                    .await;
                });
            }
            Ok(Err(e)) => {
                debug!(
                    %client_id,
                    session_id = %sid,
                    error = %e,
                    "[FeatureSetResolver] on-demand probe failed (will retry on next request after throttle)",
                );
            }
            Err(_elapsed) => {
                debug!(
                    %client_id,
                    session_id = %sid,
                    budget_ms = PROBE_BUDGET.as_millis(),
                    "[FeatureSetResolver] on-demand probe timed out (will retry on next request after throttle)",
                );
            }
        }
    }

    /// Build InitializeResult with negotiated protocol version
    fn build_initialize_result(&self, protocol_version: ProtocolVersion) -> InitializeResult {
        let info = self.get_info();
        let mut result = InitializeResult::new(info.capabilities);
        result.protocol_version = protocol_version;
        result.server_info = info.server_info;
        result.instructions = info.instructions;
        result
    }
}

impl ServerHandler for McpMuxGatewayHandler {
    fn get_info(&self) -> ServerInfo {
        use rmcp::model::{PromptsCapability, ResourcesCapability, ToolsCapability};

        // Note: get_info is called frequently, no logging needed

        let capabilities = ServerCapabilities::builder()
            .enable_tools_with(ToolsCapability {
                list_changed: Some(true),
            })
            .enable_prompts_with(PromptsCapability {
                list_changed: Some(true),
            })
            .enable_resources_with(ResourcesCapability {
                subscribe: Some(false),
                list_changed: Some(true),
            })
            .build();
        let mut server_info = Implementation::new("mcpmux-gateway", env!("CARGO_PKG_VERSION"));
        server_info.title = Some("McpMux".to_string());
        let mut info = ServerInfo::new(capabilities);
        info.server_info = server_info;
        info.instructions = Some(
            "McpMux aggregates multiple MCP servers. Use tools/prompts/resources \
             from your authorized backend servers."
                .to_string(),
        );
        info
    }

    async fn initialize(
        &self,
        params: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Negotiate protocol version
        let client_version_str = params.protocol_version.to_string();
        let negotiated_version = self.negotiate_protocol_version(&client_version_str);

        // Client initialization - log once
        debug!(
            client_id = %oauth_ctx.client_id,
            space_id = %oauth_ctx.space_id,
            protocol_version = %negotiated_version,
            "Client initializing"
        );

        Ok(self.build_initialize_result(negotiated_version))
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let oauth_ctx = match self.get_oauth_context(&context.extensions) {
            Ok(ctx) => ctx,
            Err(e) => {
                warn!("Failed to extract OAuth context on_initialized: {}", e);
                return;
            }
        };

        let peer = std::sync::Arc::new(context.peer);
        let session_id_for_register = extract_session_id(&context.extensions);

        // CRITICAL: stamp the roots capability **before any await** so the
        // resolver / probe paths see the right answer if a request from
        // this session arrives while we're still partway through this
        // handler. The race we hit before this reordering:
        //
        //     on_initialized starts
        //     register_session ✓
        //     await prime_hashes_for_space ← yields ~5ms
        //                                    tools/list races in here,
        //                                    is_roots_capable() == None,
        //                                    → resolver falls to "no roots
        //                                    + no grants — deny" (Tier 2),
        //                                    returns 4 meta tools
        //     await returns, now set_roots_capable(true) — too late
        //
        // Stamping synchronously up front guarantees that whatever else
        // tokio decides to schedule between here and the spawned
        // list_roots() task at the bottom, the resolver has the right
        // capability flag.
        if let Some(sid) = session_id_for_register.as_deref() {
            let declares_roots = peer
                .peer_info()
                .map(|info| info.capabilities.roots.is_some())
                .unwrap_or(false);
            self.services
                .session_roots
                .set_roots_capable(sid, declares_roots);
        }

        // Register the *session* with MCPNotifier so subsequent fanout can
        // re-resolve per session (a single OAuth client can hold multiple
        // sessions on different folders, each routing independently).
        if let Some(sid) = session_id_for_register.as_deref() {
            self.notification_bridge.register_session(
                sid.to_string(),
                oauth_ctx.client_id.clone(),
                peer.clone(),
            );
            // Mark the SSE stream as active immediately — RMCP's session
            // transport handles streaming + message caching internally.
            self.notification_bridge.mark_session_stream_active(sid);
        } else {
            warn!(
                client_id = %oauth_ctx.client_id,
                "[on_initialized] no mcp-session-id; skipping notifier registration (rare — stateless transport?)"
            );
        }

        // Pre-populate feature hashes to prevent spurious first notifications
        self.notification_bridge
            .prime_hashes_for_space(oauth_ctx.space_id)
            .await;

        // If the peer advertised the `roots` capability, fetch its reported
        // workspace roots into the session registry so the resolver can pick
        // a binding. Then log + (if no binding matched) prompt the UI.
        if let Some(session_id) = extract_session_id(&context.extensions) {
            // Capability already stamped at the top of this handler — read
            // it back rather than re-deriving so we stay consistent if the
            // peer_info() ever flapped during the await above.
            let declares_roots = self
                .services
                .session_roots
                .is_roots_capable(&session_id)
                .unwrap_or(false);
            // Persist the bit on the client row, *always* — the Clients UI
            // needs to distinguish "never observed" from "explicitly
            // rootless" so its capability badge isn't misleading on
            // newly-approved clients. The repo applies sticky-positive
            // semantics on `reports_roots` so a one-off rootless reconnect
            // doesn't bounce the badge.
            {
                let repo = self.services.dependencies.inbound_client_repo.clone();
                let cid = oauth_ctx.client_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = repo.mark_roots_capability(&cid, declares_roots).await {
                        debug!(
                            client_id = %cid,
                            error = %e,
                            "[on_initialized] mark_roots_capability failed (non-fatal)"
                        );
                    }
                });
            }
            if declares_roots {
                let peer_for_roots = peer.clone();
                let session_roots = self.services.session_roots.clone();
                let services = self.services.clone();
                let notifier = self.notification_bridge.clone();
                let client_id_str = oauth_ctx.client_id.clone();
                let session_id_for_task = session_id.clone();
                tokio::spawn(async move {
                    // Retry list_roots() on transport errors with bounded
                    // backoff. Without roots a roots-capable session is
                    // useless (resolver returns PendingRoots → empty
                    // tools list), so it's worth being aggressive about
                    // recovering from transient failures. Empty results
                    // (`Ok([])`) are NOT retried — that's a valid answer
                    // ("client has no folder open right now") and the
                    // client will notify us via `roots/list_changed` if
                    // they open one.
                    //
                    // Total budget ≈ 8.2 s wall-clock if every attempt
                    // hits a transport error before timing out.
                    const BACKOFFS_MS: &[u64] = &[100, 300, 800, 2000, 5000];
                    let max_attempts = BACKOFFS_MS.len() + 1; // 6 total = 1 initial + 5 retries
                    let mut attempt: usize = 0;
                    let result = loop {
                        match peer_for_roots.list_roots().await {
                            Ok(r) => break Some(r),
                            Err(e) => {
                                attempt += 1;
                                if attempt >= max_attempts {
                                    warn!(
                                        client_id = %client_id_str,
                                        session_id = %session_id_for_task,
                                        attempts = attempt,
                                        error = %e,
                                        "[FeatureSetResolver] peer.list_roots() exhausted retries; session left unresolved (next list/get request will re-probe)",
                                    );
                                    break None;
                                }
                                let backoff = BACKOFFS_MS[attempt - 1];
                                warn!(
                                    client_id = %client_id_str,
                                    session_id = %session_id_for_task,
                                    attempt,
                                    max_attempts,
                                    next_backoff_ms = backoff,
                                    error = %e,
                                    "[FeatureSetResolver] peer.list_roots() failed; retrying after backoff",
                                );
                                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                            }
                        }
                    };

                    let Some(result) = result else { return };

                    let uris: Vec<String> =
                        result.roots.iter().map(|r| r.uri.to_string()).collect();
                    session_roots.set(&session_id_for_task, uris.iter().map(|s| s.as_str()));
                    debug!(
                        client_id = %client_id_str,
                        session_id = %session_id_for_task,
                        roots = ?uris,
                        attempts = attempt + 1,
                        "[FeatureSetResolver] fetched MCP roots",
                    );

                    // Tell the desktop UI the detected-roots list may
                    // have grown so the Workspaces tab refreshes
                    // without waiting for a polling cycle.
                    services
                        .gateway_state
                        .read()
                        .await
                        .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);

                    // Pick the longest (most specific) normalized
                    // root for the sheet. The resolver has already
                    // normalized them on insert. Passing `Some(root)`
                    // lets log_and_notify_resolution emit
                    // `WorkspaceNeedsBinding` if the resolver ended
                    // up at `source = Deny` (i.e. no binding yet).
                    let root_for_prompt =
                        session_roots.get(&session_id_for_task).and_then(|roots| {
                            roots
                                .into_iter()
                                .filter(|r| !r.is_empty())
                                .max_by_key(|r| r.len())
                        });

                    Self::log_and_notify_resolution(
                        &services,
                        Some(&notifier),
                        &client_id_str,
                        Some(&session_id_for_task),
                        root_for_prompt.as_deref(),
                    )
                    .await;
                });
            } else {
                // No roots declared — silent default, never prompt
                // (root_for_prompt = None suppresses the emit).
                Self::log_and_notify_resolution(
                    &self.services,
                    Some(&self.notification_bridge),
                    &oauth_ctx.client_id,
                    Some(&session_id),
                    None,
                )
                .await;
            }
        }

        info!(
            client_id = %oauth_ctx.client_id,
            space_id = %oauth_ctx.space_id,
            "Client initialized - peer registered for notifications"
        );
    }

    /// The client told us its roots list changed (e.g. VS Code added a
    /// folder to a multi-root workspace). Re-fetch via `list_roots`,
    /// update the session registry, and re-run the resolver — if any root
    /// is still unbound, `log_and_notify_resolution` fires a fresh
    /// `WorkspaceNeedsBinding` so the sheet pops for the newly-surfaced
    /// folder.
    async fn on_roots_list_changed(&self, context: NotificationContext<RoleServer>) {
        let oauth_ctx = match self.get_oauth_context(&context.extensions) {
            Ok(ctx) => ctx,
            Err(e) => {
                warn!(
                    "Failed to extract OAuth context on_roots_list_changed: {}",
                    e
                );
                return;
            }
        };
        let Some(session_id) = extract_session_id(&context.extensions) else {
            debug!("[FeatureSetResolver] roots/list_changed with no session id — skipping");
            return;
        };
        let peer = std::sync::Arc::new(context.peer);
        let session_roots = self.services.session_roots.clone();
        let services = self.services.clone();
        let notifier = self.notification_bridge.clone();
        let client_id_str = oauth_ctx.client_id.clone();
        let session_id_for_task = session_id.clone();
        tokio::spawn(async move {
            match peer.list_roots().await {
                Ok(result) => {
                    let uris: Vec<String> =
                        result.roots.iter().map(|r| r.uri.to_string()).collect();
                    session_roots.set(&session_id_for_task, uris.iter().map(|s| s.as_str()));
                    debug!(
                        client_id = %client_id_str,
                        session_id = %session_id_for_task,
                        roots = ?uris,
                        "[FeatureSetResolver] refreshed MCP roots (roots/list_changed)",
                    );
                    services
                        .gateway_state
                        .read()
                        .await
                        .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);

                    let root_for_prompt =
                        session_roots.get(&session_id_for_task).and_then(|roots| {
                            roots
                                .into_iter()
                                .filter(|r| !r.is_empty())
                                .max_by_key(|r| r.len())
                        });
                    Self::log_and_notify_resolution(
                        &services,
                        Some(&notifier),
                        &client_id_str,
                        Some(&session_id_for_task),
                        root_for_prompt.as_deref(),
                    )
                    .await;
                }
                Err(e) => {
                    debug!(
                        client_id = %client_id_str,
                        session_id = %session_id_for_task,
                        error = %e,
                        "[FeatureSetResolver] refresh list_roots failed — silent",
                    );
                }
            }
        });
    }

    async fn list_tools(
        &self,
        _params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let session_id_owned = extract_session_id(&context.extensions);
        // Bridge the init race: roots-capable sessions whose first
        // `list_roots()` raced this request get a one-shot 300 ms probe
        // here so they end up at the right routing decision instead of
        // empty (PendingRoots). Throttled per session.
        self.ensure_roots_probed(
            &context.peer,
            session_id_owned.as_deref(),
            &oauth_ctx.client_id,
        )
        .await;
        // Resolve routing once: the resolver returns the authoritative
        // (Space, FS) for this session — this may differ from oauth_ctx
        // when a WorkspaceBinding redirects to another space.
        let (space_id, feature_set_ids) = self
            .resolve_routing(session_id_owned.as_deref(), &oauth_ctx.client_id)
            .await?;

        // Get tools via FeatureService — using the *resolved* space.
        let tools = self
            .services
            .pool_services
            .feature_service
            .get_tools_for_grants(&space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get tools: {}", e), None))?;

        // Convert to MCP Tool types with qualified names (prefix.tool_name)
        let mut mcp_tools: Vec<Tool> = tools
            .iter()
            .filter_map(|f| {
                f.raw_json.as_ref().and_then(|json| {
                    let mut tool: Tool = serde_json::from_value(json.clone()).ok()?;
                    // Replace name with qualified name (prefix.tool_name)
                    tool.name = f.qualified_name().into();
                    Some(tool)
                })
            })
            .collect();

        // Append the resolved Space's built-in `mcpmux_*` (Tool Optimization)
        // tools. The set is empty when that built-in server is disabled for the
        // Space, and any individual tools the Space has turned off are filtered
        // out — all configured per Space via the Built-in Servers tab.
        mcp_tools.extend(
            self.services
                .meta_tool_registry
                .list_as_tools_for_space(&space_id)
                .await,
        );

        // Log tool names at DEBUG level for visibility
        let tool_names: Vec<String> = mcp_tools.iter().map(|t| t.name.to_string()).collect();
        debug!(
            count = mcp_tools.len(),
            tools = ?tool_names,
            "list_tools"
        );

        Ok(ListToolsResult::with_all_items(mcp_tools))
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Tool calls are important - log at INFO
        info!(
            tool = %params.name,
            client = %&oauth_ctx.client_id[..oauth_ctx.client_id.len().min(12)],
            "call_tool"
        );

        let session_id_owned = extract_session_id(&context.extensions);
        let session_id = session_id_owned.as_deref();

        // Resolve routing once — the binding's target space is authoritative
        // (may differ from oauth_ctx.space_id). Needed both to gate the
        // per-Space meta tools below and to route a normal tool call.
        let (space_id, feature_set_ids) = self
            .resolve_routing(session_id, &oauth_ctx.client_id)
            .await?;

        // Intercept meta tools (mcpmux_*) BEFORE feature-set filtering, gated
        // by the resolved Space's built-in config. When the Tool Optimization
        // server (or this specific tool) is disabled for the Space we fall
        // through to the feature-set path, where the tool misses and surfaces a
        // normal "not found" error.
        if crate::services::is_meta_tool(&params.name)
            && self.services.meta_tool_registry.contains(&params.name)
            && self
                .services
                .meta_tool_registry
                .is_tool_enabled_for_space(&space_id, &params.name)
                .await
        {
            // Note: client_id is the OAuth client identity (a URL for DCR-
            // registered clients like Claude, a UUID for others). The meta-
            // tool registry treats it as an opaque string identity key.
            let args: serde_json::Value = params
                .arguments
                .map(|a| serde_json::to_value(a).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            return match self
                .services
                .meta_tool_registry
                .call(&params.name, &oauth_ctx.client_id, session_id, args)
                .await
            {
                Ok(result) => Ok(result),
                Err(e) => Ok(e.into_call_tool_result()),
            };
        }

        // Call tool via routing service (handles auth and routing)
        let tool_result = self
            .services
            .pool_services
            .routing_service
            .call_tool(
                space_id,
                &feature_set_ids,
                &params.name,
                serde_json::to_value(params.arguments.unwrap_or_default()).unwrap_or_default(),
            )
            .await
            .map_err(|e| McpError::internal_error(format!("Tool call failed: {}", e), None))?;

        // Convert ToolCallResult to MCP CallToolResult
        let content: Vec<Content> = tool_result
            .content
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        // Log result summary - show content types and approximate sizes
        let content_summary: Vec<String> = content
            .iter()
            .map(|c| {
                // Content is Annotated<RawContent>, serialize to inspect type
                if let Ok(json) = serde_json::to_value(c) {
                    let content_type = json
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown");
                    match content_type {
                        "text" => {
                            let len = json
                                .get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.len())
                                .unwrap_or(0);
                            format!("text({}c)", len)
                        }
                        "image" => {
                            let mime = json.get("mimeType").and_then(|m| m.as_str()).unwrap_or("?");
                            format!("image({})", mime)
                        }
                        "resource" => {
                            let uri = json
                                .get("resource")
                                .and_then(|r| r.get("uri"))
                                .and_then(|u| u.as_str())
                                .unwrap_or("?");
                            format!("resource({})", uri)
                        }
                        _ => content_type.to_string(),
                    }
                } else {
                    "?".to_string()
                }
            })
            .collect();
        debug!(
            tool = %params.name,
            is_error = tool_result.is_error,
            content = ?content_summary,
            "call_tool result"
        );

        let result = if tool_result.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };

        Ok(result)
    }

    async fn list_prompts(
        &self,
        _params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let session_id_owned = extract_session_id(&context.extensions);
        self.ensure_roots_probed(
            &context.peer,
            session_id_owned.as_deref(),
            &oauth_ctx.client_id,
        )
        .await;
        let (space_id, feature_set_ids) = self
            .resolve_routing(session_id_owned.as_deref(), &oauth_ctx.client_id)
            .await?;

        let prompts = self
            .services
            .pool_services
            .feature_service
            .get_prompts_for_grants(&space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get prompts: {}", e), None))?;

        // Convert to MCP Prompt types with qualified names (prefix.prompt_name)
        let mcp_prompts: Vec<Prompt> = prompts
            .iter()
            .filter_map(|f| {
                f.raw_json.as_ref().and_then(|json| {
                    let mut prompt: Prompt = serde_json::from_value(json.clone()).ok()?;
                    // Replace name with qualified name (prefix.prompt_name)
                    prompt.name = f.qualified_name();
                    Some(prompt)
                })
            })
            .collect();

        // Log prompt names at DEBUG level
        let prompt_names: Vec<String> = mcp_prompts.iter().map(|p| p.name.to_string()).collect();
        debug!(
            count = mcp_prompts.len(),
            prompts = ?prompt_names,
            "list_prompts"
        );

        Ok(ListPromptsResult::with_all_items(mcp_prompts))
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let (space_id, feature_set_ids) = self
            .resolve_routing(
                extract_session_id(&context.extensions).as_deref(),
                &oauth_ctx.client_id,
            )
            .await?;

        let (server_id, prompt_name) = self
            .services
            .pool_services
            .feature_service
            .parse_qualified_prompt_name(&space_id.to_string(), &params.name)
            .await
            .map_err(|e| McpError::invalid_params(format!("Invalid prompt name: {}", e), None))?;

        let authorized_prompts = self
            .services
            .pool_services
            .feature_service
            .get_prompts_for_grants(&space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to verify authorization: {}", e), None)
            })?;

        let is_authorized = authorized_prompts
            .iter()
            .any(|p| p.server_id == server_id && p.feature_name == prompt_name && p.is_available);

        if !is_authorized {
            return Err(McpError::invalid_params(
                format!("Prompt '{}' not authorized", params.name),
                None,
            ));
        }

        let result_value = self
            .services
            .pool_services
            .pool_service
            .get_prompt(space_id, &server_id, &prompt_name, params.arguments)
            .await
            .map_err(|e| McpError::internal_error(format!("Get prompt failed: {}", e), None))?;

        // Deserialize the Value into GetPromptResult
        let result: GetPromptResult = serde_json::from_value(result_value).map_err(|e| {
            McpError::internal_error(format!("Failed to parse prompt result: {}", e), None)
        })?;

        Ok(result)
    }

    async fn list_resources(
        &self,
        _params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let session_id_owned = extract_session_id(&context.extensions);
        self.ensure_roots_probed(
            &context.peer,
            session_id_owned.as_deref(),
            &oauth_ctx.client_id,
        )
        .await;
        let (space_id, feature_set_ids) = self
            .resolve_routing(session_id_owned.as_deref(), &oauth_ctx.client_id)
            .await?;

        let resources = self
            .services
            .pool_services
            .feature_service
            .get_resources_for_grants(&space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to get resources: {}", e), None)
            })?;

        let mcp_resources: Vec<Resource> = resources
            .iter()
            .filter_map(|f| {
                f.raw_json
                    .as_ref()
                    .and_then(|json| serde_json::from_value(json.clone()).ok())
            })
            .collect();

        // Log resource URIs at DEBUG level
        let resource_uris: Vec<String> = mcp_resources.iter().map(|r| r.uri.to_string()).collect();
        debug!(
            count = mcp_resources.len(),
            resources = ?resource_uris,
            "list_resources"
        );

        Ok(ListResourcesResult::with_all_items(mcp_resources))
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let (space_id, feature_set_ids) = self
            .resolve_routing(
                extract_session_id(&context.extensions).as_deref(),
                &oauth_ctx.client_id,
            )
            .await?;

        let server_id = self
            .services
            .pool_services
            .feature_service
            .find_server_for_resource(&space_id.to_string(), &params.uri)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to resolve resource: {}", e), None)
            })?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Resource '{}' not found", params.uri), None)
            })?;

        let authorized_resources = self
            .services
            .pool_services
            .feature_service
            .get_resources_for_grants(&space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to verify authorization: {}", e), None)
            })?;

        let is_authorized = authorized_resources
            .iter()
            .any(|r| r.server_id == server_id && r.feature_name == params.uri && r.is_available);

        if !is_authorized {
            return Err(McpError::invalid_params(
                format!("Resource '{}' not authorized", params.uri),
                None,
            ));
        }

        let contents_values = self
            .services
            .pool_services
            .pool_service
            .read_resource(space_id, &server_id, &params.uri)
            .await
            .map_err(|e| McpError::internal_error(format!("Read resource failed: {}", e), None))?;

        // Convert Vec<Value> to Vec<ResourceContents>
        let contents: Vec<ResourceContents> = contents_values
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        Ok(ReadResourceResult::new(contents))
    }

    /// Override on_custom_request to handle "initialize" with flexible protocol negotiation
    ///
    /// Clients may send newer protocol versions with capability structures we don't recognize.
    /// Instead of failing deserialization, we extract only the required fields and respond
    /// with our maximum supported version, allowing graceful protocol negotiation.
    async fn on_custom_request(
        &self,
        request: CustomRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, McpError> {
        if request.method == "initialize" {
            warn!("[MCP] ⚠️  Initialize came as CustomRequest - protocol version mismatch likely");

            let oauth_ctx = self
                .get_oauth_context(&context.extensions)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

            let params_value = request.params.ok_or_else(|| {
                McpError::invalid_params("Initialize request missing params".to_string(), None)
            })?;

            // Extract client version and info from raw JSON
            let client_version_str = params_value
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let client_info: Option<Implementation> = params_value
                .get("clientInfo")
                .and_then(|v| serde_json::from_value(v.clone()).ok());

            // Use shared negotiation logic
            let negotiated_version = self.negotiate_protocol_version(client_version_str);

            info!(
                client_id = %oauth_ctx.client_id,
                space_id = %oauth_ctx.space_id,
                client_info = ?client_info,
                protocol_version = %negotiated_version,
                "[MCP] 🔌 Client initializing with flexible negotiation"
            );

            // Build response using shared logic
            let result = self.build_initialize_result(negotiated_version);

            match serde_json::to_value(result) {
                Ok(json) => return Ok(CustomResult::new(json)),
                Err(e) => {
                    return Err(McpError::internal_error(
                        format!("Failed to serialize initialize result: {}", e),
                        None,
                    ))
                }
            }
        }

        // For other custom requests, return method not found
        Err(McpError::new(
            ErrorCode::METHOD_NOT_FOUND,
            request.method,
            None,
        ))
    }
}
