//! Session-scoped enable/disable overrides for backend MCP servers.
//!
//! When a client session calls `mcpmux_enable_server` / `mcpmux_disable_server`
//! (Phase 3), the gateway mutates this registry. [`FeatureService`] consults it
//! at list materialization time to compose the effective server set:
//! `(binding_servers ∪ enabled) − disabled`.

use std::collections::HashSet;
use std::sync::Arc;

use dashmap::DashMap;

/// One session's override state for UI inspection (Phase 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOverrideEntry {
    pub session_id: String,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

/// Thread-safe registry mapping `mcp-session-id` to per-session server
/// enable/disable sets. Process-lifetime only — reaped with the session.
#[derive(Debug, Default)]
pub struct SessionOverrideRegistry {
    enabled: DashMap<String, HashSet<String>>,
    disabled: DashMap<String, HashSet<String>>,
}

impl SessionOverrideRegistry {
    /// Create a new registry wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            enabled: DashMap::new(),
            disabled: DashMap::new(),
        })
    }

    /// Add `server_id` to the session's enabled set; remove from disabled.
    pub fn enable(&self, session_id: impl Into<String>, server_id: impl Into<String>) {
        let session_id = session_id.into();
        let server_id = server_id.into();
        if let Some(mut disabled) = self.disabled.get_mut(&session_id) {
            disabled.remove(&server_id);
            if disabled.is_empty() {
                drop(disabled);
                self.disabled.remove(&session_id);
            }
        }
        self.enabled
            .entry(session_id)
            .or_default()
            .insert(server_id);
    }

    /// Add `server_id` to the session's disabled set; remove from enabled.
    pub fn disable(&self, session_id: impl Into<String>, server_id: impl Into<String>) {
        let session_id = session_id.into();
        let server_id = server_id.into();
        if let Some(mut enabled) = self.enabled.get_mut(&session_id) {
            enabled.remove(&server_id);
            if enabled.is_empty() {
                drop(enabled);
                self.enabled.remove(&session_id);
            }
        }
        self.disabled
            .entry(session_id)
            .or_default()
            .insert(server_id);
    }

    /// Drop both override sets for a session.
    pub fn clear(&self, session_id: &str) {
        self.enabled.remove(session_id);
        self.disabled.remove(session_id);
    }

    /// Enabled server ids for a session (empty when none).
    pub fn enabled_set(&self, session_id: &str) -> HashSet<String> {
        self.enabled
            .get(session_id)
            .map(|set| set.clone())
            .unwrap_or_default()
    }

    /// Disabled server ids for a session (empty when none).
    pub fn disabled_set(&self, session_id: &str) -> HashSet<String> {
        self.disabled
            .get(session_id)
            .map(|set| set.clone())
            .unwrap_or_default()
    }

    /// Drop a session's overrides — call on client disconnect / reap.
    pub fn remove(&self, session_id: &str) {
        self.enabled.remove(session_id);
        self.disabled.remove(session_id);
    }

    /// Snapshot of every session with non-empty override state.
    pub fn list_all(&self) -> Vec<SessionOverrideEntry> {
        let mut session_ids: HashSet<String> = HashSet::new();
        session_ids.extend(self.enabled.iter().map(|e| e.key().clone()));
        session_ids.extend(self.disabled.iter().map(|e| e.key().clone()));

        let mut out: Vec<SessionOverrideEntry> = session_ids
            .into_iter()
            .filter_map(|session_id| {
                let enabled: Vec<String> = self
                    .enabled
                    .get(&session_id)
                    .map(|set| set.iter().cloned().collect())
                    .unwrap_or_default();
                let disabled: Vec<String> = self
                    .disabled
                    .get(&session_id)
                    .map(|set| set.iter().cloned().collect())
                    .unwrap_or_default();
                if enabled.is_empty() && disabled.is_empty() {
                    return None;
                }
                Some(SessionOverrideEntry {
                    session_id,
                    enabled,
                    disabled,
                })
            })
            .collect();
        out.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        out
    }

    /// Current number of sessions with enabled overrides. Test helper.
    #[cfg(test)]
    pub fn enabled_session_count(&self) -> usize {
        self.enabled.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enable_round_trip() {
        let reg = SessionOverrideRegistry::default();
        reg.enable("sess-1", "github");
        let enabled = reg.enabled_set("sess-1");
        assert_eq!(enabled.len(), 1);
        assert!(enabled.contains("github"));
        assert!(reg.disabled_set("sess-1").is_empty());
    }

    #[test]
    fn test_disable_round_trip() {
        let reg = SessionOverrideRegistry::default();
        reg.disable("sess-1", "firebase");
        let disabled = reg.disabled_set("sess-1");
        assert_eq!(disabled.len(), 1);
        assert!(disabled.contains("firebase"));
        assert!(reg.enabled_set("sess-1").is_empty());
    }

    #[test]
    fn test_enable_clears_disable_and_vice_versa() {
        let reg = SessionOverrideRegistry::default();
        reg.disable("sess-1", "github");
        reg.enable("sess-1", "github");
        assert!(reg.enabled_set("sess-1").contains("github"));
        assert!(!reg.disabled_set("sess-1").contains("github"));

        reg.disable("sess-1", "github");
        assert!(!reg.enabled_set("sess-1").contains("github"));
        assert!(reg.disabled_set("sess-1").contains("github"));
    }

    #[test]
    fn test_clear_and_remove() {
        let reg = SessionOverrideRegistry::default();
        reg.enable("sess-1", "github");
        reg.disable("sess-1", "firebase");
        reg.clear("sess-1");
        assert!(reg.enabled_set("sess-1").is_empty());
        assert!(reg.disabled_set("sess-1").is_empty());

        reg.enable("sess-2", "slack");
        reg.remove("sess-2");
        assert!(reg.enabled_set("sess-2").is_empty());
    }

    #[test]
    fn test_list_all() {
        let reg = SessionOverrideRegistry::default();
        reg.enable("b", "github");
        reg.disable("a", "firebase");
        let all = reg.list_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].session_id, "a");
        assert_eq!(all[1].session_id, "b");
    }
}
