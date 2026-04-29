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
    /// Used by the request-time re-probe path in the MCP handler to avoid
    /// firing N parallel `list_roots()` calls when a roots-capable session
    /// hits a burst of `tools/list` / `prompts/list` / `resources/list` in
    /// quick succession. The handler calls `claim_probe(sid, throttle)`
    /// before firing; if it returns false, another probe was attempted
    /// recently and this one is skipped.
    last_probe: DashMap<String, Instant>,
}

impl SessionRootsRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            map: DashMap::new(),
            last_resolution: DashMap::new(),
            roots_capable: DashMap::new(),
            last_probe: DashMap::new(),
        })
    }

    /// Try to claim a probe slot for `session_id`. Returns `true` if it's
    /// been at least `throttle` since the last attempt for this session
    /// (or if there's never been one) — and stamps the new attempt
    /// atomically. Returns `false` if a probe was already attempted
    /// within the throttle window.
    ///
    /// The handler calls this before firing `peer.list_roots()` so a
    /// burst of three `tools/list` / `prompts/list` / `resources/list`
    /// calls in 50 ms results in at most one upstream probe.
    pub fn claim_probe(&self, session_id: &str, throttle: Duration) -> bool {
        let now = Instant::now();
        match self.last_probe.entry(session_id.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                if now.duration_since(*e.get()) < throttle {
                    return false;
                }
                e.insert(now);
                true
            }
            dashmap::mapref::entry::Entry::Vacant(e) => {
                e.insert(now);
                true
            }
        }
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
    }

    /// Compare-and-set the session's resolved feature-set id. Returns `true`
    /// when the value actually changed (caller should fire `list_changed`),
    /// `false` when it's the same as before.
    pub fn record_resolution(&self, session_id: &str, fs_id: Option<&str>) -> bool {
        let new_val: Option<String> = fs_id.map(|s| s.to_string());
        match self.last_resolution.get(session_id) {
            Some(prev) if *prev == new_val => false,
            _ => {
                self.last_resolution.insert(session_id.to_string(), new_val);
                true
            }
        }
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
    fn test_remove_clears_resolution_too() {
        let reg = SessionRootsRegistry::default();
        reg.record_resolution("sess-1", Some("fs-a"));
        reg.remove("sess-1");
        // After remove, recording the same value should be considered a
        // change (no prior entry).
        assert!(reg.record_resolution("sess-1", Some("fs-a")));
    }
}
