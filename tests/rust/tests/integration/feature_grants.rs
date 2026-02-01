//! Feature Grant Resolution tests
//!
//! Tests the complete flow: Space → FeatureSet → Features using FeatureService facade
//! Covers all feature set types: All, Default, ServerAll, Custom

use std::sync::Arc;
use uuid::Uuid;

use mcpmux_core::{
    FeatureSet, FeatureSetMember, FeatureSetRepository, FeatureType, MemberMode, MemberType,
    ServerFeature, ServerFeatureRepository,
};
use mcpmux_gateway::{FeatureService, PrefixCacheService};
use tests::mocks::{MockFeatureSetRepository, MockServerFeatureRepository};

// Helper to create test features
fn create_test_feature(
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
// FEATURE SET TYPE: ALL
// ============================================================================

#[tokio::test]
async fn test_all_featureset_grants_all_features() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "tool_b",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "prompt_a",
            FeatureType::Prompt,
        ))
        .await
        .unwrap();

    // Create "All" feature set
    let all_fs = FeatureSet::new_all(&space_id);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(resolved.len(), 3, "All 3 features should be resolved");
    assert!(resolved.iter().any(|f| f.feature_name == "tool_a"));
    assert!(resolved.iter().any(|f| f.feature_name == "tool_b"));
    assert!(resolved.iter().any(|f| f.feature_name == "prompt_a"));
}

#[tokio::test]
async fn test_all_featureset_excludes_unavailable() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create available and unavailable features
    let available = create_test_feature(&space_id, server_id, "available_tool", FeatureType::Tool);
    let mut unavailable =
        create_test_feature(&space_id, server_id, "unavailable_tool", FeatureType::Tool);
    unavailable.is_available = false;

    feature_repo.upsert(&available).await.unwrap();
    feature_repo.upsert(&unavailable).await.unwrap();

    let all_fs = FeatureSet::new_all(&space_id);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(
        resolved.len(),
        1,
        "Only available feature should be resolved"
    );
    assert_eq!(resolved[0].feature_name, "available_tool");
}

// ============================================================================
// FEATURE SET TYPE: SERVER-ALL
// ============================================================================

#[tokio::test]
async fn test_server_all_grants_only_server_features() {
    let space_id = Uuid::new_v4().to_string();
    let server_a = "server-a";
    let server_b = "server-b";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features for both servers
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_a,
            "tool_a1",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_a,
            "tool_a2",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_b,
            "tool_b1",
            FeatureType::Tool,
        ))
        .await
        .unwrap();

    // Create ServerAll for server_a only
    let server_all = FeatureSet::new_server_all(&space_id, server_a, "Server A");
    let server_all_id = server_all.id.clone();
    feature_set_repo.create(&server_all).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[server_all_id])
        .await
        .unwrap();

    // Should only include server_a features
    assert_eq!(
        resolved.len(),
        2,
        "Only server_a features should be resolved"
    );
    assert!(resolved.iter().all(|f| f.server_id == server_a));
    assert!(resolved.iter().any(|f| f.feature_name == "tool_a1"));
    assert!(resolved.iter().any(|f| f.feature_name == "tool_a2"));
}

// ============================================================================
// FEATURE SET TYPE: DEFAULT (Empty = No features)
// ============================================================================

#[tokio::test]
async fn test_default_featureset_empty_grants_nothing() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();

    // Create empty Default feature set (secure by default)
    let default_fs = FeatureSet::new_default(&space_id);
    let default_fs_id = default_fs.id.clone();
    feature_set_repo.create(&default_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[default_fs_id])
        .await
        .unwrap();

    assert_eq!(resolved.len(), 0, "Empty default should grant no features");
}

// ============================================================================
// FEATURE SET TYPE: CUSTOM
// ============================================================================

