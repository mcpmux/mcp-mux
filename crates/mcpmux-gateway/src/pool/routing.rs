//! Routing Service - Request dispatch and permission filtering
//!
//! RoutingService handles:
//! - Listing tools/prompts/resources filtered by client grants
//! - Dispatching tool calls to the correct backend server
//! - Handling 401 errors with automatic token refresh and retry
//!
//! Uses FeatureService for permission resolution and TokenService for refresh.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use mcpmux_core::{FeatureType, LogLevel, LogSource, ServerLog, ServerLogManager};
use rmcp::model::CallToolRequestParams;
use serde_json::Value;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::connection::ConnectionResult;
use super::features::FeatureService;
use super::service::PoolService;

/// A tool as returned by the routing service
#[derive(Debug, Clone)]
pub struct RoutedTool {
    pub name: String,
    pub server_id: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
}

/// A prompt as returned by the routing service
#[derive(Debug, Clone)]
pub struct RoutedPrompt {
    pub name: String,
    pub server_id: String,
    pub description: Option<String>,
}

/// A resource as returned by the routing service
#[derive(Debug, Clone)]
pub struct RoutedResource {
    pub uri: String,
    pub server_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Result of a tool call
#[derive(Debug)]
pub struct ToolCallResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

/// Default timeout for MCP tool calls (60 seconds)
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// RoutingService dispatches requests to backend MCP servers
pub struct RoutingService {
    feature_service: Arc<FeatureService>,
    pool_service: Arc<PoolService>,
    log_manager: Arc<ServerLogManager>,
}

impl RoutingService {
    pub fn new(
        feature_service: Arc<FeatureService>,
        pool_service: Arc<PoolService>,
        log_manager: Arc<ServerLogManager>,
    ) -> Self {
        Self {
            feature_service,
            pool_service,
            log_manager,
        }
    }

    /// List tools available to a client based on their grants
    ///
    /// Returns tools from all connected servers, filtered by the client's feature set grants.
    pub async fn list_tools(
        &self,
        space_id: Uuid,
        feature_set_ids: &[String],
    ) -> Result<Vec<RoutedTool>> {
        let space_id_str = space_id.to_string();

        // Resolve feature sets to allowed features
        let allowed_features = self
            .feature_service
            .get_tools_for_grants(&space_id_str, feature_set_ids)
            .await?;

        // Filter to just tools
        let tools: Vec<RoutedTool> = allowed_features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Tool && f.is_available)
            .map(|f| RoutedTool {
                name: f.qualified_name(), // server_id/tool_name for disambiguation
                server_id: f.server_id.clone(),
                description: f.description.clone(),
                input_schema: None, // Raw JSON is used in handlers now
            })
            .collect();

        debug!(
            "[RoutingService] Listed {} tools for grants {:?}",
            tools.len(),
            feature_set_ids
        );

        Ok(tools)
    }

    /// List prompts available to a client based on their grants
    pub async fn list_prompts(
        &self,
        space_id: Uuid,
        feature_set_ids: &[String],
    ) -> Result<Vec<RoutedPrompt>> {
        let space_id_str = space_id.to_string();

        let allowed_features = self
            .feature_service
            .get_prompts_for_grants(&space_id_str, feature_set_ids)
            .await?;

        let prompts: Vec<RoutedPrompt> = allowed_features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Prompt && f.is_available)
            .map(|f| RoutedPrompt {
                name: f.qualified_name(),
                server_id: f.server_id.clone(),
                description: f.description.clone(),
            })
            .collect();

        debug!(
            "[RoutingService] Listed {} prompts for grants {:?}",
            prompts.len(),
            feature_set_ids
        );

