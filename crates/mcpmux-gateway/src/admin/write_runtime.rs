//! Runtime adapter for gateway-dependent admin write operations.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use mcpmux_core::InstalledServerRepository;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

use crate::pool::transport::resolution::{build_transport_config, TransportResolutionOptions};
use crate::pool::{
    ConnectionContext, ConnectionResult, FeatureService, PoolService, ServerKey, ServerManager,
};
use crate::server::GatewayServer;
use crate::services::ServerVersionProbeService;
use crate::GatewayState;

/// Async runtime adapter for writes that depend on live gateway / desktop state.
#[async_trait]
pub trait GatewayWriteRuntime: Send + Sync {
    async fn start_gateway(
        &self,
        port: Option<u16>,
        allow_dynamic_fallback: Option<bool>,
    ) -> Result<Value>;
    async fn stop_gateway(&self) -> Result<Value>;
    async fn restart_gateway(
        &self,
        port: Option<u16>,
        allow_dynamic_fallback: Option<bool>,
    ) -> Result<Value>;
    async fn disconnect_server(
        &self,
        server_id: String,
        space_id: String,
        logout: Option<bool>,
    ) -> Result<Value>;
    async fn connect_all_enabled_servers(&self) -> Result<Value>;
    async fn refresh_oauth_tokens_on_startup(&self) -> Result<Value>;
    async fn set_gateway_port(&self, port: u16) -> Result<Value>;
    async fn enable_server_v2(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn disable_server_v2(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn start_auth_v2(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn cancel_auth_v2(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn retry_connection(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn update_server_package(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn logout_server(&self, space_id: String, server_id: String) -> Result<Value>;
    async fn respond_to_meta_tool_approval(
        &self,
        request_id: String,
        client_id: String,
        tool_name: String,
        decision: String,
    ) -> Result<Value>;
    async fn revoke_meta_tool_grant(&self, client_id: String, tool_name: String) -> Result<Value>;
    async fn update_oauth_client(
        &self,
        client_id: String,
        client_alias: Option<String>,
        client_icon: Option<String>,
    ) -> Result<Value>;
    async fn delete_oauth_client(&self, client_id: String) -> Result<Value>;
    async fn grant_oauth_client_feature_set(
        &self,
        client_id: String,
        space_id: String,
        feature_set_id: String,
    ) -> Result<Value>;
    async fn revoke_oauth_client_feature_set(
        &self,
        client_id: String,
        space_id: String,
        feature_set_id: String,
    ) -> Result<Value>;
    /// Hot-reload the live resolver after `gateway.local_machine_id` changes.
    async fn hot_reload_local_machine_id(&self, machine_id: Option<uuid::Uuid>) -> Result<()>;
    /// Live gateway state for inbound OAuth consent (web admin).
    async fn gateway_state(&self) -> Option<Arc<RwLock<GatewayState>>>;
}

fn gateway_write_unavailable() -> anyhow::Error {
    anyhow!("Gateway write operation not implemented for this runtime")
}

/// Headless admin write runtime backed by a live [`GatewayServer`].
pub struct LiveGatewayWriteRuntime {
    gateway_state: Arc<RwLock<GatewayState>>,
    pool_service: Arc<PoolService>,
    server_manager: Arc<ServerManager>,
    feature_service: Arc<FeatureService>,
    feature_set_resolver: Arc<crate::services::FeatureSetResolverService>,
    installed_server_repo: Arc<dyn InstalledServerRepository>,
    data_dir: PathBuf,
    version_probe: Arc<ServerVersionProbeService>,
}

impl LiveGatewayWriteRuntime {
    /// Wire headless admin writes to an active gateway server instance.
    pub fn from_gateway_server(
        server: &GatewayServer,
        data_dir: PathBuf,
        installed_server_repo: Arc<dyn InstalledServerRepository>,
        version_probe: Arc<ServerVersionProbeService>,
    ) -> Self {
        Self {
            gateway_state: server.state(),
            pool_service: server.pool_service(),
            server_manager: server.server_manager(),
            feature_service: server.feature_service(),
            feature_set_resolver: server.feature_set_resolver(),
            installed_server_repo,
            data_dir,
            version_probe,
        }
    }
}

#[async_trait]
impl GatewayWriteRuntime for LiveGatewayWriteRuntime {
    async fn start_gateway(
        &self,
        _port: Option<u16>,
        _allow_dynamic_fallback: Option<bool>,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn stop_gateway(&self) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn restart_gateway(
        &self,
        _port: Option<u16>,
        _allow_dynamic_fallback: Option<bool>,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn disconnect_server(
        &self,
        _server_id: String,
        _space_id: String,
        _logout: Option<bool>,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn connect_all_enabled_servers(&self) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn refresh_oauth_tokens_on_startup(&self) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn set_gateway_port(&self, _port: u16) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn enable_server_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn disable_server_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn start_auth_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn cancel_auth_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn retry_connection(&self, space_id: String, server_id: String) -> Result<Value> {
        let space_uuid = Uuid::parse_str(&space_id)?;

        self.pool_service.remove_instance(space_uuid, &server_id);

        let installed = self
            .installed_server_repo
            .get_by_server_id(&space_id, &server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not found: {space_id}/{server_id}"))?;

        let server_definition = installed
            .get_definition()
            .ok_or_else(|| anyhow!("Server {server_id} has no cached definition"))?;

        let key = ServerKey::new(space_uuid, &server_id);
        self.server_manager.set_connecting(&key).await;

        let transport = build_transport_config(
            &server_definition.transport,
            &installed,
            Some(&self.data_dir),
            TransportResolutionOptions::default(),
        );

        let ctx = ConnectionContext::auto(space_uuid, server_id.clone(), transport);
        let result = self.pool_service.connect_server(&ctx).await;

        match result {
            ConnectionResult::Connected { features, .. } => {
                self.server_manager.set_connected(&key, features).await;
            }
            ConnectionResult::OAuthRequired { .. } => {
                self.server_manager.set_auth_required(&key, None).await;
                if let Err(error) = self
                    .feature_service
                    .mark_unavailable(&space_id, &server_id)
                    .await
                {
                    warn!("[LiveGatewayWriteRuntime] Failed to mark features unavailable: {error}");
                }
            }
            ConnectionResult::Failed { error } => {
                self.server_manager.set_error(&key, error.clone()).await;
                if let Err(mark_error) = self
                    .feature_service
                    .mark_unavailable(&space_id, &server_id)
                    .await
                {
                    warn!(
                        "[LiveGatewayWriteRuntime] Failed to mark features unavailable: {mark_error}"
                    );
                }
                return Err(anyhow!(error));
            }
        }

        Ok(json!({ "ok": true }))
    }

    async fn update_server_package(&self, space_id: String, server_id: String) -> Result<Value> {
        let space_uuid = Uuid::parse_str(&space_id)?;

        self.pool_service.remove_instance(space_uuid, &server_id);

        let installed = self
            .installed_server_repo
            .get_by_server_id(&space_id, &server_id)
            .await?
            .ok_or_else(|| anyhow!("Server not found: {space_id}/{server_id}"))?;

        let server_definition = installed
            .get_definition()
            .ok_or_else(|| anyhow!("Server {server_id} has no cached definition"))?;

        self.installed_server_repo
            .set_enabled(&installed.id, true)
            .await?;

        let key = ServerKey::new(space_uuid, &server_id);
        self.server_manager.set_connecting(&key).await;

        let transport = build_transport_config(
            &server_definition.transport,
            &installed,
            Some(&self.data_dir),
            TransportResolutionOptions {
                apply_package_update: true,
            },
        );

        let ctx = ConnectionContext::auto(space_uuid, server_id.clone(), transport);
        let result = self.pool_service.connect_server(&ctx).await;

        match result {
            ConnectionResult::Connected { features, .. } => {
                self.server_manager.set_connected(&key, features).await;
            }
            ConnectionResult::OAuthRequired { .. } => {
                self.server_manager.set_auth_required(&key, None).await;
                if let Err(error) = self
                    .feature_service
                    .mark_unavailable(&space_id, &server_id)
                    .await
                {
                    warn!("[LiveGatewayWriteRuntime] Failed to mark features unavailable: {error}");
                }
            }
            ConnectionResult::Failed { error } => {
                self.server_manager.set_error(&key, error.clone()).await;
                if let Err(mark_error) = self
                    .feature_service
                    .mark_unavailable(&space_id, &server_id)
                    .await
                {
                    warn!(
                        "[LiveGatewayWriteRuntime] Failed to mark features unavailable: {mark_error}"
                    );
                }
                return Err(anyhow!(error));
            }
        }

        if let Err(error) = self.version_probe.probe_server(&space_id, &server_id).await {
            warn!(
                "[LiveGatewayWriteRuntime] Post-update version probe failed for {server_id}: {error}"
            );
        }

        Ok(json!({ "ok": true }))
    }

    async fn logout_server(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn respond_to_meta_tool_approval(
        &self,
        _request_id: String,
        _client_id: String,
        _tool_name: String,
        _decision: String,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn revoke_meta_tool_grant(
        &self,
        _client_id: String,
        _tool_name: String,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn update_oauth_client(
        &self,
        _client_id: String,
        _client_alias: Option<String>,
        _client_icon: Option<String>,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn delete_oauth_client(&self, _client_id: String) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn grant_oauth_client_feature_set(
        &self,
        _client_id: String,
        _space_id: String,
        _feature_set_id: String,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn revoke_oauth_client_feature_set(
        &self,
        _client_id: String,
        _space_id: String,
        _feature_set_id: String,
    ) -> Result<Value> {
        Err(gateway_write_unavailable())
    }

    async fn hot_reload_local_machine_id(&self, machine_id: Option<Uuid>) -> Result<()> {
        self.feature_set_resolver
            .set_local_machine_id(machine_id)
            .await;
        Ok(())
    }

    async fn gateway_state(&self) -> Option<Arc<RwLock<GatewayState>>> {
        Some(self.gateway_state.clone())
    }
}

#[cfg(any(test, feature = "test-utils"))]
fn gateway_not_running() -> anyhow::Error {
    anyhow!("Gateway not running")
}

/// Test/default write runtime — gateway ops fail; port persist succeeds as no-op.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Default)]
pub struct StubGatewayWriteRuntime {
    pub gateway_port_service: Option<std::sync::Arc<mcpmux_core::GatewayPortService>>,
    pub gateway_state: Option<Arc<RwLock<GatewayState>>>,
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl GatewayWriteRuntime for StubGatewayWriteRuntime {
    async fn start_gateway(
        &self,
        _port: Option<u16>,
        _allow_dynamic_fallback: Option<bool>,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn stop_gateway(&self) -> Result<Value> {
        Ok(json!({ "ok": true }))
    }

    async fn restart_gateway(
        &self,
        _port: Option<u16>,
        _allow_dynamic_fallback: Option<bool>,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn disconnect_server(
        &self,
        _server_id: String,
        _space_id: String,
        _logout: Option<bool>,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn connect_all_enabled_servers(&self) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn refresh_oauth_tokens_on_startup(&self) -> Result<Value> {
        Ok(json!({
            "servers_checked": 0,
            "tokens_refreshed": 0,
            "refresh_failed": 0,
        }))
    }

    async fn set_gateway_port(&self, port: u16) -> Result<Value> {
        if port < 1024 {
            return Err(anyhow!(
                "Port {port} is in the privileged range (≤ 1023). Choose a port between 1024 and 65535."
            ));
        }
        if let Some(ref svc) = self.gateway_port_service {
            svc.save_port(port).await?;
        }
        Ok(json!({ "ok": true }))
    }

    async fn enable_server_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn disable_server_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn start_auth_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn cancel_auth_v2(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn retry_connection(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn update_server_package(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn logout_server(&self, _space_id: String, _server_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn respond_to_meta_tool_approval(
        &self,
        _request_id: String,
        _client_id: String,
        _tool_name: String,
        _decision: String,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn revoke_meta_tool_grant(
        &self,
        _client_id: String,
        _tool_name: String,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn update_oauth_client(
        &self,
        _client_id: String,
        _client_alias: Option<String>,
        _client_icon: Option<String>,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn delete_oauth_client(&self, _client_id: String) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn grant_oauth_client_feature_set(
        &self,
        _client_id: String,
        _space_id: String,
        _feature_set_id: String,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn revoke_oauth_client_feature_set(
        &self,
        _client_id: String,
        _space_id: String,
        _feature_set_id: String,
    ) -> Result<Value> {
        Err(gateway_not_running())
    }

    async fn hot_reload_local_machine_id(&self, _machine_id: Option<Uuid>) -> Result<()> {
        Ok(())
    }

    async fn gateway_state(&self) -> Option<Arc<RwLock<GatewayState>>> {
        self.gateway_state.clone()
    }
}