#[tokio::test]
async fn test_custom_featureset_with_include_members() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features
    let tool_a = create_test_feature(&space_id, server_id, "tool_a", FeatureType::Tool);
    let tool_a_id = tool_a.id.to_string();
    let tool_b = create_test_feature(&space_id, server_id, "tool_b", FeatureType::Tool);
    let tool_b_id = tool_b.id.to_string();
    let tool_c = create_test_feature(&space_id, server_id, "tool_c", FeatureType::Tool);
    feature_repo.upsert(&tool_a).await.unwrap();
    feature_repo.upsert(&tool_b).await.unwrap();
    feature_repo.upsert(&tool_c).await.unwrap();

    // Create Custom feature set with specific members
    let mut custom_fs = FeatureSet::new_custom("Custom Set", &space_id);
    custom_fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: custom_fs.id.clone(),
        member_id: tool_a_id,
        member_type: MemberType::Feature,
        mode: MemberMode::Include,
    });
    custom_fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: custom_fs.id.clone(),
        member_id: tool_b_id,
        member_type: MemberType::Feature,
        mode: MemberMode::Include,
    });
    let custom_fs_id = custom_fs.id.clone();
    feature_set_repo.create(&custom_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[custom_fs_id])
        .await
        .unwrap();

    assert_eq!(
        resolved.len(),
        2,
        "Only included features should be resolved"
    );
    assert!(resolved.iter().any(|f| f.feature_name == "tool_a"));
    assert!(resolved.iter().any(|f| f.feature_name == "tool_b"));
    assert!(!resolved.iter().any(|f| f.feature_name == "tool_c"));
}

// ============================================================================
// NESTED FEATURE SETS
// ============================================================================

#[tokio::test]
async fn test_nested_featureset_composition() {
    let space_id = Uuid::new_v4().to_string();
    let server_a = "server-a";
    let server_b = "server-b";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_a,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_b,
            "tool_b",
            FeatureType::Tool,
        ))
        .await
        .unwrap();

    // Create ServerAll for each server
    let server_all_a = FeatureSet::new_server_all(&space_id, server_a, "Server A");
    let server_all_a_id = server_all_a.id.clone();
    let server_all_b = FeatureSet::new_server_all(&space_id, server_b, "Server B");
    let server_all_b_id = server_all_b.id.clone();
    feature_set_repo.create(&server_all_a).await.unwrap();
    feature_set_repo.create(&server_all_b).await.unwrap();

    // Create composite Custom feature set that includes both ServerAll sets
    let mut composite_fs = FeatureSet::new_custom("Composite", &space_id);
    composite_fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: composite_fs.id.clone(),
        member_id: server_all_a_id,
        member_type: MemberType::FeatureSet,
        mode: MemberMode::Include,
    });
    composite_fs.members.push(FeatureSetMember {
        id: Uuid::new_v4().to_string(),
        feature_set_id: composite_fs.id.clone(),
        member_id: server_all_b_id,
        member_type: MemberType::FeatureSet,
        mode: MemberMode::Include,
    });
    let composite_fs_id = composite_fs.id.clone();
    feature_set_repo.create(&composite_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[composite_fs_id])
        .await
        .unwrap();

    assert_eq!(resolved.len(), 2, "Both server features should be resolved");
    assert!(resolved
        .iter()
        .any(|f| f.feature_name == "tool_a" && f.server_id == server_a));
    assert!(resolved
        .iter()
        .any(|f| f.feature_name == "tool_b" && f.server_id == server_b));
}

// ============================================================================
// TYPE FILTERING
// ============================================================================

#[tokio::test]
async fn test_get_tools_for_grants() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create mixed features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "prompt_a",
            FeatureType::Prompt,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "resource://test",
            FeatureType::Resource,
        ))
        .await
        .unwrap();

    let all_fs = FeatureSet::new_all(&space_id);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let tools = service
        .get_tools_for_grants(&space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(tools.len(), 1, "Only tools should be returned");
    assert_eq!(tools[0].feature_type, FeatureType::Tool);
}

