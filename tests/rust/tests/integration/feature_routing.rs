//! Feature Routing tests
//!
//! Tests qualified name resolution and prefix handling for tools/prompts/resources
//! using the FeatureService facade.

use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::{FeatureSetRepository, ServerFeature, ServerFeatureRepository};
use mcpmux_gateway::{FeatureService, PrefixCacheService};
use tests::mocks::{MockFeatureSetRepository, MockServerFeatureRepository};

// Helper to create test features
fn create_tool(space_id: &str, server_id: &str, name: &str) -> ServerFeature {
    let mut feature = ServerFeature::tool(space_id, server_id, name);
    feature.is_available = true;
    feature
}

fn create_prompt(space_id: &str, server_id: &str, name: &str) -> ServerFeature {
    let mut feature = ServerFeature::prompt(space_id, server_id, name);
    feature.is_available = true;
    feature
}

fn create_resource(space_id: &str, server_id: &str, uri: &str) -> ServerFeature {
    let mut feature = ServerFeature::resource(space_id, server_id, uri);
    feature.is_available = true;
    feature
}

fn create_feature_service(
    feature_repo: Arc<MockServerFeatureRepository>,
    feature_set_repo: Arc<MockFeatureSetRepository>,
    prefix_cache: Arc<PrefixCacheService>,
) -> FeatureService {
    FeatureService::new(
        feature_repo as Arc<dyn ServerFeatureRepository>,
        feature_set_repo as Arc<dyn FeatureSetRepository>,
        prefix_cache,
    )
}

// ============================================================================
// QUALIFIED TOOL NAME RESOLUTION
// ============================================================================

#[tokio::test]
async fn test_find_server_for_qualified_tool_with_alias() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "filesystem-server";
    let alias = "fs";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Register server with alias
    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;

    // Create tool
    feature_repo
        .upsert(&create_tool(&space_id, server_id, "read_file"))
        .await
        .unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    // Resolve using alias_toolname format
    let result = service
        .find_server_for_qualified_tool(&space_id, "fs_read_file")
        .await
        .unwrap();

    assert!(result.is_some());
    let (resolved_server, resolved_name) = result.unwrap();
    assert_eq!(resolved_server, server_id);
    assert_eq!(resolved_name, "read_file");
}

#[tokio::test]
async fn test_find_server_for_qualified_tool_with_server_id() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "my-server";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Register server without alias (uses server_id as prefix)
    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, None)
        .await;

    // Create tool
    feature_repo
        .upsert(&create_tool(&space_id, server_id, "do_something"))
        .await
        .unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    // Resolve using server_id_toolname format
    let result = service
        .find_server_for_qualified_tool(&space_id, "my-server_do_something")
        .await
        .unwrap();

    assert!(result.is_some());
    let (resolved_server, resolved_name) = result.unwrap();
    assert_eq!(resolved_server, server_id);
    assert_eq!(resolved_name, "do_something");
}

#[tokio::test]
async fn test_find_server_for_qualified_prompt() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "prompts-server";
    let alias = "prompts";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;
    feature_repo
        .upsert(&create_prompt(&space_id, server_id, "summarize"))
        .await
        .unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .find_server_for_qualified_prompt(&space_id, "prompts_summarize")
        .await
        .unwrap();

    assert!(result.is_some());
    let (resolved_server, resolved_name) = result.unwrap();
    assert_eq!(resolved_server, server_id);
    assert_eq!(resolved_name, "summarize");
}

#[tokio::test]
async fn test_unavailable_tool_not_resolved() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some("s"))
        .await;

    // Create unavailable tool
    let mut tool = create_tool(&space_id, server_id, "unavailable_tool");
    tool.is_available = false;
    feature_repo.upsert(&tool).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .find_server_for_qualified_tool(&space_id, "s_unavailable_tool")
        .await
        .unwrap();

    assert!(result.is_none(), "Unavailable tools should not be resolved");
}

// ============================================================================
// RESOURCE URI RESOLUTION
// ============================================================================

#[tokio::test]
async fn test_find_server_for_resource_uri() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "files-server";
    let uri = "file:///home/user/document.txt";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    feature_repo
        .upsert(&create_resource(&space_id, server_id, uri))
        .await
        .unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    // Resources are resolved by URI directly
    let result = service
        .find_server_for_resource(&space_id, uri)
        .await
        .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap(), server_id);
}

#[tokio::test]
async fn test_find_server_for_custom_uri_scheme() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "domains-server";
    let uri = "instant-domains://tld-categories";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    feature_repo
        .upsert(&create_resource(&space_id, server_id, uri))
        .await
        .unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .find_server_for_resource(&space_id, uri)
        .await
        .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap(), server_id);
}

#[tokio::test]
async fn test_resource_not_found() {
    let space_id = Uuid::new_v4().to_string();

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .find_server_for_resource(&space_id, "nonexistent://resource")
        .await
        .unwrap();

    assert!(result.is_none());
}

// ============================================================================
// PARSE QUALIFIED NAME HELPERS
// ============================================================================

#[tokio::test]
async fn test_parse_qualified_tool_name() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server";
    let alias = "srv";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .parse_qualified_tool_name(&space_id, "srv_my_tool")
        .await;

    assert!(result.is_ok());
    let (resolved_server, resolved_name) = result.unwrap();
    assert_eq!(resolved_server, server_id);
    assert_eq!(resolved_name, "my_tool");
}

