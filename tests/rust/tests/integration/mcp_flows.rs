//! MCP Request Flow tests
//!
//! Tests the complete MCP request handling flow using FeatureService:
//! - tools/list, tools/call with authorization
//! - resources/list, resources/read with authorization  
//! - prompts/list, prompts/get with authorization
//! - Space isolation

use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::{
    FeatureSet, FeatureSetMember, FeatureSetRepository, FeatureType, MemberMode, MemberType,
    ServerFeature, ServerFeatureRepository,
};
use mcpmux_gateway::{FeatureService, PrefixCacheService};
use tests::mocks::{MockFeatureSetRepository, MockServerFeatureRepository};

// Helper functions
fn create_feature(
    space_id: &str,
    server_id: &str,
    name: &str,
    feature_type: FeatureType,
) -> ServerFeature {
    let mut feature = match feature_type {
        FeatureType::Tool => ServerFeature::tool(space_id, server_id, name),
        FeatureType::Prompt => ServerFeature::prompt(space_id, server_id, name),
        FeatureType::Resource => ServerFeature::resource(space_id, server_id, name),
    };
    feature.is_available = true;
    feature
}

// Test context combining all services
struct TestContext {
    space_id: String,
    feature_repo: Arc<MockServerFeatureRepository>,
    feature_set_repo: Arc<MockFeatureSetRepository>,
    prefix_cache: Arc<PrefixCacheService>,
    service: FeatureService,
}

impl TestContext {
    fn new() -> Self {
        let space_id = Uuid::new_v4().to_string();
        let feature_repo = Arc::new(MockServerFeatureRepository::new());
        let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
        let prefix_cache = Arc::new(PrefixCacheService::new());

        let service = FeatureService::new(
            Arc::clone(&feature_repo) as Arc<dyn ServerFeatureRepository>,
            Arc::clone(&feature_set_repo) as Arc<dyn FeatureSetRepository>,
            Arc::clone(&prefix_cache),
        );

        Self {
            space_id,
            feature_repo,
            feature_set_repo,
            prefix_cache,
            service,
        }
    }

    async fn register_server(&self, server_id: &str, alias: Option<&str>) {
        self.prefix_cache
            .assign_prefix_runtime(&self.space_id, server_id, alias)
            .await;
    }

    async fn add_feature(&self, server_id: &str, name: &str, feature_type: FeatureType) -> Uuid {
        let feature = create_feature(&self.space_id, server_id, name, feature_type);
        let id = feature.id;
        self.feature_repo.upsert(&feature).await.unwrap();
        id
    }

    async fn add_feature_set(&self, fs: FeatureSet) -> String {
        let id = fs.id.clone();
        self.feature_set_repo.create(&fs).await.unwrap();
        id
    }
}

// ============================================================================
// TOOLS FLOW TESTS
// ============================================================================

#[tokio::test]
async fn test_list_tools_with_all_grant() {
    let ctx = TestContext::new();

    // Setup: Register server and add tools
    ctx.register_server("files", Some("fs")).await;
    ctx.add_feature("files", "read_file", FeatureType::Tool)
        .await;
    ctx.add_feature("files", "write_file", FeatureType::Tool)
        .await;

    // Create "All" grant
    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    // Simulate tools/list with grant
    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 2);

    // Tools should have qualified names
    let tool_names: Vec<String> = tools.iter().map(|t| t.qualified_name()).collect();
    assert!(tool_names.iter().any(|n| n == "fs_read_file"));
    assert!(tool_names.iter().any(|n| n == "fs_write_file"));
}

#[tokio::test]
async fn test_list_tools_with_restricted_grant() {
    let ctx = TestContext::new();

    ctx.register_server("files", Some("fs")).await;
    let tool_a_id = ctx
        .add_feature("files", "safe_read", FeatureType::Tool)
        .await;
    ctx.add_feature("files", "dangerous_delete", FeatureType::Tool)
        .await;

    // Create custom grant with only safe_read
    let mut custom_fs = FeatureSet::new_custom("Safe Tools", &ctx.space_id);
    custom_fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: custom_fs.id.clone(),
        member_id: tool_a_id.to_string(),
        member_type: MemberType::Feature,
        mode: MemberMode::Include,
    });
    let custom_fs_id = ctx.add_feature_set(custom_fs).await;

    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[custom_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].feature_name, "safe_read");
}

