//! Integration tests for `SpaceService::rename`, the write path behind the
//! desktop's "Rename Space" action.
//!
//! A Space name is presentation metadata: renaming must not touch the Space's
//! `is_default` flag, its base directories, or its workspace bindings, all of
//! which drive gateway routing.

use std::sync::Arc;

use mcpmux_core::{repository::SpaceRepository, service::SpaceService, Space};
use tests::mocks::MockSpaceRepository;
use uuid::Uuid;

#[tokio::test]
async fn rename_persists_the_new_name() {
    let space = Space::new("Work");
    let repo = Arc::new(MockSpaceRepository::new().with_space(space.clone()));
    let service = SpaceService::new(repo.clone());

    let renamed = service
        .rename(&space.id, "Clients".to_string())
        .await
        .expect("rename should succeed");

    assert_eq!(renamed.name, "Clients");
    assert!(renamed.updated_at >= space.updated_at);

    let stored = SpaceRepository::get(repo.as_ref(), &space.id)
        .await
        .expect("get should succeed")
        .expect("space should still exist");
    assert_eq!(stored.name, "Clients");
}

#[tokio::test]
async fn rename_preserves_routing_fields() {
    let mut space = Space::new("Work");
    space = space.set_default();
    space.icon = Some("briefcase".to_string());
    let repo = Arc::new(MockSpaceRepository::new().with_space(space.clone()));
    SpaceRepository::set_default(repo.as_ref(), &space.id)
        .await
        .expect("set_default should succeed");
    let service = SpaceService::new(repo.clone());

    let renamed = service
        .rename(&space.id, "Personal".to_string())
        .await
        .expect("rename should succeed");

    assert!(renamed.is_default);
    assert_eq!(renamed.icon, Some("briefcase".to_string()));
    assert_eq!(renamed.id, space.id);

    let stored = SpaceRepository::get_default(repo.as_ref())
        .await
        .expect("get_default should succeed")
        .expect("the default Space must still be reachable");
    assert_eq!(stored.name, "Personal");
}

#[tokio::test]
async fn rename_rejects_an_unknown_space() {
    let repo = Arc::new(MockSpaceRepository::new());
    let service = SpaceService::new(repo);

    let error = service
        .rename(&Uuid::new_v4(), "Ghost".to_string())
        .await
        .expect_err("renaming a missing Space must fail");

    assert!(error.to_string().contains("Space not found"));
}
