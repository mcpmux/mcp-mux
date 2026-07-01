//! Admin read runtime backed by a live [`GatewayServer`].

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mcpmux_core::{is_port_available, GatewayPortService};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::admin::runtime::GatewayRuntime;
use crate::pool::{ConnectionStatus, PoolService, ServerManager};
use crate::server::{GatewayServer, GatewayState};
use crate::services::{ApprovalBroker, GrantService, SessionRootsRegistry};

/// Admin read runtime wired to a running MCP gateway.
pub struct LiveGatewayRuntime {
    gateway_state: Arc<RwLock<GatewayState>>,
    gateway_port_service: Arc<GatewayPortService>,
    listen_url: String,
    pool_service: Arc<PoolService>,
    server_manager: Arc<ServerManager>,
    session_roots: Arc<SessionRootsRegistry>,
    approval_broker: Arc<ApprovalBroker>,
    grant_service: Arc<GrantService>,
}

impl LiveGatewayRuntime {
    /// Connect admin bridge reads to an active gateway server instance.
    pub fn from_gateway_server(
        server: &GatewayServer,
        gateway_port_service: Arc<GatewayPortService>,
        listen_url: impl Into<String>,
    ) -> Self {
        Self {
            gateway_state: server.state(),
            gateway_port_service,
            listen_url: listen_url.into(),
            pool_service: server.pool_service(),
            server_manager: server.server_manager(),
            session_roots: server.session_roots(),
            approval_broker: server.approval_broker(),
            grant_service: server.grant_service(),
        }
    }
}

#[async_trait]
impl GatewayRuntime for LiveGatewayRuntime {
    async fn get_gateway_status(&self, space_id: Option<String>) -> Result<Value> {
        let active_sessions = self.gateway_state.read().await.sessions.len();
        let connected_backends = if let Some(space_id) = space_id {
            let space_uuid = Uuid::parse_str(&space_id)?;
            self.server_manager
                .connected_count_for_space(&space_uuid)
                .await
        } else {
            self.server_manager.connected_count().await
        };

        Ok(json!({
            "running": true,
            "url": self.listen_url,
            "active_sessions": active_sessions,
            "connected_backends": connected_backends,
        }))
    }

    async fn probe_gateway_start(&self, port: Option<u16>) -> Result<Value> {
        let (preferred_port, source) = if let Some(port) = port {
            (port, "override")
        } else if let Some(port) = self.gateway_port_service.load_persisted_port().await {
            (port, "configured")
        } else {
            (mcpmux_core::DEFAULT_GATEWAY_PORT, "default")
        };
        Ok(json!({
            "preferredPort": preferred_port,
            "preferredAvailable": is_port_available(preferred_port),
            "source": source,
        }))
    }

    async fn take_pending_port_conflict(&self) -> Result<Value> {
        Ok(Value::Null)
    }

    async fn get_gateway_port_settings(&self) -> Result<Value> {
        let configured_port = self.gateway_port_service.load_persisted_port().await;
        let active_port = self
            .listen_url
            .split("://")
            .nth(1)
            .and_then(|host_port| host_port.split('/').next())
            .and_then(|host_port| host_port.rsplit(':').next())
            .and_then(|port| port.parse::<u16>().ok());
        Ok(json!({
            "configuredPort": configured_port,
            "defaultPort": mcpmux_core::DEFAULT_GATEWAY_PORT,
            "activePort": active_port,
        }))
    }

    async fn reset_gateway_port(&self) -> Result<Value> {
        self.gateway_port_service.clear_persisted_port().await?;
        Ok(json!({ "ok": true }))
    }

    async fn list_connected_servers(&self) -> Result<Value> {
        Ok(json!([]))
    }

    async fn get_pool_stats(&self) -> Result<Value> {
        let stats = self.pool_service.stats();
        Ok(json!({
            "total_instances": stats.total_instances,
            "connected_instances": stats.connected_instances,
            "total_space_server_mappings": stats.connecting_instances
                + stats.failed_instances
                + stats.oauth_pending_instances,
        }))
    }

