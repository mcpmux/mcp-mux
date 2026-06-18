//! Session-scoped registry of MCP workspace roots.
//!
//! When a client declares the `roots` capability on `initialize`, the gateway
//! calls `roots/list` via the peer and stashes the result here keyed by the
//! client's `mcp-session-id`. The `FeatureSetResolverService` consults this
//! registry to pick a workspace binding.
//!
//! Roots are stored already-normalized (via
//! [`mcpmux_core::normalize_workspace_root`]) so the resolver doesn't need to
//! re-normalize on every lookup.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mcpmux_core::normalize_workspace_root;

/// Thread-safe registry mapping `mcp-session-id` to the caller's reported
/// workspace roots, plus the most recently resolved feature-set id so the
/// gateway can tell when a session's resolution flips and emit a per-peer
/// `list_changed` to that one session only.
#[derive(Debug, Default)]
pub struct SessionRootsRegistry {
    map: DashMap<String, Vec<String>>,
    /// `session_id -> last-resolved feature-set id` (or `None` for "deny").
    /// We compare each fresh resolution to this snapshot; a different value
    /// means the client's effective tools changed and we must notify it.
    last_resolution: DashMap<String, Option<String>>,
    /// `session_id -> declared MCP `roots` capability` (true when the peer's
    /// `initialize.params.capabilities.roots` was non-empty).
    ///
    /// Stamped during `on_initialized` regardless of whether roots have
    /// arrived yet. The resolver reads this to decide between
    /// `WorkspaceBinding` routing (capable) and the rootless `client_grants`
    /// fallback (not capable). Absence here means we never saw an
    /// `initialize` for that session — treated as "unknown" by the resolver
    /// and routed via grants.
    roots_capable: DashMap<String, bool>,
    /// `session_id -> Instant of the last on-demand `list_roots()` probe`.
    ///
    /// Used by [`Self::should_throttle_probe`] to avoid hammering a
    /// failing client when its previous probe already errored out
    /// recently. Only stamped after a probe attempt completes (success
    /// or failure), not on entry — so concurrent in-flight probes
    /// coordinate via [`Self::probe_lock`] instead of this throttle.
    last_probe: DashMap<String, Instant>,
    /// Per-session mutex guarding `peer.list_roots()` probe attempts.
    ///
    /// Single-flight semantics: when a burst of three list requests
    /// (`tools/list` + `prompts/list` + `resources/list`) hits a
    /// roots-pending session within milliseconds, only one upstream
    /// `list_roots()` call should be in flight. The other two block on
    /// the same lock; once the first attempt populates `map`, the
    /// followers re-check `map.get(sid)` and skip the upstream call
    /// entirely.
    ///
    /// Without this, a boolean "already tried" flag let the followers
    /// see `roots_pending` and return empty *before* the first probe's
    /// result landed — exactly the bug that left Claude Code's
    /// VS Code extension showing only the meta tools.
    probe_lock: DashMap<String, Arc<tokio::sync::Mutex<()>>>,
    /// `session_id -> Instant the resolver first saw this session with no
    /// roots yet`. Stamped lazily by [`Self::elapsed_since_first_seen`] so
    /// the resolver's `PendingRoots` tier can wait a grace window for a root
    /// to arrive before falling back to the Space default — preventing a
    /// roots-capable client from flashing the default FeatureSet and then
    /// flipping to its mapped one the instant its root lands.
    first_seen: DashMap<String, Instant>,
}

