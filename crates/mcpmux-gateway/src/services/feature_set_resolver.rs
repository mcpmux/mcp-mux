//! FeatureSet Resolver Service.
//!
//! Binding-canonical resolution. The resolver answers one question: *which
//! [`WorkspaceBinding`](mcpmux_core::WorkspaceBinding) (if any) matches the
//! signals this caller presents?* Signals are ranked; none are mandatory:
//!
//! ```text
//! resolve(session_id, client_id):
//!     // Signal 1 — reported root (deprecated MCP primitive, SEP-2577)
//!     if session reported roots AND a binding matches:
//!         return (binding.space_id, [binding.feature_set_id], WorkspaceBinding)
//!
//!     // Tier 1b — roots reported, no binding matched
//!     if session reported roots AND no binding matched:
//!         if roots_capable == false (true rootless):
//!             fall through to Tier 3 (declare-root gate + grant lookup)
//!         else:
//!             return (scoped_space, [], Unbound)   // deny by default
//!             // (upstream emits WorkspaceNeedsBinding so the user can bind)
//!
//!     // Tier 1c — declared `roots` but they haven't arrived yet
//!     if session declared `roots` AND none yet in registry:
//!         if within the pending-roots grace window:
//!             return ([], default_space, PendingRoots)   // wait for the root
//!         else:
//!             return (default_space, [], Unbound)   // gave up waiting
//!
//!     // Signal 2 — client identity (id-type binding, rootless)
//!     if client has an id-type WorkspaceBinding:
//!         return (binding.space_id, binding.feature_set_ids, WorkspaceBinding)
//!
//!     // Signal 3 — client identity (rootless-by-design: Claude.ai web, …)
//!     if client has grants in the default space:
//!         if rootless session AND no declared root yet:
//!             return ([], default_space, PendingRoots)   // meta tools only
//!         else:
//!             return (default_space, grants, ClientGrant)
//!
//!     // Tier 4 — no roots, no id binding, no grants
//!     return (default_space, [], Unbound)
//! ```
//!
//! Signal 3 (machine) scopes binding lookup. When the client sends
//! `X-Mcpmux-Machine-Id`, only that machine and global bindings are
//! considered — gateway `local_machine_id` and `inbound_clients.machine_id`
//! are skipped so a tunneled caller is not mistaken for the gateway host.
//! Without the header: client machine → local machine → global.
//!
//! ## Deny by default
//!
//! When no binding matches — an unmapped folder (Tier 1b), a rootless client
//! with no grants, or a roots-capable client that never reported a folder
//! (Tier 1c after the grace window) — the resolver returns
//! [`ResolutionSource::Unbound`] with empty `feature_set_ids`. Callers get
//! zero backend tools until they have an explicit binding (or a client grant
//! for rootless clients). Meta management tools are appended unconditionally
//! by the request handler.
//!
//! ## Grace window — avoid "empty then mapped" flips
//!
//! A roots-capable client that's *about* to report a folder must resolve
//! straight to that folder's binding (or `Unbound`), never flash tools first
//! and then flip. So while a session has declared (or might declare) `roots`
//! and none have arrived yet, the resolver holds at `PendingRoots` (empty) for
//! a short grace window. Only once the window lapses with no root in sight
//! does it settle on `Unbound`. A roots-capable session **never** falls
//! through to another client's grants — after the grace it goes straight to
//! `Unbound`, preserving per-session isolation.
//!
//! The caller's client identity is used **only** for the rootless Tier-2 grant
//! lookup — every roots-capable session routes via its own reported roots,
//! regardless of which OAuth client opened it. This is what makes "two VS Code
//! windows sharing one OAuth identity" route independently.
//!
//! # Trust model (deliberate design decision)
//!
//! Roots are client-asserted, so **any** OAuth-approved local client can
//! report roots matching **any** binding and be routed into that binding's
//! Space — including its tools and the credentials behind them
//! (SECURITY_AUDIT HIGH-1, accepted 2026-06-12). The boundary is the
//! one-time client approval: every client the user approves is trusted with
//! every workspace binding, the same way it is trusted with the local
//! filesystem it already runs on. The gateway binds to loopback only, and
//! unapproved clients never get a token. If per-client Space isolation is
//! ever needed, gate Tier 1 on a per-`(client, space)` trust table before
//! honoring the binding.
//!
//! Roots-capable detection is stamped at `on_initialized` time into
//! [`SessionRootsRegistry::set_roots_capable`].
//!
//! # Explicit workspace root via the `X-Mcpmux-Workspace` header
//!
//! A connection can carry an explicit workspace root in the
//! `X-Mcpmux-Workspace` HTTP header, injected by McpMux's per-workspace client
//! configs. The OAuth middleware pins it into
//! [`SessionRootsRegistry::set_pinned`], where it **shadows** the client's
//! probed MCP roots in [`SessionRootsRegistry::get`]. Because this resolver
//! reads roots exclusively through `get`, a pinned root flows through Tier 1
//! unchanged — an exact binding match, else the Space default — with no extra
//! tier or parameter. This is the deterministic path for clients that don't
//! report `roots` reliably (e.g. Cursor multiplexing one MCP host across
//! windows): the header always wins over a stale or absent reported root.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use mcpmux_core::{
    FeatureSetRepository, SpaceBaseDirRepository, SpaceRepository, WorkspaceBindingRepository,
};
use mcpmux_storage::InboundClientRepository;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use super::session_roots::SessionRootsRegistry;

