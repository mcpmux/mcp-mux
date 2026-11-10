//! Prior credential reuse integration tests (Issue #227)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use mcpmux_core::repository::{InstalledServerRepository, SpaceRepository};
use mcpmux_core::service::{find_prior_credential, get_prior_credential_values};
use mcpmux_storage::{
    generate_master_key, FieldEncryptor, SqliteInstalledServerRepository, SqliteSpaceRepository,
};
use pretty_assertions::assert_eq;
use tests::{db::TestDatabase, fixtures};

fn test_encryptor() -> Arc<FieldEncryptor> {
    let key = generate_master_key().expect("Failed to generate key");
    Arc::new(FieldEncryptor::new(&key).expect("Failed to create encryptor"))
}

#[tokio::test]
async fn test_find_prior_credential_no_candidates() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let space = fixtures::test_space("Personal");
    space_repo.create(&space).await.unwrap();

    let found = find_prior_credential(
        &server_repo,
        None,
        &space_repo,
        &space.id.to_string(),
        "io.github.github/github-mcp-server",
        None,
    )
    .await
    .unwrap();

    assert_eq!(found, None);
}

#[tokio::test]
async fn test_find_prior_credential_returns_summary_and_values() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let encryptor = test_encryptor();
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), encryptor);
    let space_repo = SqliteSpaceRepository::new(db);

    let work_space = fixtures::test_space("Work");
    let personal_space = fixtures::test_space("Personal");
    space_repo.create(&work_space).await.unwrap();
    space_repo.create(&personal_space).await.unwrap();

    // Install GitHub in Work space with PAT input
    let mut server = fixtures::test_installed_server(&work_space.id.to_string(), "github");
    let mut inputs = HashMap::new();
    inputs.insert(
        "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
        "ghp_secret_pat_99999".to_string(),
    );
    server.input_values = inputs;
    server_repo.install(&server).await.unwrap();

    // In Personal space, look for prior credentials
    let summary = find_prior_credential(
        &server_repo,
        None,
        &space_repo,
        &personal_space.id.to_string(),
        "github",
        None,
    )
    .await
    .unwrap()
    .expect("Expected prior credential summary");

    assert_eq!(summary.source_space_id, work_space.id.to_string());
    assert_eq!(summary.source_space_name, "Work");
    assert_eq!(summary.server_id, "github");
    assert_eq!(
        summary.available_keys,
        vec!["GITHUB_PERSONAL_ACCESS_TOKEN".to_string()]
    );

    // Fetch the actual decrypted values
    let values = get_prior_credential_values(
        &server_repo,
        None,
        &summary.source_space_id,
        &summary.server_id,
    )
    .await
    .unwrap();

    assert_eq!(
        values.get("GITHUB_PERSONAL_ACCESS_TOKEN"),
        Some(&"ghp_secret_pat_99999".to_string())
    );
}

#[tokio::test]
async fn test_find_prior_credential_ignores_empty_or_same_space() {
    let test_db = TestDatabase::new();
    let db = Arc::new(Mutex::new(test_db.db));
    let server_repo = SqliteInstalledServerRepository::new(Arc::clone(&db), test_encryptor());
    let space_repo = SqliteSpaceRepository::new(db);

    let work_space = fixtures::test_space("Work");
    let personal_space = fixtures::test_space("Personal");
    space_repo.create(&work_space).await.unwrap();
    space_repo.create(&personal_space).await.unwrap();

    // 1. Same space install shouldn't be matched
    let mut server = fixtures::test_installed_server(&personal_space.id.to_string(), "github");
    let mut inputs = HashMap::new();
    inputs.insert("TOKEN".to_string(), "abc".to_string());
    server.input_values = inputs;
    server_repo.install(&server).await.unwrap();

    let found = find_prior_credential(
        &server_repo,
        None,
        &space_repo,
        &personal_space.id.to_string(),
        "github",
        None,
    )
    .await
    .unwrap();

    assert_eq!(found, None);

    // 2. Empty values in other space shouldn't be matched
    let mut work_server = fixtures::test_installed_server(&work_space.id.to_string(), "slack");
    let mut empty_inputs = HashMap::new();
    empty_inputs.insert("SLACK_TOKEN".to_string(), "   ".to_string());
    work_server.input_values = empty_inputs;
    server_repo.install(&work_server).await.unwrap();

    let slack_found = find_prior_credential(
        &server_repo,
        None,
        &space_repo,
        &personal_space.id.to_string(),
        "slack",
        None,
    )
    .await
    .unwrap();

    assert_eq!(slack_found, None);
}
