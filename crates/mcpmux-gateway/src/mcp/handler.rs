//! McpMux Gateway MCP Handler
//!
//! Implements the MCP ServerHandler trait to expose aggregated tools, prompts,
//! and resources from multiple backend MCP servers.

use anyhow::Result;
use rmcp::{
    model::*,
    service::{NotificationContext, RequestContext},
    ErrorData as McpError, RoleServer, ServerHandler,
};
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::context::{extract_oauth_context, OAuthContext};
use crate::consumers::MCPNotifier;
use crate::server::ServiceContainer;

/// McpMux Gateway Handler
///
/// Routes MCP requests to appropriate backend services:
/// - Authorization via FeatureService (grants, spaces)
/// - Tool/prompt/resource routing via PoolService
/// - Server management via ServerManager
#[derive(Clone)]
pub struct McpMuxGatewayHandler {
    pub services: Arc<ServiceContainer>,
    pub notification_bridge: Arc<MCPNotifier>,
}

impl McpMuxGatewayHandler {
    pub fn new(services: Arc<ServiceContainer>, notification_bridge: Arc<MCPNotifier>) -> Self {
        Self {
            services,
            notification_bridge,
        }
    }

    /// Extract OAuth context from request extensions, with session fallback
    ///
    /// Tries to get OAuth context from headers first (injected by middleware).
    /// If headers are missing (e.g., client reconnected without auth), falls back
    /// to session metadata stored during initialization.
    fn get_oauth_context(&self, extensions: &Extensions) -> Result<OAuthContext> {
        // Try to get from headers first (preferred path)
        match extract_oauth_context(extensions) {
            Ok(ctx) => Ok(ctx),
            Err(e) => {
                // OAuth headers missing - client may need to re-authenticate
                // Note: This path should not be reachable since oauth_middleware blocks
                // requests without valid Authorization header
                warn!("OAuth headers missing: {}", e);

                Err(anyhow::anyhow!(
                    "OAuth context not available: headers missing. \
                     This should not happen - oauth_middleware should have blocked this request."
                ))
            }
        }
    }

    /// Negotiate protocol version between client and server.
    /// Returns the highest version both parties support.
    fn negotiate_protocol_version(&self, client_version_str: &str) -> ProtocolVersion {
        let our_max_version = ProtocolVersion::LATEST;
        let our_max_str = our_max_version.to_string();

        if client_version_str > our_max_str.as_str() {
            // Client is newer - respond with our maximum
            debug!(
                client_version = %client_version_str,
                our_max = %our_max_str,
                "Client uses newer protocol, negotiating down"
            );
            our_max_version
        } else {
            // Client version is compatible - use their version
            // Deserialize client version into ProtocolVersion
            serde_json::from_value(serde_json::Value::String(client_version_str.to_string()))
                .unwrap_or(our_max_version)
        }
    }

    /// Build InitializeResult with negotiated protocol version
    fn build_initialize_result(&self, protocol_version: ProtocolVersion) -> InitializeResult {
        InitializeResult {
            protocol_version,
            capabilities: self.get_info().capabilities,
            server_info: self.get_info().server_info,
            instructions: self.get_info().instructions,
        }
    }
}