#[tokio::test]
async fn test_call_tool_routing() {
    let ctx = TestContext::new();

    ctx.register_server("math-server", Some("math")).await;
    ctx.add_feature("math-server", "calculate", FeatureType::Tool)
        .await;

    // Verify routing resolves correctly
    let result = ctx
        .service
        .find_server_for_qualified_tool(&ctx.space_id, "math_calculate")
        .await
        .unwrap();

    assert!(result.is_some());
    let (server_id, tool_name) = result.unwrap();
    assert_eq!(server_id, "math-server");
    assert_eq!(tool_name, "calculate");
}

#[tokio::test]
async fn test_call_tool_unauthorized() {
    let ctx = TestContext::new();

    ctx.register_server("admin-server", Some("admin")).await;
    ctx.add_feature("admin-server", "delete_all", FeatureType::Tool)
        .await;

    // No grants - empty feature set
    let empty_fs = FeatureSet::new_custom("Empty", &ctx.space_id);
    let empty_fs_id = ctx.add_feature_set(empty_fs).await;

    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[empty_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 0, "No tools should be authorized");
}

// ============================================================================
// RESOURCES FLOW TESTS
// ============================================================================

#[tokio::test]
async fn test_list_resources_with_grant() {
    let ctx = TestContext::new();

    ctx.register_server("files", Some("fs")).await;
    ctx.add_feature("files", "file:///docs/readme.md", FeatureType::Resource)
        .await;
    ctx.add_feature("files", "file:///docs/config.json", FeatureType::Resource)
        .await;

    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    let resources = ctx
        .service
        .get_resources_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(resources.len(), 2);
}

#[tokio::test]
async fn test_read_resource_routing() {
    let ctx = TestContext::new();

    ctx.register_server("docs-server", Some("docs")).await;
    ctx.add_feature("docs-server", "docs://api-reference", FeatureType::Resource)
        .await;

    // Resolve resource URI to server
    let result = ctx
        .service
        .find_server_for_resource(&ctx.space_id, "docs://api-reference")
        .await
        .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap(), "docs-server");
}

#[tokio::test]
async fn test_resource_custom_uri_scheme() {
    let ctx = TestContext::new();

    ctx.register_server("domains", Some("dom")).await;
    ctx.add_feature(
        "domains",
        "instant-domains://tld-categories",
        FeatureType::Resource,
    )
    .await;

    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    let resources = ctx
        .service
        .get_resources_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(resources.len(), 1);
    assert_eq!(
        resources[0].feature_name,
        "instant-domains://tld-categories"
    );
}

// ============================================================================
// PROMPTS FLOW TESTS
// ============================================================================

#[tokio::test]
async fn test_list_prompts_with_grant() {
    let ctx = TestContext::new();

    ctx.register_server("prompts-server", Some("p")).await;
    ctx.add_feature("prompts-server", "summarize", FeatureType::Prompt)
        .await;
    ctx.add_feature("prompts-server", "explain_code", FeatureType::Prompt)
        .await;

    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    let prompts = ctx
        .service
        .get_prompts_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(prompts.len(), 2);

    let prompt_names: Vec<String> = prompts.iter().map(|p| p.qualified_name()).collect();
    assert!(prompt_names.iter().any(|n| n == "p_summarize"));
    assert!(prompt_names.iter().any(|n| n == "p_explain_code"));
}