/// How long a session that's declared (or might declare) the `roots`
/// capability is held at [`ResolutionSource::PendingRoots`] before the
/// resolver gives up waiting and falls back to the Space default. Sized to
/// comfortably outlast a well-behaved client's `initialize` →
/// `roots/list` round-trip (typically sub-second) so the grace only ever
/// catches clients that declared `roots` but never actually report one.
const DEFAULT_PENDING_ROOTS_GRACE: Duration = Duration::from_secs(5);

/// Why the resolver picked the FS(es) it picked (or didn't pick any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
// SpaceDefault is retained for serde compat; it is no longer produced but may appear
// in deserialized data from older gateway versions. The unused-variant pattern is
// intentional, not an oversight.
#[allow(clippy::manual_non_exhaustive)]
pub enum ResolutionSource {
    /// A [`WorkspaceBinding`](mcpmux_core::WorkspaceBinding) matched one of
    /// the session's reported MCP roots.
    WorkspaceBinding,
    /// No binding matched, but the client is roots-capable so its `roots`
    /// list is in flight; return empty and re-resolve when they arrive.
    PendingRoots,
    /// Rootless-by-design client. The space-default's per-client
    /// `client_grants` were applied.
    ClientGrant,
    /// No binding matched; deny by default (empty `feature_set_ids`). Carries
    /// `space_id` for base-dir context and the bind CTA. Upstream emits
    /// `WorkspaceNeedsBinding` for unmapped folders.
    Unbound,
    /// Deprecated — no longer produced by the resolver. Retained for serde
    /// compatibility (e.g. `set_workspace_root` responses that may still
    /// carry this value from older gateway versions).
    #[doc(hidden)]
    SpaceDefault,
    /// No FeatureSet resolved at all. Defensive: reached only when there's no
    /// default Space, or — degenerately — the default Space somehow has no
    /// Starter FeatureSet.
    Deny,
}

/// Output of [`FeatureSetResolverService::resolve`].
///
/// `feature_set_ids` is empty when the resolution was a deny. Multiple ids
/// are possible only on the `ClientGrant` path — bindings always resolve to
/// exactly one FS.
#[derive(Debug, Clone)]
pub struct ResolvedFeatureSet {
    pub feature_set_ids: Vec<String>,
    /// Resolved Space id. Used by the routing layer when filtering features.
    pub space_id: Option<Uuid>,
    pub source: ResolutionSource,
}

impl ResolvedFeatureSet {
    /// Stable key for change detection (sorted + comma-joined). Used by
    /// `SessionRootsRegistry::record_resolution` to decide when a session's
    /// effective tools changed and a per-peer `list_changed` is owed.
    pub fn fingerprint(&self) -> Option<String> {
        if self.feature_set_ids.is_empty() {
            return None;
        }
        let mut ids = self.feature_set_ids.clone();
        ids.sort();
        Some(ids.join(","))
    }
}