#[tokio::test]
async fn test_parse_qualified_tool_name_with_underscores() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server";
    let alias = "srv";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    // Tool name contains underscores
    let result = service
        .parse_qualified_tool_name(&space_id, "srv_my_complex_tool_name")
        .await;

    assert!(result.is_ok());
    let (resolved_server, resolved_name) = result.unwrap();
    assert_eq!(resolved_server, server_id);
    assert_eq!(resolved_name, "my_complex_tool_name");
}

#[tokio::test]
async fn test_parse_tool_name_with_unknown_prefix_uses_prefix_as_server() {
    // When a prefix is not registered, it falls back to using the prefix itself as server_id
    let space_id = Uuid::new_v4().to_string();

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .parse_qualified_tool_name(&space_id, "unknown_tool_name")
        .await;

    // Falls back to using "unknown" as server_id (not an error)
    assert!(result.is_ok());
    let (server_id, tool_name) = result.unwrap();
    assert_eq!(server_id, "unknown");
    assert_eq!(tool_name, "tool_name");
}

#[tokio::test]
async fn test_parse_qualified_prompt_name() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "prompts";
    let alias = "p";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let result = service
        .parse_qualified_prompt_name(&space_id, "p_summarize_document")
        .await;

    assert!(result.is_ok());
    let (resolved_server, resolved_name) = result.unwrap();
    assert_eq!(resolved_server, server_id);
    assert_eq!(resolved_name, "summarize_document");
}

// ============================================================================
// PREFIX CACHE TESTS
// ============================================================================

#[tokio::test]
async fn test_prefix_cache_alias_priority() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "long-server-name";
    let alias = "short";

    let prefix_cache = Arc::new(PrefixCacheService::new());
    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;

    // Both alias and server_id should resolve to server
    let alias_result = prefix_cache
        .resolve_qualified_name(&space_id, "short_tool")
        .await;
    let server_result = prefix_cache
        .resolve_qualified_name(&space_id, "long-server-name_tool")
        .await;

    assert!(alias_result.is_some());
    assert!(server_result.is_some());

    let (alias_server, alias_name) = alias_result.unwrap();
    let (id_server, id_name) = server_result.unwrap();

    assert_eq!(alias_server, server_id);
    assert_eq!(id_server, server_id);
    assert_eq!(alias_name, "tool");
    assert_eq!(id_name, "tool");
}

#[tokio::test]
async fn test_prefix_cache_multiple_servers() {
    let space_id = Uuid::new_v4().to_string();

    let prefix_cache = Arc::new(PrefixCacheService::new());
    prefix_cache
        .assign_prefix_runtime(&space_id, "server-a", Some("a"))
        .await;
    prefix_cache
        .assign_prefix_runtime(&space_id, "server-b", Some("b"))
        .await;

    let result_a = prefix_cache
        .resolve_qualified_name(&space_id, "a_tool")
        .await;
    let result_b = prefix_cache
        .resolve_qualified_name(&space_id, "b_tool")
        .await;

    assert!(result_a.is_some());
    assert!(result_b.is_some());

    let (server_a, _) = result_a.unwrap();
    let (server_b, _) = result_b.unwrap();

    assert_eq!(server_a, "server-a");
    assert_eq!(server_b, "server-b");
}

#[tokio::test]
async fn test_prefix_cache_space_isolation() {
    let space_a = Uuid::new_v4().to_string();
    let space_b = Uuid::new_v4().to_string();

    let prefix_cache = Arc::new(PrefixCacheService::new());
    prefix_cache
        .assign_prefix_runtime(&space_a, "actual-server", Some("s"))
        .await;
    // Don't register in space_b

    let result_a = prefix_cache
        .resolve_qualified_name(&space_a, "s_tool")
        .await;
    let result_b = prefix_cache
        .resolve_qualified_name(&space_b, "s_tool")
        .await;

    // Both parse the qualified name, but resolve server differently
    assert!(result_a.is_some(), "Should parse in space_a");
    assert!(
        result_b.is_some(),
        "Should also parse in space_b (with fallback)"
    );

    // Space A: "s" resolves to "actual-server" (registered)
    let (server_a, _) = result_a.unwrap();
    assert_eq!(
        server_a, "actual-server",
        "Should resolve to registered server"
    );

    // Space B: "s" falls back to "s" itself (not registered)
    let (server_b, _) = result_b.unwrap();
    assert_eq!(server_b, "s", "Should fall back to prefix itself");
}

#[tokio::test]
async fn test_get_prefix_for_server_returns_alias() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "my-server";
    let alias = "myalias";

    let prefix_cache = Arc::new(PrefixCacheService::new());
    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some(alias))
        .await;

    let prefix = prefix_cache
        .get_prefix_for_server(&space_id, server_id)
        .await;
    assert_eq!(prefix, alias);
}

#[tokio::test]
async fn test_get_prefix_for_server_fallback() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "my-server";

    let prefix_cache = Arc::new(PrefixCacheService::new());
    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, None)
        .await;

    let prefix = prefix_cache
        .get_prefix_for_server(&space_id, server_id)
        .await;
    assert_eq!(prefix, server_id);
}
