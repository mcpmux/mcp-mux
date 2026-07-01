//! Pluggable backend for meta-gateway resource read and prompt fetch.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::pool::PoolService;

/// Reads resources and fetches prompts from backend MCP servers.
#[async_trait]
pub trait DisclosureBackend: Send + Sync {
    /// Read a backend resource URI and return MCP content blocks as JSON values.
    async fn read_resource(&self, space_id: Uuid, server_id: &str, uri: &str)
        -> Result<Vec<Value>>;

    /// Fetch a backend prompt and return the serialized MCP result.
    async fn fetch_prompt(
        &self,
        space_id: Uuid,
        server_id: &str,
        prompt_name: &str,
        arguments: Option<Map<String, Value>>,
    ) -> Result<Value>;
}

#[async_trait]
impl DisclosureBackend for PoolService {
    async fn read_resource(
        &self,
        space_id: Uuid,
        server_id: &str,
        uri: &str,
    ) -> Result<Vec<Value>> {
        PoolService::read_resource(self, space_id, server_id, uri).await
    }

    async fn fetch_prompt(
        &self,
        space_id: Uuid,
        server_id: &str,
        prompt_name: &str,
        arguments: Option<Map<String, Value>>,
    ) -> Result<Value> {
        PoolService::get_prompt(self, space_id, server_id, prompt_name, arguments).await
    }
}

/// Wrap a [`PoolService`] as a [`DisclosureBackend`] trait object.
pub fn pool_as_disclosure_backend(pool: Arc<PoolService>) -> Arc<dyn DisclosureBackend> {
    pool
}
