//! SpaceBaseDirRepository integration tests.
//!
//! Base dirs scope a workspace root to a Space. These cover CRUD, the
//! one-owner-per-path UNIQUE constraint, longest-prefix lookup, and that base
//! dirs cascade away with their Space.

use std::sync::Arc;

use mcpmux_core::repository::{SpaceBaseDirRepository, SpaceRepository};
use mcpmux_storage::{SqliteSpaceBaseDirRepository, SqliteSpaceRepository};
use tests::{db::TestDatabase, fixtures};
use tokio::sync::Mutex;

fn repos(test_db: TestDatabase) -> (SqliteSpaceRepository, SqliteSpaceBaseDirRepository) {
    let db = Arc::new(Mutex::new(test_db.db));
    (
        SqliteSpaceRepository::new(Arc::clone(&db)),
        SqliteSpaceBaseDirRepository::new(db),
    )
}

#[tokio::test]
async fn add_list_and_remove() {
    let (space_repo, repo) = repos(TestDatabase::new());
    let space = fixtures::test_space("Work");
    SpaceRepository::create(&space_repo, &space).await.unwrap();

    let bd = repo.add(&space.id, "/work").await.unwrap();
    assert_eq!(bd.path, "/work");
    assert_eq!(bd.space_id, space.id.to_string());

    assert_eq!(repo.list_by_space(&space.id).await.unwrap().len(), 1);
    assert_eq!(repo.list_all().await.unwrap().len(), 1);

    repo.remove(&bd.id).await.unwrap();
    assert!(repo.list_all().await.unwrap().is_empty());
}

#[tokio::test]
async fn path_is_unique_across_spaces() {
    let (space_repo, repo) = repos(TestDatabase::new());
    let a = fixtures::test_space("A");
    let b = fixtures::test_space("B");
    SpaceRepository::create(&space_repo, &a).await.unwrap();
    SpaceRepository::create(&space_repo, &b).await.unwrap();

    repo.add(&a.id, "/shared").await.unwrap();
    // The same folder can't be a base dir of a second space.
    let err = repo.add(&b.id, "/shared").await.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("already"),
        "expected a friendly duplicate error, got: {err}"
    );
}

#[tokio::test]
async fn find_space_for_root_longest_prefix_wins() {
    let (space_repo, repo) = repos(TestDatabase::new());
    let work = fixtures::test_space("Work");
    let client = fixtures::test_space("Client");
    SpaceRepository::create(&space_repo, &work).await.unwrap();
    SpaceRepository::create(&space_repo, &client).await.unwrap();

    repo.add(&work.id, "/work").await.unwrap();
    repo.add(&client.id, "/work/client").await.unwrap();

    // Under the nested base dir → the most-specific (longest) space.
    assert_eq!(
        repo.find_space_for_root("/work/client/app").await.unwrap(),
        Some(client.id)
    );
    // Under only the broad base dir → that space.
    assert_eq!(
        repo.find_space_for_root("/work/other").await.unwrap(),
        Some(work.id)
    );
    // Exact base-dir match resolves to its space.
    assert_eq!(
        repo.find_space_for_root("/work").await.unwrap(),
        Some(work.id)
    );
    // Not under any base dir.
    assert_eq!(repo.find_space_for_root("/elsewhere").await.unwrap(), None);
    // Sibling that merely shares a name prefix is NOT a match (segment boundary).
    assert_eq!(repo.find_space_for_root("/workspace").await.unwrap(), None);
}

#[tokio::test]
async fn base_dirs_cascade_on_space_delete() {
    let (space_repo, repo) = repos(TestDatabase::new());
    let space = fixtures::test_space("Temp");
    SpaceRepository::create(&space_repo, &space).await.unwrap();
    repo.add(&space.id, "/temp").await.unwrap();
    assert_eq!(repo.list_all().await.unwrap().len(), 1);

    SpaceRepository::delete(&space_repo, &space.id)
        .await
        .unwrap();
    assert!(
        repo.list_all().await.unwrap().is_empty(),
        "base dirs should cascade-delete with their space"
    );
}