#[tokio::test]
async fn test_get_prompt_routing() {
    let ctx = TestContext::new();

    ctx.register_server("prompts", Some("pr")).await;
    ctx.add_feature("prompts", "code_review", FeatureType::Prompt)
        .await;

    let result = ctx
        .service
        .find_server_for_qualified_prompt(&ctx.space_id, "pr_code_review")
        .await
        .unwrap();

    assert!(result.is_some());
    let (server_id, prompt_name) = result.unwrap();
    assert_eq!(server_id, "prompts");
    assert_eq!(prompt_name, "code_review");
}

// ============================================================================
// MIXED FEATURE TYPES
// ============================================================================

#[tokio::test]
async fn test_server_provides_multiple_feature_types() {
    let ctx = TestContext::new();

    ctx.register_server("full-server", Some("full")).await;
    ctx.add_feature("full-server", "my_tool", FeatureType::Tool)
        .await;
    ctx.add_feature("full-server", "my_prompt", FeatureType::Prompt)
        .await;
    ctx.add_feature("full-server", "my://resource", FeatureType::Resource)
        .await;

    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    // List all
    let all_features = ctx
        .service
        .resolve_feature_sets(&ctx.space_id, &[all_fs_id.clone()])
        .await
        .unwrap();

    assert_eq!(all_features.len(), 3);

    // Filter by type
    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[all_fs_id.clone()])
        .await
        .unwrap();
    let prompts = ctx
        .service
        .get_prompts_for_grants(&ctx.space_id, &[all_fs_id.clone()])
        .await
        .unwrap();
    let resources = ctx
        .service
        .get_resources_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(prompts.len(), 1);
    assert_eq!(resources.len(), 1);
}

// ============================================================================
// MULTI-SERVER AGGREGATION
// ============================================================================

#[tokio::test]
async fn test_aggregate_tools_from_multiple_servers() {
    let ctx = TestContext::new();

    // Setup multiple servers
    ctx.register_server("server-a", Some("a")).await;
    ctx.register_server("server-b", Some("b")).await;
    ctx.register_server("server-c", Some("c")).await;

    ctx.add_feature("server-a", "tool_a", FeatureType::Tool)
        .await;
    ctx.add_feature("server-b", "tool_b", FeatureType::Tool)
        .await;
    ctx.add_feature("server-c", "tool_c", FeatureType::Tool)
        .await;

    // Grant access to all
    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 3);

    let qualified_names: Vec<String> = tools.iter().map(|t| t.qualified_name()).collect();
    assert!(qualified_names.contains(&"a_tool_a".to_string()));
    assert!(qualified_names.contains(&"b_tool_b".to_string()));
    assert!(qualified_names.contains(&"c_tool_c".to_string()));
}

#[tokio::test]
async fn test_partial_server_grant() {
    let ctx = TestContext::new();

    ctx.register_server("server-a", Some("a")).await;
    ctx.register_server("server-b", Some("b")).await;

    ctx.add_feature("server-a", "tool_a", FeatureType::Tool)
        .await;
    ctx.add_feature("server-b", "tool_b", FeatureType::Tool)
        .await;

    // Create ServerAll grant for server-a only
    let server_all_a = FeatureSet::new_server_all(&ctx.space_id, "server-a", "Server A");
    let server_all_a_id = ctx.add_feature_set(server_all_a).await;

    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[server_all_a_id])
        .await
        .unwrap();

    // Should only get tools from server-a
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server_id, "server-a");
}

// ============================================================================
// SPACE ISOLATION
// ============================================================================

