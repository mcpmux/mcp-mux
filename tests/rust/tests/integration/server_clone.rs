//! Integration tests for server account clones — lifecycle, prefixes, and uninstall edges.

use std::collections::HashMap;
use std::sync::Arc;

use mcpmux_core::{
    application::ServerAppService, EventBus, InstalledServer, InstalledServerRepository,
    ServerDefinition, ServerDiscoveryService, ServerFeature, ServerFeatureRepository,
    ServerSource, SpaceRepository, TransportConfig, TransportMetadata,
};
use mcpmux_gateway::{FeatureService, PrefixCacheService, SessionOverrideRegistry};
use mcpmux_storage::{
    generate_master_key, FieldEncryptor, SqliteInstalledServerRepository,
    SqliteServerFeatureRepository, SqliteSpaceRepository,
};
use tests::db::TestDatabase;
use tests::fixtures;
use tokio::sync::Mutex;
use uuid::Uuid;

struct CloneFixture {
    service: ServerAppService,
    installed_server_repo: Arc<dyn InstalledServerRepository>,
    feature_repo: Arc<dyn ServerFeatureRepository>,
    prefix_cache: Arc<PrefixCacheService>,
    feature_service: Arc<FeatureService>,
    space_id: Uuid,
}

impl CloneFixture {
    async fn new() -> Self {
        let test_db = TestDatabase::in_memory();
        let db = Arc::new(Mutex::new(test_db.db));
        let key = generate_master_key().expect("generate key");
        let encryptor = Arc::new(FieldEncryptor::new(&key).expect("create encryptor"));

        let space_repo = SqliteSpaceRepository::new(db.clone());
        let default_space = space_repo.get_default().await.unwrap().unwrap();
        let space_id = default_space.id;

        let installed_server_repo: Arc<dyn InstalledServerRepository> = Arc::new(
            SqliteInstalledServerRepository::new(db.clone(), encryptor),
        );
        let feature_repo: Arc<dyn ServerFeatureRepository> =
            Arc::new(SqliteServerFeatureRepository::new(db));

        let prefix_cache = Arc::new(
            PrefixCacheService::new().with_dependencies(
                installed_server_repo.clone(),
                Arc::new(ServerDiscoveryService::new(
                    std::env::temp_dir().join(format!("mcpmux-clone-test-{}", Uuid::new_v4())),
                    std::env::temp_dir().join(format!("mcpmux-clone-spaces-{}", Uuid::new_v4())),
                )),
            ),
        );
        let feature_service = Arc::new(FeatureService::new(
            feature_repo.clone(),
            Arc::new(tests::mocks::MockFeatureSetRepository::new()),
            prefix_cache.clone(),
            SessionOverrideRegistry::new(),
        ));

        let service = ServerAppService::new(
            installed_server_repo.clone(),
            Some(feature_repo.clone()),
            None,
            EventBus::new().sender(),
        );

        Self {
            service,
            installed_server_repo,
            feature_repo,
            prefix_cache,
            feature_service,
            space_id,
        }
    }

    fn space_id_str(&self) -> String {
        self.space_id.to_string()
    }
}

fn env_stdio_definition(server_id: &str, name: &str, alias: &str) -> ServerDefinition {
    ServerDefinition {
        id: server_id.to_string(),
        name: name.to_string(),
        description: None,
        alias: Some(alias.to_string()),
        auth: None,
        icon: None,
        transport: TransportConfig::Stdio {
            command: "echo".to_string(),
            args: vec!["mcp".to_string()],
            env: HashMap::from([("ACCOUNT".to_string(), "${ACCOUNT}".to_string())]),
            metadata: TransportMetadata::default(),
        },
        categories: vec![],
        publisher: None,
        source: ServerSource::Bundled,
        badges: vec![],
        hosting_type: Default::default(),
        license: None,
        license_url: None,
        installation: None,
        capabilities: None,
        sponsored: None,
        media: None,
        changelog_url: None,
    }
}

