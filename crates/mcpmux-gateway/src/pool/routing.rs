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
use mcpmux_core::{FeatureType, LogLevel, LogSource, ServerFeature, ServerLog, ServerLogManager};
use rmcp::model::{CallToolRequestParams, CallToolResult, Content, Meta};
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
    pub structured_content: Option<Value>,
    pub meta: Option<Meta>,
}

impl ToolCallResult {
    fn from_mcp_result(result: CallToolResult) -> Self {
        Self {
            content: result
                .content
                .into_iter()
                .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
                .collect(),
            is_error: result.is_error.unwrap_or(false),
            structured_content: result.structured_content,
            meta: result.meta,
        }
    }

    pub(crate) fn into_mcp_result(self) -> CallToolResult {
        let content: Vec<Content> = self
            .content
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect();
        let mut result = if self.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };
        result.structured_content = self.structured_content;
        result.meta = self.meta;
        result
    }
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

        // Authorize AND route in one step by matching the requested qualified
        // name against the resolved feature set — using the SAME encoding the
        // list path uses (`ServerFeature::qualified_name`). This guarantees
        // "if it lists, it calls": the (server_id, tool_name) we route to come
        // straight from the matched feature, so there's no dependency on the
        // prefix-cache reverse lookup, which could be stale and surface a
        // listed tool as "not allowed by the current grants".
        let allowed_features = self
            .feature_service
            .resolve_feature_sets(&space_id_str, feature_set_ids)
            .await?;

        let feature =
            match_feature(&allowed_features, FeatureType::Tool, tool_name).map_err(|e| {
                warn!("[RoutingService] {}", e);
                e
            })?;

        let (server_id, actual_tool_name) = match feature {
            Some(f) => (f.server_id.clone(), f.feature_name.clone()),
            None => {
                let available = allowed_features
                    .iter()
                    .filter(|f| f.feature_type == FeatureType::Tool && f.is_available)
                    .count();
                warn!(
                    "[RoutingService] Tool '{}' not in the resolved feature set ({} tools available)",
                    tool_name, available
                );
                return Err(anyhow!(
                    "Tool '{}' is not allowed by the current grants",
                    tool_name
                ));
            }
        };

        info!(
            "[RoutingService] Tool '{}' ALLOWED → server={}, tool={}",
            tool_name, server_id, actual_tool_name
        );

        info!(
            "[RoutingService] Calling tool {} on server {}",
            actual_tool_name, server_id
        );

        // Log the tool call attempt. Persist only the argument KEY names, not
        // their values — tool arguments routinely carry secrets/PII, and this
        // log is written to plaintext `current.log`. Keys alone are enough to
        // debug routing without leaking payloads.
        let arg_keys: Vec<&str> = arguments
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        self.log(
            &space_id,
            &server_id,
            LogLevel::Info,
            format!("Calling tool: {}", actual_tool_name),
            Some(serde_json::json!({
                "tool": actual_tool_name,
                "argument_keys": arg_keys
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
                    let mut params = CallToolRequestParams::new(tool_name.to_string());
                    params.arguments = args.as_object().cloned();

                    // Wrap call_tool with timeout to prevent hanging
                    let res = tokio::time::timeout(TOOL_CALL_TIMEOUT, client.call_tool(params))
                        .await
                        .map_err(|_| anyhow!("Tool call timed out after {:?}", TOOL_CALL_TIMEOUT))?
                        .map_err(|e| anyhow!("MCP call failed: {}", e))?;

                    Ok(ToolCallResult::from_mcp_result(res))
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
                    if Self::result_is_auth_failure(&result.content, true) {
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
                    if Self::result_is_auth_failure(&result.content, false) {
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

    /// Whether a tool result is the server reporting an authentication failure
    /// (rather than tool output that happens to mention one).
    ///
    /// Some MCP servers (e.g., Atlassian) return auth errors as tool results
    /// rather than HTTP-level errors — `is_error: true` with a 401 message, or
    /// even a "successful" `{"code":401,"message":"Unauthorized"}`. The SDK may
    /// have already refreshed the token, but the server's internal session can
    /// be stale, and a fresh connection fixes it; the caller then reconnects and
    /// runs the call again. Because the retry repeats the call, only a result
    /// that is *itself* an auth error qualifies: a single short text item that
    /// is a structured 401 error, or, for `is_error` results, a short auth error
    /// message. Output that merely contains these words (an issue body, a web
    /// page, documentation) never triggers a second call.
    fn result_is_auth_failure(content: &[Value], is_error: bool) -> bool {
        const MAX_ERROR_TEXT: usize = 512;
        let [item] = content else {
            return false;
        };
        let Some(text) = item.get("text").and_then(|v| v.as_str()) else {
            return false;
        };
        let text = text.trim();
        if text.len() > MAX_ERROR_TEXT {
            return false;
        }
        if serde_json::from_str::<Value>(text).is_ok_and(|json| Self::json_is_auth_error(&json)) {
            return true;
        }
        // A failed call whose (short) error message names an auth failure, e.g.
        // "401 Unauthorized" or {"message":"Token expired"}.
        is_error && Self::is_auth_error(&text.to_lowercase())
    }

    /// A JSON error object carrying a 401 status/code, or an OAuth
    /// `invalid_token`/`unauthorized` error code.
    fn json_is_auth_error(json: &Value) -> bool {
        let Some(obj) = json.as_object() else {
            return false;
        };
        let is_401 = |v: &Value| v.as_u64() == Some(401) || v.as_str() == Some("401");
        let is_auth_code = |v: &Value| {
            v.as_str().is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "unauthorized" | "invalid_token" | "unauthenticated"
                )
            })
        };
        ["code", "status", "statusCode", "status_code"]
            .iter()
            .filter_map(|k| obj.get(*k))
            .any(|v| is_401(v) || is_auth_code(v))
            || obj
                .get("error")
                .is_some_and(|e| is_auth_code(e) || Self::json_is_auth_error(e))
    }
}

/// The one available feature of `feature_type` answering to `qualified_name`.
///
/// Features on two servers must never share a qualified name (prefixes are
/// unique, but `_` is legal inside tool names, so `gh` + `evil_delete` and
/// `gh_evil` + `delete` still collide; resource URIs aren't prefixed at all).
/// If they do, the request is refused rather than sent — with its arguments —
/// to whichever server happens to come first.
pub(crate) fn match_feature<'a>(
    features: &'a [ServerFeature],
    feature_type: FeatureType,
    qualified_name: &str,
) -> Result<Option<&'a ServerFeature>> {
    let mut matches = features.iter().filter(|f| {
        f.feature_type == feature_type && f.is_available && f.qualified_name() == qualified_name
    });
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if let Some(other) = matches.find(|f| f.server_id != first.server_id) {
        return Err(anyhow!(
            "'{}' matches features on more than one server ({} and {}); \
             rename or disable one of them",
            qualified_name,
            first.server_id,
            other.server_id
        ));
    }
    Ok(Some(first))
}