/// Resolves which FeatureSet(s) apply for a given session.
///
/// Cheap to clone via `Arc`; inject one instance into the gateway's service
/// container and reuse across requests.
pub struct FeatureSetResolverService {
    space_repo: Arc<dyn SpaceRepository>,
    binding_repo: Arc<dyn WorkspaceBindingRepository>,
    session_roots: Arc<SessionRootsRegistry>,
    /// Reads `client_grants` for the rootless Tier-2 fallback. Stored as a
    /// concrete repo (storage owns this type and there's only ever one).
    client_repo: Arc<InboundClientRepository>,
    /// Retained in the constructor signature for API stability; was used by the
    /// now-removed `default_fallback()` helper. Remove once all call sites are updated.
    #[allow(dead_code)]
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    /// Scopes an unmapped reported root to a Space by base directory — an
    /// unmapped folder under a Space's base dir falls back to that Space's
    /// Starter instead of the global default Space.
    space_base_dir_repo: Arc<dyn SpaceBaseDirRepository>,
    /// Grace window for the `PendingRoots` tier — see
    /// [`DEFAULT_PENDING_ROOTS_GRACE`]. Configurable so tests can force the
    /// post-grace path deterministically without sleeping.
    pending_grace: Duration,
    /// This install's registered machine id (`gateway.local_machine_id`).
    /// When set, Tier 1 prefers machine-scoped bindings before global fallbacks.
    local_machine_id: Arc<RwLock<Option<Uuid>>>,
}

impl FeatureSetResolverService {
    pub fn new(
        space_repo: Arc<dyn SpaceRepository>,
        binding_repo: Arc<dyn WorkspaceBindingRepository>,
        session_roots: Arc<SessionRootsRegistry>,
        client_repo: Arc<InboundClientRepository>,
        feature_set_repo: Arc<dyn FeatureSetRepository>,
        space_base_dir_repo: Arc<dyn SpaceBaseDirRepository>,
        local_machine_id: Option<Uuid>,
    ) -> Self {
        Self {
            space_repo,
            binding_repo,
            session_roots,
            client_repo,
            feature_set_repo,
            space_base_dir_repo,
            pending_grace: DEFAULT_PENDING_ROOTS_GRACE,
            local_machine_id: Arc::new(RwLock::new(local_machine_id)),
        }
    }

    /// Hot-reload this install's machine identity without restarting the gateway.
    pub async fn set_local_machine_id(&self, id: Option<Uuid>) {
        *self.local_machine_id.write().await = id;
    }

    /// The machine identity that should govern a *binding write* for this
    /// caller — mirrors Tier 1's read-side priority (request header, then the
    /// OAuth client's registered machine, then this gateway's own local
    /// machine identity) so a bind and the resolve that follows it never
    /// disagree about whose binding it is. `None` means no machine identity
    /// exists anywhere yet (fresh install with nothing registered) — callers
    /// should fall back to the pre-machine, client-scoped binding shape.
    pub async fn effective_machine_id(
        &self,
        client_id: Option<&str>,
        request_machine_id: Option<Uuid>,
    ) -> Result<Option<Uuid>> {
        if let Some(id) = request_machine_id {
            return Ok(Some(id));
        }
        if let Some(cid) = client_id {
            if let Some(client_machine) = self.client_repo.get_machine_id(cid).await? {
                return Ok(Some(client_machine));
            }
        }
        Ok(*self.local_machine_id.read().await)
    }