    async fn list_reported_workspace_roots(&self) -> Result<Value> {
        Ok(json!(self.session_roots.list_all_roots()))
    }

    async fn list_meta_tool_grants(&self) -> Result<Value> {
        Ok(json!(self
            .approval_broker
            .list_always_allow()
            .into_iter()
            .map(|(client_id, tool_name)| json!({
                "client_id": client_id,
                "tool_name": tool_name,
            }))
            .collect::<Vec<_>>()))
    }

    async fn get_oauth_clients(&self) -> Result<Value> {
        let gateway_state = self.gateway_state.read().await;
        let Some(repository) = gateway_state.inbound_client_repository() else {
            return Err(anyhow::anyhow!("Database not available"));
        };
        let clients = repository.list_clients().await?;
        let approved = clients
            .into_iter()
            .filter(|client| client.approved)
            .map(|client| {
                json!({
                    "client_id": client.client_id,
                    "registration_type": client.registration_type.as_str(),
                    "client_name": client.client_name,
                    "client_alias": client.client_alias,
                    "redirect_uris": client.redirect_uris,
                    "scope": client.scope,
                    "approved": client.approved,
                    "logo_uri": client.logo_uri,
                    "client_uri": client.client_uri,
                    "software_id": client.software_id,
                    "software_version": client.software_version,
                    "metadata_url": client.metadata_url,
                    "metadata_cached_at": client.metadata_cached_at,
                    "metadata_cache_ttl": client.metadata_cache_ttl,
                    "last_seen": client.last_seen,
                    "created_at": client.created_at,
                    "reports_roots": client.reports_roots,
                    "roots_capability_known": client.roots_capability_known,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!(approved))
    }

    async fn get_oauth_client_grants(&self, client_id: String, space_id: String) -> Result<Value> {
        Ok(json!(
            self.grant_service
                .get_grants_for_space(&client_id, &space_id)
                .await?
        ))
    }

    async fn get_server_statuses(&self, space_id: String) -> Result<Value> {
        let space_uuid =
            Uuid::parse_str(&space_id).map_err(|e| anyhow::anyhow!("Invalid space_id: {e}"))?;
        let statuses = self.server_manager.get_all_statuses(space_uuid).await;
        let mut result = serde_json::Map::new();
        for (server_id, (status, flow_id, has_connected_before, message)) in statuses {
            result.insert(
                server_id.clone(),
                json!({
                    "server_id": server_id,
                    "status": connection_status_to_ui(status),
                    "flow_id": flow_id,
                    "has_connected_before": has_connected_before,
                    "message": message,
                }),
            );
        }
        Ok(json!(result))
    }

    async fn clear_unmapped_reported_roots(&self, bound_roots_lower: Vec<String>) -> Result<Value> {
        use std::collections::HashSet;

        let bound: HashSet<String> = bound_roots_lower.into_iter().collect();
        let dropped = self
            .session_roots
            .forget_unmapped_roots(|root| bound.contains(&root.to_lowercase()));
        let count = dropped.len();
        if count > 0 {
            self.gateway_state
                .read()
                .await
                .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);
        }
        Ok(json!(count))
    }

    async fn forget_reported_root(&self, root: String) -> Result<Value> {
        let found = self.session_roots.forget_root(&root);
        if found {
            self.gateway_state
                .read()
                .await
                .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);
        }
        Ok(json!(found))
    }
}

fn connection_status_to_ui(status: ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Disconnected => "disconnected",
        ConnectionStatus::Connecting => "connecting",
        ConnectionStatus::Connected => "connected",
        ConnectionStatus::Refreshing => "refreshing",
        ConnectionStatus::AuthRequired => "oauth_required",
        ConnectionStatus::Authenticating => "authenticating",
        ConnectionStatus::Error => "error",
    }
}