impl ServerHandler for McpMuxGatewayHandler {
    fn get_info(&self) -> ServerInfo {
        use rmcp::model::{PromptsCapability, ResourcesCapability, ToolsCapability};

        // Note: get_info is called frequently, no logging needed

        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools_with(ToolsCapability {
                    list_changed: Some(true),
                })
                .enable_prompts_with(PromptsCapability {
                    list_changed: Some(true),
                })
                .enable_resources_with(ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(true),
                })
                .build(),
            server_info: Implementation {
                name: "mcpmux-gateway".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(
                "McpMux aggregates multiple MCP servers. Use tools/prompts/resources \
                 from your authorized backend servers."
                    .to_string(),
            ),
        }
    }

    async fn initialize(
        &self,
        params: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Negotiate protocol version
        let client_version_str = params.protocol_version.to_string();
        let negotiated_version = self.negotiate_protocol_version(&client_version_str);

        // Client initialization - log once
        debug!(
            client_id = %oauth_ctx.client_id,
            space_id = %oauth_ctx.space_id,
            protocol_version = %negotiated_version,
            "Client initializing"
        );

        Ok(self.build_initialize_result(negotiated_version))
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let oauth_ctx = match self.get_oauth_context(&context.extensions) {
            Ok(ctx) => ctx,
            Err(e) => {
                warn!("Failed to extract OAuth context on_initialized: {}", e);
                return;
            }
        };

        // Register peer with MCPNotifier for list_changed notification delivery
        let peer = std::sync::Arc::new(context.peer);
        self.notification_bridge
            .register_peer(oauth_ctx.client_id.clone(), peer);

        // Mark the client stream as active immediately - RMCP's session transport
        // handles SSE streaming and message caching internally
        self.notification_bridge
            .mark_client_stream_active(&oauth_ctx.client_id);

        // Pre-populate feature hashes to prevent spurious first notifications
        self.notification_bridge
            .prime_hashes_for_space(oauth_ctx.space_id)
            .await;

        info!(
            client_id = %oauth_ctx.client_id,
            space_id = %oauth_ctx.space_id,
            "Client initialized - peer registered for notifications"
        );
    }

    async fn list_tools(
        &self,
        _params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Get client's grants
        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get grants: {}", e), None))?;

        // Get tools via FeatureService
        let tools = self
            .services
            .pool_services
            .feature_service
            .get_tools_for_grants(&oauth_ctx.space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get tools: {}", e), None))?;

        // Convert to MCP Tool types with qualified names (prefix.tool_name)
        let mcp_tools: Vec<Tool> = tools
            .iter()
            .filter_map(|f| {
                f.raw_json.as_ref().and_then(|json| {
                    let mut tool: Tool = serde_json::from_value(json.clone()).ok()?;
                    // Replace name with qualified name (prefix.tool_name)
                    tool.name = f.qualified_name().into();
                    Some(tool)
                })
            })
            .collect();

        // Log tool names at DEBUG level for visibility
        let tool_names: Vec<String> = mcp_tools.iter().map(|t| t.name.to_string()).collect();
        debug!(
            count = mcp_tools.len(),
            tools = ?tool_names,
            "list_tools"
        );

        Ok(ListToolsResult::with_all_items(mcp_tools))
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Tool calls are important - log at INFO
        info!(
            tool = %params.name,
            client = %&oauth_ctx.client_id[..oauth_ctx.client_id.len().min(12)],
            "call_tool"
        );

        // Get client's feature set grants for authorization
        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get grants: {}", e), None))?;

        // Call tool via routing service (handles auth and routing)
        let tool_result = self
            .services
            .pool_services
            .routing_service
            .call_tool(
                oauth_ctx.space_id,
                &feature_set_ids,
                &params.name,
                serde_json::to_value(params.arguments.unwrap_or_default()).unwrap_or_default(),
            )
            .await
            .map_err(|e| McpError::internal_error(format!("Tool call failed: {}", e), None))?;

        // Convert ToolCallResult to MCP CallToolResult
        let content: Vec<Content> = tool_result
            .content
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        // Log result summary - show content types and approximate sizes
        let content_summary: Vec<String> = content
            .iter()
            .map(|c| {
                // Content is Annotated<RawContent>, serialize to inspect type
                if let Ok(json) = serde_json::to_value(c) {
                    let content_type = json
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown");
                    match content_type {
                        "text" => {
                            let len = json
                                .get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.len())
                                .unwrap_or(0);
                            format!("text({}c)", len)
                        }
                        "image" => {
                            let mime = json.get("mimeType").and_then(|m| m.as_str()).unwrap_or("?");
                            format!("image({})", mime)
                        }
                        "resource" => {
                            let uri = json
                                .get("resource")
                                .and_then(|r| r.get("uri"))
                                .and_then(|u| u.as_str())
                                .unwrap_or("?");
                            format!("resource({})", uri)
                        }
                        _ => content_type.to_string(),
                    }
                } else {
                    "?".to_string()
                }
            })
            .collect();
        debug!(
            tool = %params.name,
            is_error = tool_result.is_error,
            content = ?content_summary,
            "call_tool result"
        );

        let result = CallToolResult {
            content,
            structured_content: None,
            is_error: Some(tool_result.is_error),
            meta: None,
        };

        Ok(result)
    }

    async fn list_prompts(
        &self,
        _params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get grants: {}", e), None))?;

        let prompts = self
            .services
            .pool_services
            .feature_service
            .get_prompts_for_grants(&oauth_ctx.space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get prompts: {}", e), None))?;

        // Convert to MCP Prompt types with qualified names (prefix.prompt_name)
        let mcp_prompts: Vec<Prompt> = prompts
            .iter()
            .filter_map(|f| {
                f.raw_json.as_ref().and_then(|json| {
                    let mut prompt: Prompt = serde_json::from_value(json.clone()).ok()?;
                    // Replace name with qualified name (prefix.prompt_name)
                    prompt.name = f.qualified_name();
                    Some(prompt)
                })
            })
            .collect();

        // Log prompt names at DEBUG level
        let prompt_names: Vec<String> = mcp_prompts.iter().map(|p| p.name.to_string()).collect();
        debug!(
            count = mcp_prompts.len(),
            prompts = ?prompt_names,
            "list_prompts"
        );

        Ok(ListPromptsResult::with_all_items(mcp_prompts))
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let (server_id, prompt_name) = self
            .services
            .pool_services
            .feature_service
            .parse_qualified_prompt_name(&oauth_ctx.space_id.to_string(), &params.name)
            .await
            .map_err(|e| McpError::invalid_params(format!("Invalid prompt name: {}", e), None))?;

        // Verify authorization
        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get grants: {}", e), None))?;

        let authorized_prompts = self
            .services
            .pool_services
            .feature_service
            .get_prompts_for_grants(&oauth_ctx.space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to verify authorization: {}", e), None)
            })?;

        let is_authorized = authorized_prompts
            .iter()
            .any(|p| p.server_id == server_id && p.feature_name == prompt_name && p.is_available);

        if !is_authorized {
            return Err(McpError::invalid_params(
                format!("Prompt '{}' not authorized", params.name),
                None,
            ));
        }

        let result_value = self
            .services
            .pool_services
            .pool_service
            .get_prompt(
                oauth_ctx.space_id,
                &server_id,
                &prompt_name,
                params.arguments,
            )
            .await
            .map_err(|e| McpError::internal_error(format!("Get prompt failed: {}", e), None))?;

        // Deserialize the Value into GetPromptResult
        let result: GetPromptResult = serde_json::from_value(result_value).map_err(|e| {
            McpError::internal_error(format!("Failed to parse prompt result: {}", e), None)
        })?;

        Ok(result)
    }

    async fn list_resources(
        &self,
        _params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get grants: {}", e), None))?;

        let resources = self
            .services
            .pool_services
            .feature_service
            .get_resources_for_grants(&oauth_ctx.space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to get resources: {}", e), None)
            })?;

        let mcp_resources: Vec<Resource> = resources
            .iter()
            .filter_map(|f| {
                f.raw_json
                    .as_ref()
                    .and_then(|json| serde_json::from_value(json.clone()).ok())
            })
            .collect();

        // Log resource URIs at DEBUG level
        let resource_uris: Vec<String> = mcp_resources.iter().map(|r| r.uri.to_string()).collect();
        debug!(
            count = mcp_resources.len(),
            resources = ?resource_uris,
            "list_resources"
        );

        Ok(ListResourcesResult::with_all_items(mcp_resources))
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let oauth_ctx = self
            .get_oauth_context(&context.extensions)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let server_id = self
            .services
            .pool_services
            .feature_service
            .find_server_for_resource(&oauth_ctx.space_id.to_string(), &params.uri)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to resolve resource: {}", e), None)
            })?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Resource '{}' not found", params.uri), None)
            })?;

        // Verify authorization
        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to get grants: {}", e), None))?;

        let authorized_resources = self
            .services
            .pool_services
            .feature_service
            .get_resources_for_grants(&oauth_ctx.space_id.to_string(), &feature_set_ids)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to verify authorization: {}", e), None)
            })?;

        let is_authorized = authorized_resources
            .iter()
            .any(|r| r.server_id == server_id && r.feature_name == params.uri && r.is_available);

        if !is_authorized {
            return Err(McpError::invalid_params(
                format!("Resource '{}' not authorized", params.uri),
                None,
            ));
        }

        let contents_values = self
            .services
            .pool_services
            .pool_service
            .read_resource(oauth_ctx.space_id, &server_id, &params.uri)
            .await
            .map_err(|e| McpError::internal_error(format!("Read resource failed: {}", e), None))?;

        // Convert Vec<Value> to Vec<ResourceContents>
        let contents: Vec<ResourceContents> = contents_values
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        Ok(ReadResourceResult { contents })
    }

    /// Override on_custom_request to handle "initialize" with flexible protocol negotiation
    ///
    /// Clients may send newer protocol versions with capability structures we don't recognize.
    /// Instead of failing deserialization, we extract only the required fields and respond
    /// with our maximum supported version, allowing graceful protocol negotiation.
    async fn on_custom_request(
        &self,
        request: CustomRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, McpError> {
        if request.method == "initialize" {
            warn!("[MCP] ⚠️  Initialize came as CustomRequest - protocol version mismatch likely");

            let oauth_ctx = self
                .get_oauth_context(&context.extensions)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

            let params_value = request.params.ok_or_else(|| {
                McpError::invalid_params("Initialize request missing params".to_string(), None)
            })?;

            // Extract client version and info from raw JSON
            let client_version_str = params_value
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let client_info: Option<Implementation> = params_value
                .get("clientInfo")
                .and_then(|v| serde_json::from_value(v.clone()).ok());

            // Use shared negotiation logic
            let negotiated_version = self.negotiate_protocol_version(client_version_str);

            info!(
                client_id = %oauth_ctx.client_id,
                space_id = %oauth_ctx.space_id,
                client_info = ?client_info,
                protocol_version = %negotiated_version,
                "[MCP] 🔌 Client initializing with flexible negotiation"
            );

            // Build response using shared logic
            let result = self.build_initialize_result(negotiated_version);

            match serde_json::to_value(result) {
                Ok(json) => return Ok(CustomResult::new(json)),
                Err(e) => {
                    return Err(McpError::internal_error(
                        format!("Failed to serialize initialize result: {}", e),
                        None,
                    ))
                }
            }
        }

        // For other custom requests, return method not found
        Err(McpError::new(
            ErrorCode::METHOD_NOT_FOUND,
            request.method,
            None,
        ))
    }
}
