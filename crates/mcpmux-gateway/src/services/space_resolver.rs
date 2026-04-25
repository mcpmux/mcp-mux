//! Space Resolution Service
//!
//! Picks which Space a connecting client lands in. With per-client connection
//! modes gone, the answer is always "the active/default Space" — but this
//! service stays as a thin abstraction so callers don't reach into
//! SpaceRepository directly and so future per-session targeting (e.g.
//! WorkspaceBinding-driven space selection) has a single seam to extend.

use anyhow::{anyhow, Result};
use mcpmux_core::SpaceRepository;
use std::sync::Arc;
use uuid::Uuid;

pub struct SpaceResolverService {
    space_repo: Arc<dyn SpaceRepository>,
}

impl SpaceResolverService {
    pub fn new(space_repo: Arc<dyn SpaceRepository>) -> Self {
        Self { space_repo }
    }

    /// Resolve which space a client should access.
    ///
    /// Currently always returns the default/active Space — per-client pins
    /// no longer exist.  `client_id` is kept in the signature for forward
    /// compatibility with routing rules keyed on identity (e.g. future
    /// headless-connection policies).
    pub async fn resolve_space_for_client(&self, _client_id: &str) -> Result<Uuid> {
        let active_space = self
            .space_repo
            .get_default()
            .await?
            .ok_or_else(|| anyhow!("No active space set"))?;
        Ok(active_space.id)
    }
}
