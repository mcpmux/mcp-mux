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
}

impl SessionRootsRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            map: DashMap::new(),
            last_resolution: DashMap::new(),
        })
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
