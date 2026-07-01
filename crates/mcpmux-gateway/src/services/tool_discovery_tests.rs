use super::{entry_content_hash, ToolIndexEntry};

fn test_entry(qualified_name: &str, description: &str) -> ToolIndexEntry {
    ToolIndexEntry {
        server_id: "server-a".to_string(),
        feature_name: "search_issues".to_string(),
        qualified_name: qualified_name.to_string(),
        description: Some(description.to_string()),
        input_schema: None,
        is_available: true,
        status: None,
        bindable_feature_set_id: None,
    }
}

#[test]
fn alias_change_leaves_content_hash_unchanged() {
    let before = test_entry("jira_search_issues", "Find Jira issues");
    let after = test_entry("atlassian_search_issues", "Find Jira issues");
    assert_eq!(entry_content_hash(&before), entry_content_hash(&after));
}

#[test]
fn description_change_changes_content_hash() {
    let before = test_entry("jira_search_issues", "Find Jira issues");
    let after = test_entry("jira_search_issues", "Find open Jira issues");
    assert_ne!(entry_content_hash(&before), entry_content_hash(&after));
}
