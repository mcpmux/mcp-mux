//! Desktop implementation of admin gateway write runtime — delegates to Tauri commands.

use async_trait::async_trait;
use mcpmux_gateway::admin::write_runtime::GatewayWriteRuntime;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::RwLock;

use crate::commands::gateway::{
    connect_all_enabled_servers, disconnect_server, refresh_oauth_tokens_on_startup,
    restart_gateway, set_gateway_port, start_gateway, stop_gateway, GatewayAppState,
};
use crate::commands::meta_tool_approval::{respond_to_meta_tool_approval, revoke_meta_tool_grant};
use crate::commands::oauth::{
    delete_oauth_client, grant_oauth_client_feature_set, revoke_oauth_client_feature_set,
    update_oauth_client, UpdateClientSettingsRequest,
};
use crate::commands::server_manager::{
    cancel_auth_v2, disable_server_v2, enable_server_v2, logout_server, retry_connection,
    start_auth_v2,
};

/// Delegates admin write operations to existing Tauri command handlers.
pub struct DesktopGatewayWriteRuntime {
    app_handle: AppHandle,
    app_gateway_state: Arc<RwLock<GatewayAppState>>,
}

impl DesktopGatewayWriteRuntime {
    /// Create a write runtime bound to the running Tauri app handle.
    pub fn new(app_handle: AppHandle, app_gateway_state: Arc<RwLock<GatewayAppState>>) -> Self {
        Self {
            app_handle,
            app_gateway_state,
        }
    }
}

#[async_trait]
impl GatewayWriteRuntime for DesktopGatewayWriteRuntime {
    async fn start_gateway(
        &self,
        port: Option<u16>,
        allow_dynamic_fallback: Option<bool>,
    ) -> anyhow::Result<Value> {
        let url = start_gateway(
            port,
            allow_dynamic_fallback,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "url": url }))
    }

    async fn stop_gateway(&self) -> anyhow::Result<Value> {
        stop_gateway(
            self.app_handle
                .state::<std::sync::Arc<RwLock<GatewayAppState>>>(),
            self.app_handle.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn restart_gateway(
        &self,
        port: Option<u16>,
        allow_dynamic_fallback: Option<bool>,
    ) -> anyhow::Result<Value> {
        let url = restart_gateway(
            port,
            allow_dynamic_fallback,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "url": url }))
    }

    async fn disconnect_server(
        &self,
        server_id: String,
        space_id: String,
        logout: Option<bool>,
    ) -> anyhow::Result<Value> {
        disconnect_server(
            server_id,
            space_id,
            logout,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn connect_all_enabled_servers(&self) -> anyhow::Result<Value> {
        let result = connect_all_enabled_servers(self.app_handle.state(), self.app_handle.state())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!(result))
    }

    async fn refresh_oauth_tokens_on_startup(&self) -> anyhow::Result<Value> {
        let result = refresh_oauth_tokens_on_startup(self.app_handle.state())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!(result))
    }

    async fn set_gateway_port(&self, port: u16) -> anyhow::Result<Value> {
        set_gateway_port(port, self.app_handle.state())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn enable_server_v2(&self, space_id: String, server_id: String) -> anyhow::Result<Value> {
        enable_server_v2(
            space_id,
            server_id,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn disable_server_v2(
        &self,
        space_id: String,
        server_id: String,
    ) -> anyhow::Result<Value> {
        disable_server_v2(
            space_id,
            server_id,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn start_auth_v2(&self, space_id: String, server_id: String) -> anyhow::Result<Value> {
        start_auth_v2(
            space_id,
            server_id,
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn cancel_auth_v2(&self, space_id: String, server_id: String) -> anyhow::Result<Value> {
        cancel_auth_v2(
            space_id,
            server_id,
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn retry_connection(&self, space_id: String, server_id: String) -> anyhow::Result<Value> {
        retry_connection(
            space_id,
            server_id,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn update_server_package(
        &self,
        _space_id: String,
        _server_id: String,
    ) -> anyhow::Result<Value> {
        // ponytail: Tauri update_server_package command lands in Phase 5
        Err(anyhow::anyhow!("Server package update not yet available"))
    }

    async fn logout_server(&self, space_id: String, server_id: String) -> anyhow::Result<Value> {
        logout_server(
            space_id,
            server_id,
            self.app_handle.state(),
            self.app_handle.state(),
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn respond_to_meta_tool_approval(
        &self,
        request_id: String,
        client_id: String,
        tool_name: String,
        decision: String,
    ) -> anyhow::Result<Value> {
        let approved = respond_to_meta_tool_approval(
            request_id,
            client_id,
            tool_name,
            decision,
            self.app_handle.state(),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "approved": approved }))
    }

    async fn revoke_meta_tool_grant(
        &self,
        client_id: String,
        tool_name: String,
    ) -> anyhow::Result<Value> {
        let revoked = revoke_meta_tool_grant(client_id, tool_name, self.app_handle.state())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "revoked": revoked }))
    }

    async fn update_oauth_client(
        &self,
        client_id: String,
        client_alias: Option<String>,
    ) -> anyhow::Result<Value> {
        let client = update_oauth_client(
            self.app_handle.state(),
            client_id,
            UpdateClientSettingsRequest { client_alias },
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!(client))
    }

    async fn delete_oauth_client(&self, client_id: String) -> anyhow::Result<Value> {
        delete_oauth_client(self.app_handle.state(), client_id)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn grant_oauth_client_feature_set(
        &self,
        client_id: String,
        space_id: String,
        feature_set_id: String,
    ) -> anyhow::Result<Value> {
        grant_oauth_client_feature_set(
            self.app_handle.clone(),
            self.app_handle.state(),
            client_id,
            space_id,
            feature_set_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn revoke_oauth_client_feature_set(
        &self,
        client_id: String,
        space_id: String,
        feature_set_id: String,
    ) -> anyhow::Result<Value> {
        revoke_oauth_client_feature_set(
            self.app_handle.clone(),
            self.app_handle.state(),
            client_id,
            space_id,
            feature_set_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!({ "ok": true }))
    }

    async fn gateway_state(&self) -> Option<Arc<RwLock<mcpmux_gateway::GatewayState>>> {
        let app_state = self.app_gateway_state.read().await;
        app_state.gateway_state.clone()
    }
}