#[cfg(test)]
mod auth_failure_tests {
    use super::RoutingService;
    use serde_json::{json, Value};

    #[test]
    fn tools_sharing_a_name_across_servers_are_refused() {
        use mcpmux_core::{FeatureType, ServerFeature};
        let tool = |server: &str, alias: &str, name: &str| {
            let mut f = ServerFeature::tool("space", server, name);
            f.server_alias = Some(alias.to_string());
            f.is_available = true;
            f
        };
        let find = |features: &[ServerFeature], name: &str| {
            super::match_feature(features, FeatureType::Tool, name)
                .map(|f| f.map(|f| f.server_id.clone()))
        };
        let one = vec![tool("server-a", "github", "create_issue")];
        assert_eq!(
            find(&one, "github_create_issue").unwrap().as_deref(),
            Some("server-a")
        );

        // Same alias on two servers.
        let clash = vec![
            tool("server-a", "github", "create_issue"),
            tool("server-b", "github", "create_issue"),
        ];
        assert!(find(&clash, "github_create_issue").is_err());
        assert!(find(&clash, "github_other").unwrap().is_none());

        // Distinct prefixes whose qualified names still coincide.
        let underscore = vec![
            tool("server-a", "gh", "evil_delete"),
            tool("server-b", "gh_evil", "delete"),
        ];
        assert!(find(&underscore, "gh_evil_delete").is_err());
    }

