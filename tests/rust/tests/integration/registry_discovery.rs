//! Integration tests for `ServerDiscoveryService` loading the registry bundle.
//!
//! The registry can publish schema values an installed client predates (e.g.
//! `auth.type: "basic"` before v0.6.1). One such server must be skipped, not
//! fail the whole bundle and leave discovery empty, and the client must not
//! cache the bundle's ETag, or a later version that can read the skipped
//! server would get a 304 and keep serving a cache without it.

use std::path::Path;
use std::sync::Arc;

use mcpmux_core::{keys, AppSettingsService, AuthConfig, ServerDiscoveryService};
use serde_json::{json, Value};
use tempfile::TempDir;
use tests::mocks::MockAppSettingsRepository;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn server(id: &str, auth: Value) -> Value {
    json!({
        "id": id,
        "name": id,
        "transport": { "type": "stdio", "command": "npx" },
        "auth": auth,
    })
}

fn bundle(servers: Vec<Value>) -> Value {
    json!({
        "version": "2.1.0",
        "updated_at": "2026-10-01T00:00:00Z",
        "servers": servers,
        "categories": [],
        "ui": {
            "filters": [],
            "sort_options": [],
            "default_sort": "name",
            "items_per_page": 20
        }
    })
}

fn basic_server() -> Value {
    server(
        "basic-server",
        json!({ "type": "basic", "instructions": "dashboard user" }),
    )
}

fn future_server() -> Value {
    server("future-server", json!({ "type": "some_future_auth" }))
}

async fn serve_bundle(registry: &MockServer, bundle: Value, etag: &str) {
    Mock::given(method("GET"))
        .and(path("/v1/bundle"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", etag)
                .set_body_json(json!({ "data": bundle })),
        )
        .mount(registry)
        .await;
}

fn discovery(
    data_dir: &Path,
    registry_url: Option<String>,
    settings: Arc<AppSettingsService>,
) -> ServerDiscoveryService {
    let service = ServerDiscoveryService::new(data_dir.to_path_buf(), data_dir.join("spaces"))
        .with_settings_service(settings);
    match registry_url {
        Some(url) => service.with_registry_api(url),
        None => service,
    }
}

async fn server_ids(service: &ServerDiscoveryService) -> Vec<String> {
    let mut ids: Vec<String> = service.list().await.into_iter().map(|s| s.id).collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn unreadable_server_is_skipped_and_the_rest_load() {
    let registry = MockServer::start().await;
    serve_bundle(
        &registry,
        bundle(vec![basic_server(), future_server()]),
        "\"e1\"",
    )
    .await;
    let data_dir = TempDir::new().unwrap();
    let settings = Arc::new(AppSettingsService::new(Arc::new(
        MockAppSettingsRepository::new(),
    )));

    let service = discovery(data_dir.path(), Some(registry.uri()), settings);
    service.refresh().await.expect("refresh should succeed");

    assert_eq!(server_ids(&service).await, ["basic-server"]);
    assert!(!service.is_offline().await);
    let basic = service.get("basic-server").await.unwrap();
    assert!(
        matches!(&basic.auth, Some(AuthConfig::Basic { instructions: Some(i) }) if i == "dashboard user"),
        "unexpected auth: {:?}",
        basic.auth
    );
}

#[tokio::test]
async fn etag_is_not_kept_when_servers_were_skipped() {
    let registry = MockServer::start().await;
    serve_bundle(
        &registry,
        bundle(vec![basic_server(), future_server()]),
        "\"e2\"",
    )
    .await;
    let data_dir = TempDir::new().unwrap();
    // An ETag left over from an earlier, fully-readable bundle.
    let settings = Arc::new(AppSettingsService::new(Arc::new(
        MockAppSettingsRepository::new().with_setting(keys::registry::BUNDLE_ETAG, "\"e1\""),
    )));

    discovery(data_dir.path(), Some(registry.uri()), settings.clone())
        .refresh()
        .await
        .unwrap();
    assert_eq!(
        settings.get_string(keys::registry::BUNDLE_ETAG).await,
        None,
        "ETag must be cleared when the cached bundle is missing servers"
    );

    // Next start (e.g. after upgrading to a version that reads the skipped
    // server): the bundle must be fetched in full, not revalidated.
    discovery(data_dir.path(), Some(registry.uri()), settings)
        .refresh()
        .await
        .unwrap();
    let requests = registry.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].headers.get("if-none-match").is_none(),
        "second fetch revalidated with {:?}",
        requests[1].headers.get("if-none-match")
    );
}

#[tokio::test]
async fn etag_is_kept_when_every_server_was_read() {
    let registry = MockServer::start().await;
    serve_bundle(&registry, bundle(vec![basic_server()]), "\"e1\"").await;
    let data_dir = TempDir::new().unwrap();
    let settings = Arc::new(AppSettingsService::new(Arc::new(
        MockAppSettingsRepository::new(),
    )));

    discovery(data_dir.path(), Some(registry.uri()), settings.clone())
        .refresh()
        .await
        .unwrap();

    assert_eq!(
        settings.get_string(keys::registry::BUNDLE_ETAG).await,
        Some("\"e1\"".to_string())
    );
}

#[tokio::test]
async fn disk_cache_with_unreadable_server_still_loads() {
    let data_dir = TempDir::new().unwrap();
    let cache_dir = data_dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("registry-bundle.json"),
        bundle(vec![basic_server(), future_server()]).to_string(),
    )
    .unwrap();
    let settings = Arc::new(AppSettingsService::new(Arc::new(
        MockAppSettingsRepository::new(),
    )));

    // No registry client: the service falls back to the disk cache.
    let service = discovery(data_dir.path(), None, settings);
    service.refresh().await.unwrap();

    assert_eq!(server_ids(&service).await, ["basic-server"]);
    assert!(service.is_offline().await);
}

#[tokio::test]
async fn oversized_registry_bundle_is_refused() {
    let registry = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/bundle"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_bytes(vec![b' '; 33 * 1024 * 1024]),
        )
        .mount(&registry)
        .await;

    let client = mcpmux_core::RegistryApiClient::new(registry.uri());
    let err = match client.fetch_bundle(None).await {
        Err(e) => format!("{e:#}"),
        Ok(_) => panic!("an oversized bundle must be refused"),
    };
    assert!(err.contains("larger than"), "{err}");
}