    /// Tier 1 exact binding lookup: request machine header, then client machine,
    /// then local machine, then global.
    async fn find_binding_for_roots(
        &self,
        roots: &[String],
        client_id: Option<&str>,
        request_machine_id: Option<Uuid>,
    ) -> Result<Option<mcpmux_core::WorkspaceBinding>> {
        for root in roots {
            let registered_machine = if let Some(cid) = client_id {
                self.client_repo.get_machine_id(cid).await?
            } else {
                None
            };

            if let Some(header_machine) = request_machine_id {
                if let Some(binding) = self
                    .binding_repo
                    .find_exact_for_machine(&header_machine, root, client_id)
                    .await?
                {
                    return Ok(Some(binding));
                }
                if let Some(binding) = self.binding_repo.find_exact_global(root).await? {
                    return Ok(Some(binding));
                }
                // Header-only semantics when the header matches the OAuth
                // client's registered machine (tunneled caller on the wrong
                // box stays Unbound) or the caller is anonymous. When the
                // header disagrees with the registered tag it is treated as
                // stale — e.g. cloud-agent config on a native localhost
                // session — and we fall through to client/local/global below.
                if client_id.is_none() || registered_machine.is_some_and(|m| m == header_machine) {
                    continue;
                }
            }

            if let Some(client_machine) = registered_machine {
                if let Some(binding) = self
                    .binding_repo
                    .find_exact_for_machine(&client_machine, root, client_id)
                    .await?
                {
                    return Ok(Some(binding));
                }
            }
            if let Some(local_id) = *self.local_machine_id.read().await {
                if let Some(binding) = self
                    .binding_repo
                    .find_exact_for_machine(&local_id, root, client_id)
                    .await?
                {
                    return Ok(Some(binding));
                }
            }
            if let Some(binding) = self.binding_repo.find_exact_global(root).await? {
                return Ok(Some(binding));
            }
        }
        // ponytail: when this install has no machine identity, preserve the
        // pre-machine exact match (includes client-scoped bindings). Once
        // local_machine_id is set, only machine + global canonical bindings apply.
        if self.local_machine_id.read().await.is_none() {
            return self.binding_repo.find_exact_for_roots(roots).await;
        }
        Ok(None)
    }

    /// Tier 2 id-type binding lookup: request machine header, then client
    /// machine, then local machine, then global.
    async fn find_binding_for_client_id(
        &self,
        client_id: &str,
        request_machine_id: Option<Uuid>,
    ) -> Result<Option<mcpmux_core::WorkspaceBinding>> {
        if let Some(header_machine) = request_machine_id {
            if let Some(binding) = self
                .binding_repo
                .find_by_id_key(client_id, Some(&header_machine))
                .await?
            {
                return Ok(Some(binding));
            }
            if let Some(binding) = self.binding_repo.find_by_id_key(client_id, None).await? {
                return Ok(Some(binding));
            }
            return Ok(None);
        }
        if let Some(client_machine) = self.client_repo.get_machine_id(client_id).await? {
            if let Some(binding) = self
                .binding_repo
                .find_by_id_key(client_id, Some(&client_machine))
                .await?
            {
                return Ok(Some(binding));
            }
        }
        if let Some(local_id) = *self.local_machine_id.read().await {
            if let Some(binding) = self
                .binding_repo
                .find_by_id_key(client_id, Some(&local_id))
                .await?
            {
                return Ok(Some(binding));
            }
        }
        self.binding_repo.find_by_id_key(client_id, None).await
    }

    /// The Space that claims one of `roots` by base directory, or `None`. Each
    /// root's longest-prefix match is taken (via the repo); the first reported
    /// root that lands in a Space wins. Used to scope an unmapped folder to its
    /// Space rather than always falling back to the global default.
    async fn space_for_roots(&self, roots: &[String]) -> Result<Option<Uuid>> {
        for r in roots {
            if let Some(space_id) = self.space_base_dir_repo.find_space_for_root(r).await? {
                return Ok(Some(space_id));
            }
        }
        Ok(None)
    }

    /// The Space a session is scoped to by base directory — its reported root
    /// sits under that Space's base dir — or `None` when it isn't base-dir
    /// scoped (no session, no roots, or no matching base dir). The meta-tools
    /// use this to hard-restrict self-optimization to the matched Space.
    pub async fn scoped_space_for_session(&self, session_id: Option<&str>) -> Result<Option<Uuid>> {
        let Some(sid) = session_id else {
            return Ok(None);
        };
        let Some(roots) = self.session_roots.get(sid) else {
            return Ok(None);
        };
        self.space_for_roots(&roots).await
    }