    #[test]
    fn resources_with_the_same_uri_on_two_servers_are_refused() {
        use mcpmux_core::{FeatureType, ServerFeature};
        let resource = |server: &str| {
            let mut f = ServerFeature::resource("space", server, "memo://notes");
            f.is_available = true;
            f
        };
        let one = vec![resource("server-a")];
        assert!(
            super::match_feature(&one, FeatureType::Resource, "memo://notes")
                .unwrap()
                .is_some()
        );
        let clash = vec![resource("server-a"), resource("server-b")];
        assert!(super::match_feature(&clash, FeatureType::Resource, "memo://notes").is_err());
    }

    fn text(t: &str) -> Vec<Value> {
        vec![json!({ "type": "text", "text": t })]
    }

    #[test]
    fn structured_401_results_are_auth_failures() {
        for t in [
            r#"{"code":401,"message":"Unauthorized"}"#,
            r#"{"status":"401"}"#,
            r#"{"error":"invalid_token"}"#,
            r#"{"error":{"code":401,"message":"token expired"}}"#,
        ] {
            assert!(
                RoutingService::result_is_auth_failure(&text(t), false),
                "{t}"
            );
            assert!(
                RoutingService::result_is_auth_failure(&text(t), true),
                "{t}"
            );
        }
    }

    #[test]
    fn short_auth_error_messages_count_only_when_the_call_failed() {
        for msg in [
            text("401 Unauthorized: token expired"),
            text(r#"{"message":"401 Unauthorized"}"#),
        ] {
            assert!(RoutingService::result_is_auth_failure(&msg, true));
            assert!(!RoutingService::result_is_auth_failure(&msg, false));
        }
    }

    #[test]
    fn output_that_mentions_auth_words_is_not_retried() {
        // A created issue whose body happens to say "401 Unauthorized".
        let created =
            text(r#"{"id":4012,"title":"Login fails","body":"API returns 401 Unauthorized"}"#);
        assert!(!RoutingService::result_is_auth_failure(&created, false));
        // Documentation text.
        let docs = text("To call the API, send your access token in the Authorization header.");
        assert!(!RoutingService::result_is_auth_failure(&docs, false));
        // Long output is never treated as an error message.
        let long = text(&format!("unauthorized {}", "x".repeat(600)));
        assert!(!RoutingService::result_is_auth_failure(&long, true));
        // Several content items are tool output, not an error.
        let mut many = text(r#"{"code":401}"#);
        many.extend(text("more"));
        assert!(!RoutingService::result_is_auth_failure(&many, true));
    }
}

#[cfg(test)]
mod tests {
    use super::ToolCallResult;
    use rmcp::model::{CallToolResult, Content, Meta};
    use serde_json::json;

    #[test]
    fn tool_result_round_trip_preserves_structured_content_and_meta() {
        let structured = json!({ "matches": [{ "message": "found" }] });
        let mut meta = Meta::new();
        meta.0.insert("traceId".to_string(), json!("trace-123"));

        let mut upstream = CallToolResult::structured(structured.clone());
        upstream.content = vec![Content::text("search completed")];
        upstream.meta = Some(meta.clone());

        let routed = ToolCallResult::from_mcp_result(upstream);
        let forwarded = routed.into_mcp_result();

        assert_eq!(forwarded.content, vec![Content::text("search completed")]);
        assert_eq!(forwarded.structured_content, Some(structured));
        assert_eq!(forwarded.meta, Some(meta));
        assert_eq!(forwarded.is_error, Some(false));
    }
}
