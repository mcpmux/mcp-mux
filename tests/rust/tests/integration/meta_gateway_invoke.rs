//! Integration tests for meta-gateway invoke (search → schema → invoke).

use std::sync::Arc;
use std::time::Duration;

use mcpmux_core::{
    Client, DomainEvent, FeatureSet, FeatureSetMember, FeatureSetRepository,
    InboundMcpClientRepository, InstalledServerRepository, MemberMode, MemberType, ServerFeature,
    ServerFeatureRepository, SpaceRepository, WorkspaceBindingRepository,
};
use mcpmux_gateway::pool::{format_direct_call_redirect, FeatureService};
use mcpmux_gateway::services::{
    meta_tools, ApprovalBroker, FeatureSetResolverService, MetaToolRegistry, PrefixCacheService,
    SessionOverrideRegistry, SessionRootsRegistry,
};
use mcpmux_storage::{
    generate_master_key, Database, FieldEncryptor, InboundClientRepository,
    SqliteFeatureSetRepository, SqliteInboundMcpClientRepository, SqliteInstalledServerRepository,
    SqliteServerFeatureRepository, SqliteSpaceRepository, SqliteWorkspaceBindingRepository,
};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

struct Fixture {
    registry: Arc<MetaToolRegistry>,
    feature_service: Arc<FeatureService>,
    session_overrides: Arc<SessionOverrideRegistry>,
    server_feature_repo: Arc<dyn ServerFeatureRepository>,
    feature_set_repo: Arc<dyn FeatureSetRepository>,
    space_id: Uuid,
    client_id: String,
    session_id: String,
}

fn test_encryptor() -> Arc<FieldEncryptor> {
    let key = generate_master_key().expect("generate key");
    Arc::new(FieldEncryptor::new(&key).expect("create encryptor"))
}