        Ok(prompts)
    }

    /// List resources available to a client based on their grants
    pub async fn list_resources(
        &self,
        space_id: Uuid,
        feature_set_ids: &[String],
    ) -> Result<Vec<RoutedResource>> {
        let space_id_str = space_id.to_string();

        let allowed_features = self
            .feature_service
            .get_resources_for_grants(&space_id_str, feature_set_ids)
            .await?;

        let resources: Vec<RoutedResource> = allowed_features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Resource && f.is_available)
            .map(|f| RoutedResource {
                uri: f.qualified_name(), // Use qualified name (prefix.resource_name)
                server_id: f.server_id.clone(),
                name: f.display_name.clone(),
                description: f.description.clone(),
            })
            .collect();

        debug!(
            "[RoutingService] Listed {} resources for grants {:?}",
            resources.len(),
            feature_set_ids
        );

        Ok(resources)
    }

    /// Call a tool on a backend server
    pub async fn call_tool(
        &self,
        space_id: Uuid,
        feature_set_ids: &[String],
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolCallResult> {
        let space_id_str = space_id.to_string();

        // 1. Find the server that provides this tool
        let (server_id, actual_tool_name) = self
            .feature_service
            .find_server_for_qualified_tool(&space_id_str, tool_name)
            .await?
            .ok_or_else(|| anyhow!("Tool '{}' not found", tool_name))?;

        // 2. Check if the tool is allowed by grants
        let allowed_features = self
            .feature_service
            .resolve_feature_sets(&space_id_str, feature_set_ids)
            .await?;

        info!(
            "[RoutingService] Checking authorization for tool '{}' (server: {}, actual_name: {})",
            tool_name, server_id, actual_tool_name
        );
        info!(
            "[RoutingService] Feature sets to check: {:?}",
            feature_set_ids
        );
        info!(
            "[RoutingService] Total allowed features: {}",
            allowed_features.len()
        );

        // Log all tool features for debugging
        let tool_features: Vec<_> = allowed_features
            .iter()
            .filter(|f| f.feature_type == FeatureType::Tool)
            .map(|f| format!("{}::{}", f.server_id, f.feature_name))
            .collect();
        info!("[RoutingService] Allowed tools: {:?}", tool_features);

        let is_allowed = allowed_features.iter().any(|f| {
            f.feature_type == FeatureType::Tool
                && f.server_id == server_id
                && f.feature_name == actual_tool_name
                && f.is_available
        });

        if !is_allowed {
            warn!(
                "[RoutingService] Tool '{}' NOT allowed. Looking for server_id='{}', feature_name='{}', is_available=true",
                tool_name, server_id, actual_tool_name
            );
            return Err(anyhow!(
                "Tool '{}' is not allowed by the current grants",
                tool_name
            ));
        }

        info!("[RoutingService] Tool '{}' is ALLOWED", tool_name);

        info!(
            "[RoutingService] Calling tool {} on server {}",
            actual_tool_name, server_id
        );

        // Log the tool call attempt
        self.log(
            &space_id,
            &server_id,
            LogLevel::Info,
            format!("Calling tool: {}", actual_tool_name),
            Some(serde_json::json!({
                "tool": actual_tool_name,
                "arguments": arguments
            })),
        )
        .await;

        // Define the call operation
        // Function to execute the call on the instance
        async fn execute_call(
            pool: Arc<PoolService>,
            space_id: Uuid,
            server_id: String,
            tool_name: String,
            args: Value,
        ) -> Result<ToolCallResult> {
            let instance = pool
                .get_instance(space_id, &server_id)
                .ok_or_else(|| anyhow!("Server not connected: {}", server_id))?;

            // We need to get the service handle (peer) which is cloneable
            // But we don't have direct access to it via with_client easily because with_client
            // passes &McpClient (RunningService).
            // We can assume RunningService is not cloneable but its peer() returns a Service handle which is.
            // Let's use with_client to get the handle out.
            let client_handle = instance.with_client(|client| client.peer().clone());

            match client_handle {
                Some(client) => {
                    let params = CallToolRequestParams {
                        name: tool_name.into(),
                        arguments: args.as_object().cloned(),
                        task: None,
                        meta: None,
                    };

                    // Wrap call_tool with timeout to prevent hanging
                    let res = tokio::time::timeout(TOOL_CALL_TIMEOUT, client.call_tool(params))
                        .await
                        .map_err(|_| anyhow!("Tool call timed out after {:?}", TOOL_CALL_TIMEOUT))?
                        .map_err(|e| anyhow!("MCP call failed: {}", e))?;

                    let content: Vec<Value> = res
                        .content
                        .into_iter()
                        .map(|c| serde_json::to_value(c).unwrap_or(Value::Null))
                        .collect();

                    Ok(ToolCallResult {
                        content,
                        is_error: res.is_error.unwrap_or(false),
                    })
                }
                None => Err(anyhow!("Server instance has no active client")),
            }
        }

        // 3. Dispatch the call with retry logic
        // NOTE: Preemptive token refresh is no longer needed here.
        // RMCP's AuthClient with DatabaseCredentialStore handles token refresh
        // automatically on every HTTP request when needed.
        info!(
            "[RoutingService] Executing tool call: {} on {} (timeout: {:?})",
            actual_tool_name, server_id, TOOL_CALL_TIMEOUT
        );

        let call_start = std::time::Instant::now();
        match execute_call(
            self.pool_service.clone(),
            space_id,
            server_id.clone(),
            actual_tool_name.clone(),
            arguments.clone(),
        )
        .await
        {
            Ok(result) => {
                let duration = call_start.elapsed();
                if result.is_error {
                    // Check if this is an auth error embedded in the tool result.
                    // Some servers (e.g., Atlassian) return 401 as tool results rather than
                    // HTTP errors. The SDK refreshes the token successfully, but the server's
                    // internal session may be stale. A fresh MCP connection fixes this.
                    if Self::content_has_auth_error(&result.content) {
                        warn!(
                            "[RoutingService] Auth error in tool result for {}/{}, attempting auto-reconnect",
                            server_id, actual_tool_name
                        );
                        self.log(
                            &space_id,
                            &server_id,
                            LogLevel::Warn,
                            format!(
                                "Auth error in tool result for '{}' - auto-reconnecting",
                                actual_tool_name
                            ),
                            Some(serde_json::json!({ "result": result.content, "duration_ms": duration.as_millis() })),
                        )
                        .await;

                        match self
                            .pool_service
                            .reconnect_instance(space_id, &server_id)
                            .await
                        {
                            ConnectionResult::Connected { .. } => {
                                info!(
                                    "[RoutingService] Reconnected {}, retrying tool call: {}",
                                    server_id, actual_tool_name
                                );

                                let retry_start = std::time::Instant::now();
                                match execute_call(
                                    self.pool_service.clone(),
                                    space_id,
                                    server_id.clone(),
                                    actual_tool_name.clone(),
                                    arguments.clone(),
                                )
                                .await
                                {
                                    Ok(retry_result) => {
                                        let retry_duration = retry_start.elapsed();
                                        if retry_result.is_error {
                                            warn!(
                                                "[RoutingService] Tool retry still has error: {} (duration: {:?})",
                                                actual_tool_name, retry_duration
                                            );
                                        } else {
                                            info!(
                                                "[RoutingService] Tool retry succeeded after reconnect: {} (duration: {:?})",
                                                actual_tool_name, retry_duration
                                            );
                                        }
                                        self.log(
                                            &space_id,
                                            &server_id,
                                            LogLevel::Info,
                                            format!(
                                                "Tool '{}' retried after auto-reconnect (is_error={})",
                                                actual_tool_name, retry_result.is_error
                                            ),
                                            Some(serde_json::json!({ "retry_duration_ms": retry_duration.as_millis() })),
                                        )
                                        .await;
                                        Ok(retry_result)
                                    }
                                    Err(retry_err) => {
                                        warn!(
                                            "[RoutingService] Tool retry transport error: {} - {}",
                                            actual_tool_name, retry_err
                                        );
                                        self.log(
                                            &space_id,
                                            &server_id,
                                            LogLevel::Error,
                                            format!(
                                                "Tool '{}' still failing after reconnect",
                                                actual_tool_name
                                            ),
                                            Some(serde_json::json!({ "error": retry_err.to_string() })),
                                        )
                                        .await;
                                        // Return original tool result since it has the error details
                                        Ok(result)
                                    }
                                }
                            }
                            other => {
                                warn!(
                                    "[RoutingService] Auto-reconnect failed for {}: {:?}",
                                    server_id, other
                                );
                                self.log(
                                    &space_id,
                                    &server_id,
                                    LogLevel::Error,
                                    format!(
                                        "Auto-reconnect failed for tool '{}' - manual reconnection required",
                                        actual_tool_name
                                    ),
                                    Some(serde_json::json!({ "reconnect_result": format!("{:?}", other) })),
                                )
                                .await;
                                Ok(result)
                            }
                        }
                    } else {
                        warn!(
                            "[RoutingService] Tool execution error: {} (duration: {:?})",
                            actual_tool_name, duration
                        );
                        self.log(
                            &space_id,
                            &server_id,
                            LogLevel::Error,
                            format!("Tool execution error: {}", actual_tool_name),
                            Some(serde_json::json!({ "result": result.content, "duration_ms": duration.as_millis() }))
                        ).await;
                        Ok(result)
                    }
                } else {
                    // Even on "success" (is_error=false), some servers (e.g., Atlassian)
                    // return auth errors as plain text content like {"code":401,"message":"Unauthorized"}.
                    // Detect these and auto-reconnect + retry.
                    if Self::content_has_auth_error(&result.content) {
                        warn!(
                            "[RoutingService] Auth error in successful tool result for {}/{}, attempting auto-reconnect",
                            server_id, actual_tool_name
                        );
                        self.log(
                            &space_id,
                            &server_id,
                            LogLevel::Warn,
                            format!(
                                "Auth error in tool result for '{}' (is_error=false) - auto-reconnecting",
                                actual_tool_name
                            ),
                            Some(serde_json::json!({ "result": result.content, "duration_ms": duration.as_millis() })),
                        )
                        .await;

                        match self
                            .pool_service
                            .reconnect_instance(space_id, &server_id)
                            .await
                        {
                            ConnectionResult::Connected { .. } => {
                                info!(
                                    "[RoutingService] Reconnected {}, retrying tool call: {}",
                                    server_id, actual_tool_name
                                );

                                let retry_start = std::time::Instant::now();
                                match execute_call(
                                    self.pool_service.clone(),
                                    space_id,
                                    server_id.clone(),
                                    actual_tool_name.clone(),
                                    arguments.clone(),
                                )
                                .await
                                {
                                    Ok(retry_result) => {
                                        let retry_duration = retry_start.elapsed();
                                        info!(
                                            "[RoutingService] Tool retry result: {} (is_error={}, duration: {:?})",
                                            actual_tool_name, retry_result.is_error, retry_duration
                                        );
                                        self.log(
                                            &space_id,
                                            &server_id,
                                            LogLevel::Info,
                                            format!(
                                                "Tool '{}' retried after auto-reconnect (is_error={})",
                                                actual_tool_name, retry_result.is_error
                                            ),
                                            Some(serde_json::json!({ "retry_duration_ms": retry_duration.as_millis() })),
                                        )
                                        .await;
                                        Ok(retry_result)
                                    }
                                    Err(retry_err) => {
                                        warn!(
                                            "[RoutingService] Tool retry transport error: {} - {}",
                                            actual_tool_name, retry_err
                                        );
                                        Ok(result)
                                    }
                                }
                            }
                            other => {
                                warn!(
                                    "[RoutingService] Auto-reconnect failed for {}: {:?}",
                                    server_id, other
                                );
                                Ok(result)
                            }
                        }
                    } else {
                        info!(
                            "[RoutingService] Tool executed successfully: {} (duration: {:?})",
                            actual_tool_name, duration
                        );
                        self.log(
                            &space_id,
                            &server_id,
                            LogLevel::Info,
                            format!("Tool executed successfully: {}", actual_tool_name),
                            Some(serde_json::json!({ "duration_ms": duration.as_millis() })),
                        )
                        .await;
                        Ok(result)
                    }
                }
            }
            Err(e) => {
                let duration = call_start.elapsed();
                let err_str = e.to_string().to_lowercase();

                warn!(
                    "[RoutingService] Tool call failed: {} on {} - {} (duration: {:?})",
                    actual_tool_name, server_id, e, duration
                );

                let is_auth = Self::is_auth_error(&err_str);

                if is_auth {
                    // Auth error detected - attempt auto-reconnect and retry once.
                    // This handles the case where RMCP's AuthClient failed to refresh
                    // the token (e.g., stale in-memory state after idle).
                    // Creating a fresh connection loads latest tokens from the database.
                    warn!(
                        "[RoutingService] Auth error for {}/{}, attempting auto-reconnect",
                        server_id, actual_tool_name
                    );
                    self.log(
                        &space_id,
                        &server_id,
                        LogLevel::Warn,
                        format!(
                            "Auth error on tool '{}' - auto-reconnecting to refresh credentials",
                            actual_tool_name
                        ),
                        Some(serde_json::json!({ "error": e.to_string(), "duration_ms": duration.as_millis() })),
                    )
                    .await;

                    match self
                        .pool_service
                        .reconnect_instance(space_id, &server_id)
                        .await
                    {
                        ConnectionResult::Connected { .. } => {
                            info!(
                                "[RoutingService] Reconnected {}, retrying tool call: {}",
                                server_id, actual_tool_name
                            );

                            // Retry the call once with the fresh connection
                            let retry_start = std::time::Instant::now();
                            match execute_call(
                                self.pool_service.clone(),
                                space_id,
                                server_id.clone(),
                                actual_tool_name.clone(),
                                arguments.clone(),
                            )
                            .await
                            {
                                Ok(result) => {
                                    let retry_duration = retry_start.elapsed();
                                    info!(
                                        "[RoutingService] Tool retry succeeded: {} (duration: {:?})",
                                        actual_tool_name, retry_duration
                                    );
                                    self.log(
                                        &space_id,
                                        &server_id,
                                        LogLevel::Info,
                                        format!(
                                            "Tool '{}' succeeded after auto-reconnect",
                                            actual_tool_name
                                        ),
                                        Some(serde_json::json!({ "retry_duration_ms": retry_duration.as_millis() })),
                                    )
                                    .await;
                                    Ok(result)
                                }
                                Err(retry_err) => {
                                    warn!(
                                        "[RoutingService] Tool retry also failed: {} - {}",
                                        actual_tool_name, retry_err
                                    );
                                    self.log(
                                        &space_id,
                                        &server_id,
                                        LogLevel::Error,
                                        format!(
                                            "Tool '{}' still failing after reconnect - manual reconnection required",
                                            actual_tool_name
                                        ),
                                        Some(serde_json::json!({ "error": retry_err.to_string() })),
                                    )
                                    .await;
                                    Err(anyhow!(
                                        "Server '{}' auth error persists after auto-reconnect. Please disconnect and connect again. Error: {}",
                                        server_id,
                                        retry_err
                                    ))
                                }
                            }
                        }
                        other => {
                            warn!(
                                "[RoutingService] Auto-reconnect failed for {}: {:?}",
                                server_id, other
                            );
                            self.log(
                                &space_id,
                                &server_id,
                                LogLevel::Error,
                                format!(
                                    "Auto-reconnect failed for tool '{}' - manual reconnection required",
                                    actual_tool_name
                                ),
                                Some(serde_json::json!({ "reconnect_result": format!("{:?}", other) })),
                            )
                            .await;
                            Err(anyhow!(
                                "Server '{}' requires reconnection. Auto-reconnect failed. Please disconnect and connect again.",
                                server_id
                            ))
                        }
                    }
                } else {
                    // Not an auth error, return original error
                    self.log(
                        &space_id,
                        &server_id,
                        LogLevel::Error,
                        format!("Tool call failed: {}", e),
                        Some(serde_json::json!({ "error": e.to_string(), "duration_ms": duration.as_millis() })),
                    )
                    .await;
                    Err(e)
                }
            }
        }
    }

    /// Log an event
    async fn log(
        &self,
        space_id: &Uuid,
        server_id: &str,
        level: LogLevel,
        message: String,
        metadata: Option<Value>,
    ) {
        let mut log = ServerLog::new(level, LogSource::App, message);
        if let Some(meta) = metadata {
            log = log.with_metadata(meta);
        }

        if let Err(e) = self
            .log_manager
            .append(&space_id.to_string(), server_id, log)
            .await
        {
            warn!("[RoutingService] Failed to log event: {}", e);
        }
    }

    /// Check if an error string indicates authentication is needed
    fn is_auth_error(error_str: &str) -> bool {
        let indicators = [
            "401",
            "unauthorized",
            "invalid_token",
            "token expired",
            "access token",
        ];
        indicators.iter().any(|s| error_str.contains(s))
    }

    /// Check if tool result content contains authentication error indicators.
    ///
    /// Some MCP servers (e.g., Atlassian) return auth errors as tool results
    /// (`is_error: true` with 401 in the text) rather than HTTP-level errors.
    /// The SDK may have already refreshed the token, but the server's internal
    /// session can be stale. A fresh connection (reconnect) fixes this.
    fn content_has_auth_error(content: &[Value]) -> bool {
        for item in content {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                let lower = text.to_lowercase();
                if Self::is_auth_error(&lower) {
                    return true;
                }
            }
        }
        false
    }
}