#[tokio::test]
async fn test_get_prompts_for_grants() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create mixed features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "prompt_a",
            FeatureType::Prompt,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "prompt_b",
            FeatureType::Prompt,
        ))
        .await
        .unwrap();

    let all_fs = FeatureSet::new_all(&space_id);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let prompts = service
        .get_prompts_for_grants(&space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(prompts.len(), 2, "Only prompts should be returned");
    assert!(prompts
        .iter()
        .all(|f| f.feature_type == FeatureType::Prompt));
}

#[tokio::test]
async fn test_get_resources_for_grants() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create mixed features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "resource://test",
            FeatureType::Resource,
        ))
        .await
        .unwrap();

    let all_fs = FeatureSet::new_all(&space_id);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resources = service
        .get_resources_for_grants(&space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(resources.len(), 1, "Only resources should be returned");
    assert_eq!(resources[0].feature_type, FeatureType::Resource);
}

// ============================================================================
// SPACE ISOLATION
// ============================================================================

#[tokio::test]
async fn test_features_isolated_by_space() {
    let space_a = Uuid::new_v4().to_string();
    let space_b = Uuid::new_v4().to_string();
    let server_id = "server-001";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features in different spaces
    feature_repo
        .upsert(&create_test_feature(
            &space_a,
            server_id,
            "tool_in_space_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_b,
            server_id,
            "tool_in_space_b",
            FeatureType::Tool,
        ))
        .await
        .unwrap();

    // Create All feature set for space_a
    let all_fs = FeatureSet::new_all(&space_a);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    // Resolve for space_a
    let resolved = service
        .resolve_feature_sets(&space_a, &[all_fs_id])
        .await
        .unwrap();

    // Should only get space_a feature
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].feature_name, "tool_in_space_a");
    assert_eq!(resolved[0].space_id, space_a);
}

// ============================================================================
// MULTIPLE GRANTS COMBINED
// ============================================================================

#[tokio::test]
async fn test_multiple_grants_union() {
    let space_id = Uuid::new_v4().to_string();
    let server_a = "server-a";
    let server_b = "server-b";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Create features
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_a,
            "tool_a",
            FeatureType::Tool,
        ))
        .await
        .unwrap();
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_b,
            "tool_b",
            FeatureType::Tool,
        ))
        .await
        .unwrap();

    // Create ServerAll for each server
    let server_all_a = FeatureSet::new_server_all(&space_id, server_a, "Server A");
    let server_all_a_id = server_all_a.id.clone();
    let server_all_b = FeatureSet::new_server_all(&space_id, server_b, "Server B");
    let server_all_b_id = server_all_b.id.clone();
    feature_set_repo.create(&server_all_a).await.unwrap();
    feature_set_repo.create(&server_all_b).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    // Resolve with multiple grants
    let resolved = service
        .resolve_feature_sets(&space_id, &[server_all_a_id, server_all_b_id])
        .await
        .unwrap();

    // Should include features from both servers
    assert_eq!(resolved.len(), 2);
    assert!(resolved.iter().any(|f| f.feature_name == "tool_a"));
    assert!(resolved.iter().any(|f| f.feature_name == "tool_b"));
}

#[tokio::test]
async fn test_prefix_enrichment() {
    let space_id = Uuid::new_v4().to_string();
    let server_id = "my-server";

    let feature_repo = Arc::new(MockServerFeatureRepository::new());
    let feature_set_repo = Arc::new(MockFeatureSetRepository::new());
    let prefix_cache = Arc::new(PrefixCacheService::new());

    // Register server prefix
    prefix_cache
        .assign_prefix_runtime(&space_id, server_id, Some("myalias"))
        .await;

    // Create feature
    feature_repo
        .upsert(&create_test_feature(
            &space_id,
            server_id,
            "my_tool",
            FeatureType::Tool,
        ))
        .await
        .unwrap();

    let all_fs = FeatureSet::new_all(&space_id);
    let all_fs_id = all_fs.id.clone();
    feature_set_repo.create(&all_fs).await.unwrap();

    let service = create_feature_service(feature_repo, feature_set_repo, prefix_cache);

    let resolved = service
        .resolve_feature_sets(&space_id, &[all_fs_id])
        .await
        .unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].server_alias, Some("myalias".to_string()));
}