impl Fixture {
    async fn new() -> Self {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));

        let space_repo: Arc<dyn SpaceRepository> = Arc::new(SqliteSpaceRepository::new(db.clone()));
        let feature_set_repo: Arc<dyn FeatureSetRepository> =
            Arc::new(SqliteFeatureSetRepository::new(db.clone()));
        let client_repo: Arc<dyn InboundMcpClientRepository> =
            Arc::new(SqliteInboundMcpClientRepository::new(db.clone()));
        let binding_repo: Arc<dyn WorkspaceBindingRepository> =
            Arc::new(SqliteWorkspaceBindingRepository::new(db.clone()));
        let server_feature_repo: Arc<dyn ServerFeatureRepository> =
            Arc::new(SqliteServerFeatureRepository::new(db.clone()));
        let installed_server_repo: Arc<dyn InstalledServerRepository> = Arc::new(
            SqliteInstalledServerRepository::new(db.clone(), test_encryptor()),
        );

        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        let client = Client::new("InvokeTestClient", "test-type");
        let client_id = client.id.to_string();
        client_repo.create(&client).await.unwrap();

        let mut list_issues = ServerFeature::tool(space_id, "github", "list_issues");
        list_issues.description = Some("List issues in a repository".into());
        list_issues.raw_json = Some(json!({
            "name": "list_issues",
            "description": "List issues in a repository",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string" },
                    "repo": { "type": "string" }
                },
                "required": ["owner", "repo"]
            }
        }));
        server_feature_repo.upsert(&list_issues).await.unwrap();

        let mut grant_all = FeatureSet::new_custom("Grant GitHub", space_id.to_string());
        grant_all.members.push(FeatureSetMember {
            id: Uuid::new_v4().to_string(),
            feature_set_id: grant_all.id.clone(),
            member_type: MemberType::Feature,
            member_id: list_issues.id.to_string(),
            mode: MemberMode::Include,
        });
        feature_set_repo.create(&grant_all).await.unwrap();

        let session_roots = SessionRootsRegistry::new();
        let session_overrides = SessionOverrideRegistry::new();
        let session_id = "sess-invoke".to_string();

        let inbound_client_repo = Arc::new(InboundClientRepository::new(db.clone()));
        let resolver = Arc::new(FeatureSetResolverService::new(
            space_repo.clone(),
            binding_repo.clone(),
            session_roots.clone(),
            inbound_client_repo.clone(),
        ));

        let prefix_cache = Arc::new(PrefixCacheService::new());
        let feature_service = Arc::new(FeatureService::new(
            server_feature_repo.clone(),
            feature_set_repo.clone(),
            prefix_cache,
            session_overrides.clone(),
        ));

        let broker = Arc::new(ApprovalBroker::new().with_timeout(Duration::from_millis(500)));
        let (tx, _event_rx) = broadcast::channel::<DomainEvent>(32);

        let registry = meta_tools::build_default_registry(
            client_repo,
            space_repo,
            feature_set_repo.clone(),
            binding_repo,
            server_feature_repo.clone(),
            installed_server_repo,
            resolver,
            feature_service.clone(),
            None,
            session_roots,
            session_overrides.clone(),
            broker,
            tx,
            None,
        );

        Self {
            registry,
            feature_service,
            session_overrides,
            server_feature_repo,
            feature_set_repo,
            space_id,
            client_id,
            session_id,
        }
    }

    fn result_json(result: &rmcp::model::CallToolResult) -> Value {
        let raw = serde_json::to_value(result).unwrap();
        raw.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("text"))
            .and_then(|t| t.as_str())
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or(raw)
    }

    async fn call(&self, name: &str, args: Value) -> rmcp::model::CallToolResult {
        match self
            .registry
            .call(name, &self.client_id, Some(&self.session_id), args)
            .await
        {
            Ok(r) => r,
            Err(e) => e.into_call_tool_result(),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn advertised_tools_empty_while_invokable_has_backend_tools() {
    let f = Fixture::new().await;
    let fs_ids = vec![
        f.feature_set_repo
            .list_by_space(&f.space_id.to_string())
            .await
            .unwrap()
            .into_iter()
            .find(|fs| fs.name == "Grant GitHub")
            .unwrap()
            .id,
    ];

    let advertised = f
        .feature_service
        .get_advertised_tools_for_grants(&f.space_id.to_string(), &fs_ids, Some(&f.session_id))
        .await
        .unwrap();
    assert!(advertised.is_empty(), "Phase A: no surfaced tools yet");

    f.session_overrides.enable(&f.session_id, "github");
    let invokable = f
        .feature_service
        .get_invokable_tools_for_grants(&f.space_id.to_string(), &fs_ids, Some(&f.session_id))
        .await
        .unwrap();
    assert_eq!(invokable.len(), 1);
    assert_eq!(invokable[0].feature_name, "list_issues");
}

#[tokio::test(flavor = "multi_thread")]
async fn github_read_path_enable_search_schema() {
    let f = Fixture::new().await;

    let servers = f.call("mcpmux_list_servers", json!({})).await;
    let body = Fixture::result_json(&servers);
    let github = body
        .get("servers")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.iter().find(|s| s.get("id") == Some(&json!("github"))))
        .expect("github server listed");
    assert_eq!(github.get("status"), Some(&json!("inactive")));

    f.session_overrides.enable(&f.session_id, "github");

    let search = f
        .call(
            "mcpmux_search_tools",
            json!({
                "query": "list issues",
                "server_id": "github",
                "detail_level": "description"
            }),
        )
        .await;
    let search_body = Fixture::result_json(&search);
    let tools = search_body.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("qualified_name"),
        Some(&json!("github_list_issues"))
    );

    let schema = f
        .call(
            "mcpmux_get_tool_schema",
            json!({ "tools": "github_list_issues" }),
        )
        .await;
    let schema_body = Fixture::result_json(&schema);
    let schemas = schema_body.get("schemas").unwrap().as_array().unwrap();
    assert_eq!(schemas.len(), 1);
    assert!(schemas[0].get("input_schema").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn invoke_denied_when_server_inactive() {
    let f = Fixture::new().await;
    let result = f
        .call(
            "mcpmux_invoke_tool",
            json!({
                "server_id": "github",
                "tool": "list_issues",
                "args": { "owner": "mcpmux", "repo": "mcp-mux" }
            }),
        )
        .await;
    assert!(result.is_error.unwrap_or(false));
    let body = Fixture::result_json(&result);
    let message = body.get("message").and_then(|m| m.as_str()).unwrap_or("");
    assert!(message.contains("inactive"));
    assert!(message.contains("mcpmux_enable_server"));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_empty_when_server_inactive() {
    let f = Fixture::new().await;
    let search = f
        .call(
            "mcpmux_search_tools",
            json!({ "query": "list", "server_id": "github" }),
        )
        .await;
    let body = Fixture::result_json(&search);
    assert_eq!(body.get("total"), Some(&json!(0)));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_all_tools_filters_by_server_id() {
    let f = Fixture::new().await;

    let other = ServerFeature::tool(f.space_id, "firebase", "deploy");
    f.server_feature_repo.upsert(&other).await.unwrap();

    let all = f.call("mcpmux_list_all_tools", json!({})).await;
    let all_body = Fixture::result_json(&all);
    assert_eq!(all_body.get("tools").unwrap().as_array().unwrap().len(), 2);

    let filtered = f
        .call("mcpmux_list_all_tools", json!({ "server_id": "github" }))
        .await;
    let filtered_body = Fixture::result_json(&filtered);
    let tools = filtered_body.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("server_id"), Some(&json!("github")));
}

#[tokio::test(flavor = "multi_thread")]
async fn direct_backend_call_redirect_message() {
    let msg = format_direct_call_redirect("github_list_issues", "github", "list_issues");
    assert!(msg.contains("mcpmux_invoke_tool"));
    assert!(msg.contains("github"));
    assert!(msg.contains("list_issues"));
}

#[tokio::test(flavor = "multi_thread")]
async fn registry_lists_new_meta_tools() {
    let f = Fixture::new().await;
    let names: Vec<String> = f
        .registry
        .list_as_tools()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.iter().any(|n| n == "mcpmux_search_tools"));
    assert!(names.iter().any(|n| n == "mcpmux_get_tool_schema"));
    assert!(names.iter().any(|n| n == "mcpmux_invoke_tool"));
}
