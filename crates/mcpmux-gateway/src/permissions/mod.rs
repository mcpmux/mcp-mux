//! Permission filtering for the gateway
//!
//! Applies FeatureSet rules to filter tools, prompts, and resources.

use std::collections::HashMap;
use tracing::{debug, warn};
use uuid::Uuid;

/// A simplified FeatureSet for filtering (doesn't need full mcpmux-core dependency)
#[derive(Debug, Clone)]
pub struct PermissionSet {
    /// ID of the feature set
    pub id: Uuid,
    /// Tool include patterns
    pub tools_include: Vec<String>,
    /// Tool exclude patterns
    pub tools_exclude: Vec<String>,
    /// Prompt include patterns
    pub prompts_include: Vec<String>,
    /// Prompt exclude patterns
    pub prompts_exclude: Vec<String>,
    /// Resource include patterns
    pub resources_include: Vec<String>,
    /// Resource exclude patterns
    pub resources_exclude: Vec<String>,
}

impl PermissionSet {
    /// Check if a tool name is allowed
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        // First check excludes
        for pattern in &self.tools_exclude {
            if matches_glob(pattern, tool_name) {
                return false;
            }
        }
        // Then check includes
        for pattern in &self.tools_include {
            if matches_glob(pattern, tool_name) {
                return true;
            }
        }
        false
    }

    /// Check if a prompt name is allowed
    pub fn allows_prompt(&self, prompt_name: &str) -> bool {
        for pattern in &self.prompts_exclude {
            if matches_glob(pattern, prompt_name) {
                return false;
            }
        }
        for pattern in &self.prompts_include {
            if matches_glob(pattern, prompt_name) {
                return true;
            }
        }
        false
    }

    /// Check if a resource URI is allowed
    pub fn allows_resource(&self, uri: &str) -> bool {
        for pattern in &self.resources_exclude {
            if matches_glob(pattern, uri) {
                return false;
            }
        }
        for pattern in &self.resources_include {
            if matches_glob(pattern, uri) {
                return true;
            }
        }
        false
    }
}

/// Permission filter that applies FeatureSet rules
pub struct PermissionFilter {
    /// Client ID -> granted PermissionSets for current space
    client_permissions: HashMap<Uuid, Vec<PermissionSet>>,
}

impl PermissionFilter {
    /// Create a new permission filter
    pub fn new() -> Self {
        Self {
            client_permissions: HashMap::new(),
        }
    }

    /// Set permissions for a client
    pub fn set_client_permissions(&mut self, client_id: Uuid, permissions: Vec<PermissionSet>) {
        debug!("Setting {} permission sets for client {}", permissions.len(), client_id);
        self.client_permissions.insert(client_id, permissions);
    }

    /// Clear permissions for a client
    pub fn clear_client_permissions(&mut self, client_id: &Uuid) {
        self.client_permissions.remove(client_id);
    }

    /// Check if a client can access a tool
    pub fn can_access_tool(&self, client_id: &Uuid, tool_name: &str) -> bool {
        let Some(permissions) = self.client_permissions.get(client_id) else {
            // No permissions set = deny all
            warn!("No permissions found for client {}", client_id);
            return false;
        };

        // Any matching permission set grants access
        for perm in permissions {
            if perm.allows_tool(tool_name) {
                debug!("Tool {} allowed for client {} by permission set {}", tool_name, client_id, perm.id);
                return true;
            }
        }

        debug!("Tool {} denied for client {}", tool_name, client_id);
        false
    }

    /// Check if a client can access a prompt
    pub fn can_access_prompt(&self, client_id: &Uuid, prompt_name: &str) -> bool {
        let Some(permissions) = self.client_permissions.get(client_id) else {
            return false;
        };

        for perm in permissions {
            if perm.allows_prompt(prompt_name) {
                return true;
            }
        }
        false
    }

    /// Check if a client can access a resource
    pub fn can_access_resource(&self, client_id: &Uuid, uri: &str) -> bool {
        let Some(permissions) = self.client_permissions.get(client_id) else {
            return false;
        };

        for perm in permissions {
            if perm.allows_resource(uri) {
                return true;
            }
        }
        false
    }

    /// Filter a list of tools based on client permissions
    pub fn filter_tools<T: HasName>(&self, client_id: &Uuid, tools: Vec<T>) -> Vec<T> {
        tools.into_iter()
            .filter(|t| self.can_access_tool(client_id, t.name()))
            .collect()
    }

    /// Filter a list of prompts based on client permissions
    pub fn filter_prompts<T: HasName>(&self, client_id: &Uuid, prompts: Vec<T>) -> Vec<T> {
        prompts.into_iter()
            .filter(|p| self.can_access_prompt(client_id, p.name()))
            .collect()
    }

    /// Filter a list of resources based on client permissions
    pub fn filter_resources<T: HasUri>(&self, client_id: &Uuid, resources: Vec<T>) -> Vec<T> {
        resources.into_iter()
            .filter(|r| self.can_access_resource(client_id, r.uri()))
            .collect()
    }
}

impl Default for PermissionFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for items that have a name
pub trait HasName {
    fn name(&self) -> &str;
}

/// Trait for items that have a URI
pub trait HasUri {
    fn uri(&self) -> &str;
}

/// Simple glob pattern matching
fn matches_glob(pattern: &str, text: &str) -> bool {
    // Handle simple cases
    if pattern == "*" {
        return true;
    }
    if pattern == text {
        return true;
    }

    // Simple wildcard matching
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return false;
    }

    let mut remaining = text;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if i == 0 {
            // First part must be at the start
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            // Last part must be at the end
            if !remaining.ends_with(part) {
                return false;
            }
            remaining = &remaining[..remaining.len() - part.len()];
        } else {
            // Middle parts can be anywhere
            if let Some(pos) = remaining.find(part) {
                remaining = &remaining[pos + part.len()..];
            } else {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matching() {
        assert!(matches_glob("*", "anything"));
        assert!(matches_glob("github.*", "github.create_issue"));
        assert!(matches_glob("*.delete_*", "github.delete_branch"));
        assert!(matches_glob("*_list", "repos_list"));
        assert!(!matches_glob("github.*", "atlassian.get_page"));
    }

    #[test]
    fn test_permission_set() {
        let perm = PermissionSet {
            id: Uuid::new_v4(),
            tools_include: vec!["github.*".to_string(), "*.list_*".to_string()],
            tools_exclude: vec!["*.delete_*".to_string()],
            prompts_include: vec!["*".to_string()],
            prompts_exclude: vec![],
            resources_include: vec![],
            resources_exclude: vec![],
        };

        assert!(perm.allows_tool("github.create_issue"));
        assert!(perm.allows_tool("atlassian.list_pages"));
        assert!(!perm.allows_tool("github.delete_branch")); // excluded
        assert!(!perm.allows_tool("atlassian.update_page")); // not included
    }

    #[test]
    fn test_permission_filter() {
        let mut filter = PermissionFilter::new();
        let client_id = Uuid::new_v4();

        let perm = PermissionSet {
            id: Uuid::new_v4(),
            tools_include: vec!["github.*".to_string()],
            tools_exclude: vec![],
            prompts_include: vec![],
            prompts_exclude: vec![],
            resources_include: vec![],
            resources_exclude: vec![],
        };

        filter.set_client_permissions(client_id, vec![perm]);

        assert!(filter.can_access_tool(&client_id, "github.create_issue"));
        assert!(!filter.can_access_tool(&client_id, "atlassian.get_page"));
    }
}