    /// Override the pending-roots grace window. `Duration::ZERO` makes the
    /// resolver skip the wait entirely and fall back to the Space default on
    /// the first pending resolution — used by tests to exercise the
    /// post-grace path without a real delay.
    pub fn with_pending_grace(mut self, grace: Duration) -> Self {
        self.pending_grace = grace;
        self
    }

    /// Deny by default: no binding matched. Carries `space_id` for base-dir
    /// context and the bind CTA; `feature_set_ids` is empty.
    fn unbound(&self, space_id: Uuid) -> ResolvedFeatureSet {
        ResolvedFeatureSet {
            feature_set_ids: vec![],
            space_id: Some(space_id),
            source: ResolutionSource::Unbound,
        }
    }

    /// Whether a binding is eligible under an optional Space lock.
    fn binding_matches_space_lock(
        binding: &mcpmux_core::WorkspaceBinding,
        space_lock: Option<Uuid>,
    ) -> bool {
        space_lock.is_none_or(|lock| binding.space_id == lock)
    }

    /// Space id used for Unbound / grant lookups when a lock is active.
    fn unbound_space_id(space_lock: Option<Uuid>, fallback: Uuid) -> Uuid {
        space_lock.unwrap_or(fallback)
    }

    /// Borrow the session-roots registry. The notifier uses this to GC
    /// dead sessions out of the registry when reaping the corresponding
    /// peer entries — keeping both stores in sync.
    pub fn session_roots(&self) -> &Arc<SessionRootsRegistry> {
        &self.session_roots
    }

