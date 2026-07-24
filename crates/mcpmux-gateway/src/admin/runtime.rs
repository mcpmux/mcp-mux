//! Runtime adapter for gateway-state dependent admin reads.

use anyhow::Result;
use async_trait::async_trait;
#[cfg(any(test, feature = "test-utils"))]
use serde_json::json;
use serde_json::Value;

/// Async runtime adapter for reads that depend on live gateway state.
#[async_trait]
pub trait GatewayRuntime: Send + Sync {
    async fn get_gateway_status(&self, _space_id: Option<String>) -> Result<Value>;
    async fn probe_gateway_start(&self, _port: Option<u16>) -> Result<Value>;
    async fn take_pending_port_conflict(&self) -> Result<Value>;
    async fn get_gateway_port_settings(&self) -> Result<Value>;
    async fn reset_gateway_port(&self) -> Result<Value>;
    async fn list_connected_servers(&self) -> Result<Value>;
    async fn get_pool_stats(&self) -> Result<Value>;
    async fn list_reported_workspace_roots(&self) -> Result<Value>;
    async fn list_meta_tool_grants(&self) -> Result<Value>;
    async fn get_oauth_clients(&self) -> Result<Value>;
    async fn get_oauth_client_grants(&self, _client_id: String, _space_id: String)
        -> Result<Value>;
    async fn get_server_statuses(&self, _space_id: String) -> Result<Value>;
    async fn clear_unmapped_reported_roots(&self, bound_roots_lower: Vec<String>) -> Result<Value>;
    async fn forget_reported_root(&self, root: String) -> Result<Value>;
}

/// Test/default runtime that returns empty or safe defaults.
#[cfg(any(test, feature = "test-utils"))]
pub struct StubGatewayRuntime;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl GatewayRuntime for StubGatewayRuntime {
    async fn get_gateway_status(&self, _space_id: Option<String>) -> Result<Value> {
        Ok(json!({
            "running": false,
            "url": null,
            "active_sessions": 0,
            "connected_backends": 0,
        }))
    }

    async fn probe_gateway_start(&self, _port: Option<u16>) -> Result<Value> {
        Ok(json!({
            "preferredPort": mcpmux_core::DEFAULT_GATEWAY_PORT,
            "preferredAvailable": true,
            "source": "default",
        }))
    }

    async fn take_pending_port_conflict(&self) -> Result<Value> {
        Ok(Value::Null)
    }

    async fn get_gateway_port_settings(&self) -> Result<Value> {
        Ok(json!({
            "configuredPort": null,
            "defaultPort": mcpmux_core::DEFAULT_GATEWAY_PORT,
            "activePort": null,
        }))
    }

    async fn reset_gateway_port(&self) -> Result<Value> {
        Ok(json!({ "ok": true }))
    }

    async fn list_connected_servers(&self) -> Result<Value> {
        Ok(json!([]))
    }

    async fn get_pool_stats(&self) -> Result<Value> {
        Ok(json!({
            "total_instances": 0,
            "connected_instances": 0,
            "total_space_server_mappings": 0,
        }))
    }

    async fn list_reported_workspace_roots(&self) -> Result<Value> {
        Ok(json!([]))
    }

    async fn list_meta_tool_grants(&self) -> Result<Value> {
        Ok(json!([]))
    }

    async fn get_oauth_clients(&self) -> Result<Value> {
        Ok(json!([]))
    }

    async fn get_oauth_client_grants(
        &self,
        _client_id: String,
        _space_id: String,
    ) -> Result<Value> {
        Ok(json!([]))
    }

    async fn get_server_statuses(&self, _space_id: String) -> Result<Value> {
        Ok(json!({}))
    }

    async fn clear_unmapped_reported_roots(
        &self,
        _bound_roots_lower: Vec<String>,
    ) -> Result<Value> {
        Ok(json!(0))
    }

    async fn forget_reported_root(&self, _root: String) -> Result<Value> {
        Ok(json!(false))
    }
}
