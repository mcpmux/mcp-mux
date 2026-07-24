//! Pluggable backend for `mcpmux_invoke_tool` routing.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::pool::{RoutingService, ToolCallResult};

/// Dispatches permission-checked tool calls to a backend MCP server.
#[async_trait]
pub trait InvokeToolBackend: Send + Sync {
    /// Invoke a qualified backend tool and return raw MCP content.
    async fn call_tool(
        &self,
        space_id: Uuid,
        feature_set_ids: &[String],
        qualified_name: &str,
        arguments: Value,
    ) -> Result<ToolCallResult>;
}

#[async_trait]
impl InvokeToolBackend for RoutingService {
    async fn call_tool(
        &self,
        space_id: Uuid,
        feature_set_ids: &[String],
        qualified_name: &str,
        arguments: Value,
    ) -> Result<ToolCallResult> {
        RoutingService::call_tool(self, space_id, feature_set_ids, qualified_name, arguments).await
    }
}

/// Wrap a [`RoutingService`] as an [`InvokeToolBackend`] trait object.
pub fn routing_as_invoke_backend(routing: Arc<RoutingService>) -> Arc<dyn InvokeToolBackend> {
    routing
}