    /// Resolve the effective (Space, FS list, source) tuple for a session.
    ///
    /// `session_id`: the client's `mcp-session-id` header (or `None` when
    /// the caller is stateless — e.g. desktop UI HTTP path).
    /// `client_id`: the OAuth client identity. Used for Tier-2 id-type binding
    /// lookup and Tier-3 `client_grants`; ignored for path-based routing.
    /// `request_machine_id`: optional per-device identity from
    /// `X-Mcpmux-Machine-Id`; highest-priority machine signal for Tier 1.
    pub async fn resolve(
        &self,
        session_id: Option<&str>,
        client_id: Option<&str>,
        request_machine_id: Option<Uuid>,
    ) -> Result<ResolvedFeatureSet> {
        let default_space_id = match self.space_repo.get_default().await? {
            Some(s) => s.id,
            None => {
                warn!("[FeatureSetResolver] no default space — deny");
                return Ok(ResolvedFeatureSet {
                    feature_set_ids: vec![],
                    space_id: None,
                    source: ResolutionSource::Deny,
                });
            }
        };

        // Tier 0 — Space lock narrows all subsequent tiers to one Space.
        // It never grants tools by itself; no in-Space match still → Unbound.
        let space_lock = match client_id {
            Some(cid) => self.client_repo.get_locked_space(cid).await?,
            None => None,
        };
        let deny_space_id = Self::unbound_space_id(space_lock, default_space_id);

        // Tier 1 / 1b / 1c — branches on roots-capable + roots-arrived state.
        if let Some(sid) = session_id {
            let roots = self.session_roots.get(sid);
            // Three distinct states, NOT two:
            //   None      — roots never arrived (still in flight, or never
            //               declared). Eligible for Tier 1c PendingRoots.
            //   Some([])  — roots ARRIVED and the client has no folder open
            //               (Claude Desktop chat, empty VS Code window). This
            //               is a settled "rootless right now" answer — it must
            //               fall through to the Tier-2 grant lookup, NOT hang
            //               in PendingRoots forever. (Bug: conflating this with
            //               None stranded granted clients on meta-tools-only.)
            //   Some([..])— has roots → Tier 1 binding match.
            let roots_arrived = roots.is_some();
            let has_roots = roots.as_ref().is_some_and(|r| !r.is_empty());
            // Three states for capability:
            //   `Some(true)`  — declared roots on initialize
            //   `Some(false)` — explicitly didn't declare; treat as rootless
            //   `None`        — never observed `notifications/initialized`
            //                   for this session yet. Treat as PROBABLY
            //                   capable (most modern MCP clients are) so
            //                   the resolver returns PendingRoots instead
            //                   of falling all the way through to the
            //                   rootless client_grants tier — otherwise a
            //                   tools/list that races on_initialized
            //                   yields "no roots + no grants — deny" and
            //                   the user sees only meta tools until
            //                   reconnect.
            let roots_capable_known = self.session_roots.is_roots_capable(sid);

            // Tier 1: session reported roots — try an EXACT binding match
            // (no ancestor inheritance).
            if has_roots {
                let reported_roots = roots.expect("has_roots implies Some");
                if let Some(binding) = self
                    .find_binding_for_roots(&reported_roots, client_id, request_machine_id)
                    .await?
                {
                    if Self::binding_matches_space_lock(&binding, space_lock) {
                        debug!(
                            workspace_root = %binding.workspace_root,
                            space_id = %binding.space_id,
                            feature_sets = ?binding.feature_set_ids,
                            "[FeatureSetResolver] resolved via WorkspaceBinding",
                        );
                        return Ok(ResolvedFeatureSet {
                            feature_set_ids: binding.feature_set_ids,
                            space_id: Some(binding.space_id),
                            source: ResolutionSource::WorkspaceBinding,
                        });
                    }
                    debug!(
                        binding_space = %binding.space_id,
                        ?space_lock,
                        "[FeatureSetResolver] path binding outside locked Space — ignored",
                    );
                }
                // Tier 1b: had roots, no binding (or binding outside lock).
                if roots_capable_known == Some(false) {
                    // True rootless client declared a root (e.g. via
                    // `mcpmux_set_workspace_root`) but it didn't exact-match any
                    // binding — fall through to Tier 3 grant lookup instead of
                    // hard-denying. The pre-Tier-3 gate treats the declared root
                    // as the identity signal it was waiting for.
                    debug!(
                        session_id = %sid,
                        "[FeatureSetResolver] rootless session declared root but no binding matched — fall through to Tier 3",
                    );
                } else {
                    // Roots-capable or still-probing session: unmapped folder
                    // denies by default. When locked, scope `space_id` to the
                    // locked Space; otherwise longest-prefix base dir.
                    let target_space = if space_lock.is_some() {
                        deny_space_id
                    } else {
                        self.space_for_roots(&reported_roots)
                            .await?
                            .unwrap_or(default_space_id)
                    };
                    debug!(
                        %target_space,
                        scoped_by_base_dir = target_space != default_space_id,
                        "[FeatureSetResolver] roots reported but no binding matched — Unbound",
                    );
                    return Ok(self.unbound(target_space));
                }
            }

            // Tier 1c: client declared `roots` but none have ARRIVED yet
            // (roots == None), OR we haven't observed `initialize` yet so we
            // don't know either way. Returning PendingRoots (empty) means the
            // first response is empty if the on-demand probe loses the race,
            // but the next request retries via the probe + the on_initialized
            // list_roots task fires `list_changed` once roots actually land.
            //
            // Crucially gated on `!roots_arrived`: if roots already arrived
            // empty (`Some([])`), this is a settled answer — skip PendingRoots
            // and fall through to Tier 2 so a granted-but-folderless client
            // still gets its tools.
            if !roots_arrived && !matches!(roots_capable_known, Some(false)) {
                // Grace window: hold at PendingRoots (empty) while the client
                // still might report a folder, so it resolves straight to
                // that folder's binding (or the default-for-unmapped) instead
                // of flashing the Space default and then flipping. Stamps
                // first-seen on the first pending resolve and measures from
                // there.
                let elapsed = self.session_roots.elapsed_since_first_seen(sid);
                if elapsed < self.pending_grace {
                    debug!(
                        session_id = %sid,
                        capability = ?roots_capable_known,
                        elapsed_ms = elapsed.as_millis(),
                        "[FeatureSetResolver] roots-capable (or unknown), roots pending — empty until they arrive",
                    );
                    return Ok(ResolvedFeatureSet {
                        feature_set_ids: vec![],
                        space_id: Some(deny_space_id),
                        source: ResolutionSource::PendingRoots,
                    });
                }
                // Grace lapsed with no root in sight — deny by default. Go
                // STRAIGHT to Unbound (not via Tier-2 grants): a roots-capable
                // session must never pick up another client's grants
                // (per-session isolation invariant).
                debug!(
                    session_id = %sid,
                    capability = ?roots_capable_known,
                    "[FeatureSetResolver] pending-roots grace lapsed, no root reported — Unbound",
                );
                return Ok(self.unbound(deny_space_id));
            }
        }

        // Tier 2 — id-type client mapping (rootless / API-key / OAuth clientId).
        if let Some(cid) = client_id {
            if let Some(binding) = self
                .find_binding_for_client_id(cid, request_machine_id)
                .await?
            {
                if Self::binding_matches_space_lock(&binding, space_lock) {
                    debug!(
                        client_id = %cid,
                        space_id = %binding.space_id,
                        feature_sets = ?binding.feature_set_ids,
                        "[FeatureSetResolver] resolved via id-type WorkspaceBinding",
                    );
                    return Ok(ResolvedFeatureSet {
                        feature_set_ids: binding.feature_set_ids,
                        space_id: Some(binding.space_id),
                        source: ResolutionSource::WorkspaceBinding,
                    });
                }
                debug!(
                    binding_space = %binding.space_id,
                    ?space_lock,
                    "[FeatureSetResolver] id binding outside locked Space — ignored",
                );
            }
        }

        // Tier 3 — rootless-by-design. Either the session declared no
        // `roots` capability, or the caller has no session id at all
        // (the desktop UI's preview HTTP path lands here too). Consult the
        // per-client grant table.
        if let Some(cid) = client_id {
            let grant_space_id = deny_space_id;
            // Propagate storage errors instead of treating them as "no
            // grants": a transient DB failure must surface as a request
            // error, not a silent deny (which would also record a `None`
            // fingerprint and fire a spurious Deny→Grant flip-notification
            // cycle once the error clears).
            let grants = self
                .client_repo
                .get_grants_for_space(cid, &grant_space_id.to_string())
                .await?;
            if !grants.is_empty() {
                // Pre-Tier-3 gate: rootless sessions must declare a workspace
                // root (via `mcpmux_set_workspace_root` or equivalent) before
                // the blanket grant unlocks. Reuses PendingRoots so meta tools
                // stay reachable while the client self-unblocks.
                if let Some(sid) = session_id {
                    if self.session_roots.is_roots_capable(sid) == Some(false) {
                        let has_declared_root = self
                            .session_roots
                            .get(sid)
                            .is_some_and(|roots| !roots.is_empty());
                        if !has_declared_root {
                            debug!(
                                session_id = %sid,
                                client_id = %cid,
                                "[FeatureSetResolver] rootless session has grant but no declared root — PendingRoots",
                            );
                            return Ok(ResolvedFeatureSet {
                                feature_set_ids: vec![],
                                space_id: Some(grant_space_id),
                                source: ResolutionSource::PendingRoots,
                            });
                        }
                    }
                }
                debug!(
                    client_id = %cid,
                    space_id = %grant_space_id,
                    grant_count = grants.len(),
                    "[FeatureSetResolver] resolved via ClientGrant",
                );
                return Ok(ResolvedFeatureSet {
                    feature_set_ids: grants,
                    space_id: Some(grant_space_id),
                    source: ResolutionSource::ClientGrant,
                });
            }
        }

        // Tier 4 — no roots, no id binding, no grants. Deny by default. The mcpmux_* meta
        // tools are appended unconditionally by the request handler regardless,
        // so the LLM can always self-bind / ask the user for a grant from here.
        debug!(
            space_id = %deny_space_id,
            ?client_id,
            ?space_lock,
            "[FeatureSetResolver] no roots + no id binding + no grants — Unbound",
        );
        Ok(self.unbound(deny_space_id))
    }
}