impl SessionRootsRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            map: DashMap::new(),
            last_resolution: DashMap::new(),
            roots_capable: DashMap::new(),
            last_probe: DashMap::new(),
            probe_lock: DashMap::new(),
            first_seen: DashMap::new(),
        })
    }

    /// Elapsed time since this session was first observed without roots,
    /// stamping "now" on the first call. The resolver uses this to bound the
    /// `PendingRoots` wait: while the result is below the grace window it
    /// keeps waiting for a root; past it, it settles on the Space default.
    /// Idempotent — the timestamp is only set once per session and cleared by
    /// [`Self::remove`].
    pub fn elapsed_since_first_seen(&self, session_id: &str) -> Duration {
        let first = *self
            .first_seen
            .entry(session_id.to_string())
            .or_insert_with(Instant::now);
        first.elapsed()
    }

    /// Get (or create) the per-session probe lock. The returned Arc is
    /// what the handler awaits to serialize concurrent probes — see
    /// [`Self::probe_lock`] for the rationale.
    pub fn probe_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.probe_lock
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Should we skip an on-demand probe because the previous attempt
    /// completed (success or failure) within the last `throttle`?
    ///
    /// Distinct from `probe_lock`: the lock serializes *concurrent*
    /// probes; this rate-limit prevents *sequential* probes from
    /// hammering a peer whose previous attempt errored.
    pub fn should_throttle_probe(&self, session_id: &str, throttle: Duration) -> bool {
        let Some(last) = self.last_probe.get(session_id) else {
            return false;
        };
        Instant::now().duration_since(*last) < throttle
    }

    /// Stamp the completion of an on-demand probe so the next caller
    /// observes the throttle. Called after the probe returns (regardless
    /// of success or failure) so successive probes back off only when
    /// the previous one actually finished.
    pub fn mark_probe_completed(&self, session_id: &str) {
        self.last_probe
            .insert(session_id.to_string(), Instant::now());
    }

    /// Record whether a session declared the MCP `roots` capability on
    /// `initialize`. Idempotent — called once per session lifecycle.
    pub fn set_roots_capable(&self, session_id: impl Into<String>, capable: bool) {
        self.roots_capable.insert(session_id.into(), capable);
    }

    /// `Some(true)` when the session declared `roots`, `Some(false)` when it
    /// explicitly didn't, `None` when no `initialize` has been observed
    /// (callers without a session id, or pre-init requests).
    pub fn is_roots_capable(&self, session_id: &str) -> Option<bool> {
        self.roots_capable.get(session_id).map(|v| *v)
    }

    /// Store the reported roots for a session. `roots` should already be
    /// absolute paths or `file://` URIs — we normalize them before storing.
    pub fn set<I, S>(&self, session_id: impl Into<String>, roots: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let normalized: Vec<String> = roots
            .into_iter()
            .map(|r| normalize_workspace_root(r.as_ref()))
            .filter(|r| !r.is_empty())
            .collect();
        self.map.insert(session_id.into(), normalized);
    }

    /// Retrieve the (already-normalized) roots for a session, if any.
    pub fn get(&self, session_id: &str) -> Option<Vec<String>> {
        self.map.get(session_id).map(|v| v.clone())
    }

    /// Drop a session's roots — call on client disconnect.
    pub fn remove(&self, session_id: &str) {
        self.map.remove(session_id);
        self.last_resolution.remove(session_id);
        self.roots_capable.remove(session_id);
        self.last_probe.remove(session_id);
        self.probe_lock.remove(session_id);
        self.first_seen.remove(session_id);
    }

    /// Compare-and-set the session's resolved feature-set id. Returns `true`
    /// when the value actually changed (caller should fire `list_changed`),
    /// `false` when it's the same as before.
    pub fn record_resolution(&self, session_id: &str, fs_id: Option<&str>) -> bool {
        let new_val: Option<String> = fs_id.map(|s| s.to_string());
        // IMPORTANT: read the prior value into an owned `bool` and let the
        // `get()` read guard drop at the end of THIS statement. Holding a
        // DashMap `Ref` across the `insert()` below would request a write lock
        // on the same shard while still holding its read lock — a self-deadlock
        // that fires exactly when a session's resolution changes from one
        // Some(..) to a different Some(..) (the common "binding changed" path).
        let unchanged = self
            .last_resolution
            .get(session_id)
            .is_some_and(|prev| *prev == new_val);
        if unchanged {
            return false;
        }
        self.last_resolution.insert(session_id.to_string(), new_val);
        true
    }

    /// Returns every reported root across every active session, de-duplicated
    /// and sorted for stable presentation. Used by the UI's "Detected
    /// workspaces" panel so the user can act on folders that clients have
    /// surfaced but haven't been bound yet.
    pub fn list_all_roots(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .map
            .iter()
            .flat_map(|entry| entry.value().clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Forget every reported root that is **not** currently mapped, so the
    /// Workspaces tab's "Unmapped" list clears and the gateway re-offers the
    /// "map this folder?" prompt the next time those sessions report a root.
    ///
    /// `is_mapped(root)` returns `true` for roots that have a binding — those
    /// are kept. For each tracked session the unmapped roots are dropped; a
    /// session left with no roots is removed from the registry entirely (along
    /// with its last-resolution snapshot and probe throttle) so its next
    /// `tools/list` re-probes the peer and the resolver fires
    /// `WorkspaceNeedsBinding` again. Sessions that still hold a mapped root
    /// keep their entry untouched (they route via their binding and never
    /// prompt). Returns the dropped roots (sorted, deduped) for logging.
    pub fn forget_unmapped_roots<F>(&self, is_mapped: F) -> Vec<String>
    where
        F: Fn(&str) -> bool,
    {
        let mut dropped: Vec<String> = Vec::new();
        let mut emptied: Vec<String> = Vec::new();

        for mut entry in self.map.iter_mut() {
            let mut removed_any = false;
            entry.value_mut().retain(|root| {
                if is_mapped(root) {
                    true
                } else {
                    dropped.push(root.clone());
                    removed_any = true;
                    false
                }
            });
            if removed_any && entry.value().is_empty() {
                emptied.push(entry.key().clone());
            }
        }

        // Remove emptied sessions AFTER the iterator above is released — a
        // `map.remove()` while iterating would request a write lock on a shard
        // the iterator still read-locks (self-deadlock). Dropping the roots
        // entry (rather than leaving an empty Vec) is what makes the next
        // request re-probe: `ensure_roots_probed` early-returns while
        // `get(sid)` is `Some(_)`, even for an empty Vec.
        for sid in emptied {
            self.map.remove(&sid);
            self.last_resolution.remove(&sid);
            self.last_probe.remove(&sid);
            // Reset the grace clock too, so the re-probed session waits afresh
            // for its root to re-arrive instead of immediately defaulting on a
            // stale first-seen timestamp.
            self.first_seen.remove(&sid);
        }

        dropped.sort();
        dropped.dedup();
        dropped
    }

    /// Current number of tracked sessions. Test helper; cheap to call but
    /// not useful in hot paths.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether no sessions are tracked. Paired with [`Self::len`] — clippy
    /// requires this when `len` is present.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_normalizes_and_filters_empty() {
        let reg = SessionRootsRegistry::default();
        reg.set(
            "sess-1",
            [
                #[cfg(windows)]
                "file:///D:/proj/",
                #[cfg(not(windows))]
                "file:///home/user/proj/",
                "",
            ],
        );
        let roots = reg.get("sess-1").unwrap();
        assert_eq!(roots.len(), 1);
        #[cfg(windows)]
        assert_eq!(roots[0], "d:\\proj");
        #[cfg(not(windows))]
        assert_eq!(roots[0], "/home/user/proj");
    }

    #[test]
    fn test_remove() {
        let reg = SessionRootsRegistry::default();
        reg.set("sess-1", ["/a"]);
        assert_eq!(reg.len(), 1);
        reg.remove("sess-1");
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn test_record_resolution_flips_on_change() {
        let reg = SessionRootsRegistry::default();
        // First sighting always counts as a change so the caller emits the
        // initial list_changed for whoever subscribed late.
        assert!(reg.record_resolution("sess-1", Some("fs-fallback")));
        // Same value → no change.
        assert!(!reg.record_resolution("sess-1", Some("fs-fallback")));
        // Different value → change.
        assert!(reg.record_resolution("sess-1", Some("fs-bound")));
        // None ↔ Some both count.
        assert!(reg.record_resolution("sess-1", None));
        assert!(!reg.record_resolution("sess-1", None));
    }

    #[test]
    fn test_forget_unmapped_roots_clears_unmapped_sessions() {
        let reg = SessionRootsRegistry::default();
        #[cfg(windows)]
        let (mapped_in, unmapped_in) = ("file:///D:/mapped/", "file:///D:/unmapped/");
        #[cfg(not(windows))]
        let (mapped_in, unmapped_in) = ("file:///home/u/mapped/", "file:///home/u/unmapped/");

        reg.set("sess-mapped", [mapped_in]);
        reg.set("sess-unmapped", [unmapped_in]);
        reg.record_resolution("sess-unmapped", Some("fs-x"));

        // Treat only the first session's (normalized) root as mapped.
        let mapped_norm = reg.get("sess-mapped").unwrap()[0].clone();
        let dropped = reg.forget_unmapped_roots(|root| root == mapped_norm);

        // Exactly the unmapped root was dropped.
        assert_eq!(dropped.len(), 1);
        assert_ne!(dropped[0], mapped_norm);
        // The mapped session is untouched.
        assert_eq!(reg.get("sess-mapped"), Some(vec![mapped_norm]));
        // The unmapped session is removed entirely so the next request
        // re-probes the peer and the binding prompt fires again.
        assert!(reg.get("sess-unmapped").is_none());
        // ...and its resolution snapshot was cleared (fresh = counts as change).
        assert!(reg.record_resolution("sess-unmapped", Some("fs-x")));
    }

    #[test]
    fn test_forget_unmapped_roots_keeps_mixed_session() {
        let reg = SessionRootsRegistry::default();
        #[cfg(windows)]
        let (mapped_in, unmapped_in) = ("file:///D:/keep/", "file:///D:/drop/");
        #[cfg(not(windows))]
        let (mapped_in, unmapped_in) = ("file:///home/u/keep/", "file:///home/u/drop/");

        reg.set("sess-mixed", [mapped_in, unmapped_in]);
        let roots = reg.get("sess-mixed").unwrap();
        let keep = roots[0].clone();

        let dropped = reg.forget_unmapped_roots(|root| root == keep);

        // The unmapped root went; the session survives with its mapped root.
        assert_eq!(dropped.len(), 1);
        assert_eq!(reg.get("sess-mixed"), Some(vec![keep]));
    }

    #[test]
    fn test_remove_clears_resolution_too() {
        let reg = SessionRootsRegistry::default();
        reg.record_resolution("sess-1", Some("fs-a"));
        reg.remove("sess-1");
        // After remove, recording the same value should be considered a
        // change (no prior entry).
        assert!(reg.record_resolution("sess-1", Some("fs-a")));
    }
}