#[tokio::test]
async fn test_features_dont_leak_between_spaces() {
    let space_work = Uuid::new_v4().to_string();
    let space_personal = Uuid::new_v4().to_string();

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Add features to different spaces
    let mut work_tool = ServerFeature::tool(&space_work, "work-server", "work_tool");
    work_tool.is_available = true;
    let mut personal_tool =
        ServerFeature::tool(&space_personal, "personal-server", "personal_tool");
    personal_tool.is_available = true;

    feature_repo.upsert(&work_tool).await.unwrap();
    feature_repo.upsert(&personal_tool).await.unwrap();

    // Create All grant for work space
    let work_all = FeatureSet::new_all(&space_work);
    let work_all_id = work_all.id.clone();
    feature_set_repo.create(&work_all).await.unwrap();

    let service = FeatureService::new(
        feature_repo as Arc<dyn ServerFeatureRepository>,
        feature_set_repo as Arc<dyn FeatureSetRepository>,
        prefix_cache,
    );

    // Query work space
    let work_tools = service
        .get_tools_for_grants(&space_work, &[work_all_id])
        .await
        .unwrap();

    // Should only get work space features
    assert_eq!(work_tools.len(), 1);
    assert_eq!(work_tools[0].feature_name, "work_tool");
    assert_eq!(work_tools[0].space_id, space_work);
}

#[tokio::test]
async fn test_routing_is_space_scoped() {
    let space_a = Uuid::new_v4().to_string();
    let space_b = Uuid::new_v4().to_string();

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Register same alias in different spaces pointing to different servers
    prefix_cache
        .assign_prefix_runtime(&space_a, "server-in-a", Some("common"))
        .await;
    prefix_cache
        .assign_prefix_runtime(&space_b, "server-in-b", Some("common"))
        .await;

    // Add features
    let mut feature_a = ServerFeature::tool(&space_a, "server-in-a", "my_tool");
    feature_a.is_available = true;
    let mut feature_b = ServerFeature::tool(&space_b, "server-in-b", "my_tool");
    feature_b.is_available = true;
    feature_repo.upsert(&feature_a).await.unwrap();
    feature_repo.upsert(&feature_b).await.unwrap();

    let service = FeatureService::new(
        feature_repo as Arc<dyn ServerFeatureRepository>,
        feature_set_repo as Arc<dyn FeatureSetRepository>,
        prefix_cache,
    );

    // Resolve same qualified name in different spaces
    let result_a = service
        .find_server_for_qualified_tool(&space_a, "common_my_tool")
        .await
        .unwrap();
    let result_b = service
        .find_server_for_qualified_tool(&space_b, "common_my_tool")
        .await
        .unwrap();

    assert!(result_a.is_some());
    assert!(result_b.is_some());

    let (server_a, _) = result_a.unwrap();
    let (server_b, _) = result_b.unwrap();

    assert_eq!(server_a, "server-in-a");
    assert_eq!(server_b, "server-in-b");
}

// ============================================================================
// AVAILABILITY FILTERING
// ============================================================================

#[tokio::test]
async fn test_unavailable_features_filtered_out() {
    let ctx = TestContext::new();

    ctx.register_server("server", Some("s")).await;

    // Add available tool
    ctx.add_feature("server", "available_tool", FeatureType::Tool)
        .await;

    // Add unavailable tool
    let mut unavailable = ServerFeature::tool(&ctx.space_id, "server", "unavailable_tool");
    unavailable.is_available = false;
    ctx.feature_repo.upsert(&unavailable).await.unwrap();

    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    let tools = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].feature_name, "available_tool");
}

#[tokio::test]
async fn test_server_disconnect_marks_features_unavailable() {
    let ctx = TestContext::new();

    ctx.register_server("server", Some("s")).await;
    ctx.add_feature("server", "tool_1", FeatureType::Tool).await;
    ctx.add_feature("server", "tool_2", FeatureType::Tool).await;

    let all_fs = FeatureSet::new_all(&ctx.space_id);
    let all_fs_id = ctx.add_feature_set(all_fs).await;

    // Initially available
    let tools_before = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[all_fs_id.clone()])
        .await
        .unwrap();
    assert_eq!(tools_before.len(), 2);

    // Simulate server disconnect
    ctx.feature_repo
        .mark_unavailable(&ctx.space_id, "server")
        .await
        .unwrap();

    // After disconnect
    let tools_after = ctx
        .service
        .get_tools_for_grants(&ctx.space_id, &[all_fs_id])
        .await
        .unwrap();
    assert_eq!(tools_after.len(), 0);
}