async fn seed_tool(
    feature_repo: &Arc<dyn ServerFeatureRepository>,
    space_id: &str,
    server_id: &str,
    tool_name: &str,
) {
    let mut feature = ServerFeature::tool(space_id, server_id, tool_name);
    feature.is_available = true;
    feature_repo.upsert(&feature).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_clones_have_distinct_prefixes_and_env() {
    let fixture = CloneFixture::new().await;
    let space_id = fixture.space_id;
    let space_id_str = fixture.space_id_str();

    let source = fixtures::test_installed_server(&space_id_str, "posthog")
        .with_definition(&env_stdio_definition("posthog", "PostHog", "posthog"));
    fixture
        .installed_server_repo
        .install(&source)
        .await
        .unwrap();

    let clone_work = fixture
        .service
        .clone_server(space_id, "posthog", "work", None)
        .await
        .expect("clone work");
    let clone_personal = fixture
        .service
        .clone_server(space_id, "posthog", "personal", None)
        .await
        .expect("clone personal");

    fixture
        .service
        .update_config(
            space_id,
            "posthog-work",
            HashMap::from([("ACCOUNT".to_string(), "work-account".to_string())]),
            Some(HashMap::from([(
                "ACCOUNT".to_string(),
                "work-account".to_string(),
            )])),
            None,
            None,
        )
        .await
        .unwrap();
    fixture
        .service
        .update_config(
            space_id,
            "posthog-personal",
            HashMap::from([("ACCOUNT".to_string(), "personal-account".to_string())]),
            Some(HashMap::from([(
                "ACCOUNT".to_string(),
                "personal-account".to_string(),
            )])),
            None,
            None,
        )
        .await
        .unwrap();

    seed_tool(
        &fixture.feature_repo,
        &space_id_str,
        "posthog-work",
        "capture",
    )
    .await;
    seed_tool(
        &fixture.feature_repo,
        &space_id_str,
        "posthog-personal",
        "capture",
    )
    .await;

    let work_prefix = fixture
        .prefix_cache
        .assign_prefix_for_server(&space_id_str, "posthog-work")
        .await;
    let personal_prefix = fixture
        .prefix_cache
        .assign_prefix_for_server(&space_id_str, "posthog-personal")
        .await;

    assert_eq!(work_prefix, "work");
    assert_eq!(personal_prefix, "personal");
    assert_ne!(work_prefix, personal_prefix);

    let work_resolved = fixture
        .feature_service
        .find_server_for_qualified_tool(&space_id_str, "work_capture")
        .await
        .unwrap()
        .expect("work tool resolves");
    let personal_resolved = fixture
        .feature_service
        .find_server_for_qualified_tool(&space_id_str, "personal_capture")
        .await
        .unwrap()
        .expect("personal tool resolves");

    assert_eq!(work_resolved.0, "posthog-work");
    assert_eq!(personal_resolved.0, "posthog-personal");

    let stored_work = fixture
        .installed_server_repo
        .get_by_server_id(&space_id_str, "posthog-work")
        .await
        .unwrap()
        .unwrap();
    let stored_personal = fixture
        .installed_server_repo
        .get_by_server_id(&space_id_str, "posthog-personal")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        stored_work.input_values.get("ACCOUNT").map(String::as_str),
        Some("work-account")
    );
    assert_eq!(
        stored_personal.input_values.get("ACCOUNT").map(String::as_str),
        Some("personal-account")
    );
    assert_eq!(clone_work.cloned_from.as_deref(), Some("posthog"));
    assert_eq!(clone_personal.cloned_from.as_deref(), Some("posthog"));
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstall_clone_does_not_affect_source() {
    let fixture = CloneFixture::new().await;
    let space_id = fixture.space_id;
    let space_id_str = fixture.space_id_str();

    let source = fixtures::test_installed_server(&space_id_str, "posthog")
        .with_definition(&env_stdio_definition("posthog", "PostHog", "posthog"));
    fixture
        .installed_server_repo
        .install(&source)
        .await
        .unwrap();
    fixture
        .service
        .clone_server(space_id, "posthog", "work", None)
        .await
        .unwrap();

    fixture
        .service
        .uninstall(space_id, "posthog-work")
        .await
        .expect("clone uninstall");

    assert!(
        fixture
            .installed_server_repo
            .get_by_server_id(&space_id_str, "posthog")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .installed_server_repo
            .get_by_server_id(&space_id_str, "posthog-work")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_clone_dependents_returns_source_clones() {
    let fixture = CloneFixture::new().await;
    let space_id = fixture.space_id;
    let space_id_str = fixture.space_id_str();

    let source = fixtures::test_installed_server(&space_id_str, "posthog")
        .with_definition(&env_stdio_definition("posthog", "PostHog", "posthog"));
    fixture
        .installed_server_repo
        .install(&source)
        .await
        .unwrap();
    fixture
        .service
        .clone_server(space_id, "posthog", "work", None)
        .await
        .unwrap();
    fixture
        .service
        .clone_server(space_id, "posthog", "personal", None)
        .await
        .unwrap();

    let dependents = fixture
        .service
        .list_clone_dependents(&space_id_str, "posthog")
        .await
        .unwrap();

    assert_eq!(dependents.len(), 2);
    let ids: Vec<_> = dependents
        .iter()
        .map(|server| server.server_id.as_str())
        .collect();
    assert!(ids.contains(&"posthog-work"));
    assert!(ids.contains(&"posthog-personal"));
}

#[tokio::test(flavor = "multi_thread")]
async fn clone_prefixes_do_not_break_existing_alias_uniqueness() {
    let fixture = CloneFixture::new().await;
    let space_id_str = fixture.space_id_str();

    let posthog = fixtures::test_installed_server(&space_id_str, "posthog")
        .with_definition(&env_stdio_definition("posthog", "PostHog", "api"));
    let other = fixtures::test_installed_server(&space_id_str, "other-server")
        .with_definition(&env_stdio_definition("other-server", "Other", "api"));

    fixture.installed_server_repo.install(&posthog).await.unwrap();
    fixture.installed_server_repo.install(&other).await.unwrap();

    let clone = InstalledServer::new(&space_id_str, "posthog-work")
        .with_definition(&env_stdio_definition("posthog-work", "PostHog (work)", "work"))
        .with_cloned_from("posthog");
    fixture.installed_server_repo.install(&clone).await.unwrap();

    let posthog_prefix = fixture
        .prefix_cache
        .assign_prefix_runtime(&space_id_str, "posthog", Some("api"))
        .await;
    let clone_prefix = fixture
        .prefix_cache
        .assign_prefix_runtime(&space_id_str, "posthog-work", Some("work"))
        .await;
    let other_prefix = fixture
        .prefix_cache
        .assign_prefix_runtime(&space_id_str, "other-server", Some("api"))
        .await;

    assert_eq!(posthog_prefix, "api");
    assert_eq!(clone_prefix, "work");
    assert_eq!(other_prefix, "other-server");
    assert!(
        !fixture
            .prefix_cache
            .is_prefix_available(&space_id_str, "api")
            .await
    );
    assert!(
        !fixture
            .prefix_cache
            .is_prefix_available(&space_id_str, "work")
            .await
    );
}
