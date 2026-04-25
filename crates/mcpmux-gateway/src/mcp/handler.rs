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

use super::context::{extract_oauth_context, extract_session_id, OAuthContext};
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

    /// Log resolver decision, emit `WorkspaceNeedsBinding` when a session
    /// reports roots but no binding matched (`source=Default`), and — when
    /// the session's resolved FS *flipped* from a prior value — fire a
    /// per-peer `list_changed` so the client re-pulls its tools.
    ///
    /// `notifier` is optional: callers from contexts where peer notification
    /// doesn't apply (e.g. rootless init paths) can pass `None`.
    ///
    /// Rootless sessions never trigger the binding prompt — there's nothing
    /// to bind (caller passes `root_for_prompt = None`).
    async fn log_and_notify_resolution(
        services: &std::sync::Arc<crate::server::ServiceContainer>,
        notifier: Option<&MCPNotifier>,
        client_id: &str,
        session_id: Option<&str>,
        root_for_prompt: Option<&str>,
    ) {
        let resolver = &services.feature_set_resolver;
        match resolver.resolve(session_id).await {
            Ok(resolved) => {
                info!(
                    %client_id,
                    session_id = session_id.unwrap_or("<none>"),
                    feature_set_id = resolved.feature_set_id.clone().unwrap_or_else(|| "<deny>".into()),
                    space_id = resolved.space_id.map(|u| u.to_string()).unwrap_or_else(|| "<none>".into()),
                    source = ?resolved.source,
                    "[FeatureSetResolver] resolved",
                );

                // Track the resolved FS per session so we can detect flips.
                // The very first sighting (no prior entry) counts as a flip
                // — that's the case where the client's `tools/list` at init
                // saw the fallback set but roots arriving later may have
                // landed on a different binding. Firing once on first sight
                // is safe (idempotent re-list); the dedup protects against
                // repeated identical resolutions.
                if let (Some(sid), Some(notifier)) = (session_id, notifier) {
                    let changed = services
                        .session_roots
                        .record_resolution(sid, resolved.feature_set_id.as_deref());
                    if changed {
                        notifier.notify_peer_lists_changed(client_id).await;
                    }
                }

                // Prompt only when the session reported a root AND no binding
                // matched (source=Default). `session_id` must be Some too so
                // the UI can correlate back to this peer.
                let should_prompt =
                    matches!(resolved.source, crate::services::ResolutionSource::Default);
                if let (true, Some(sid), Some(space_id), Some(root)) = (
                    should_prompt,
                    session_id,
                    resolved.space_id,
                    root_for_prompt,
                ) {
                    services.gateway_state.read().await.emit_domain_event(
                        mcpmux_core::DomainEvent::WorkspaceNeedsBinding {
                            client_id: client_id.to_string(),
                            session_id: sid.to_string(),
                            space_id,
                            workspace_root: root.to_string(),
                        },
                    );
                }
            }
            Err(e) => {
                warn!(
                    %client_id,
                    error = %e,
                    "[FeatureSetResolver] resolve failed",
                );
            }
        }
    }

    /// Build InitializeResult with negotiated protocol version
    fn build_initialize_result(&self, protocol_version: ProtocolVersion) -> InitializeResult {
        let info = self.get_info();
        let mut result = InitializeResult::new(info.capabilities);
        result.protocol_version = protocol_version;
        result.server_info = info.server_info;
        result.instructions = info.instructions;
        result
    }
}

impl ServerHandler for McpMuxGatewayHandler {
    fn get_info(&self) -> ServerInfo {
        use rmcp::model::{PromptsCapability, ResourcesCapability, ToolsCapability};

        // Note: get_info is called frequently, no logging needed

        let capabilities = ServerCapabilities::builder()
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
            .build();
        let mut server_info = Implementation::new("mcpmux-gateway", env!("CARGO_PKG_VERSION"));
        server_info.title = Some("McpMux".to_string());
        let mut info = ServerInfo::new(capabilities);
        info.server_info = server_info;
        info.instructions = Some(
            "McpMux aggregates multiple MCP servers. Use tools/prompts/resources \
             from your authorized backend servers."
                .to_string(),
        );
        info
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
            .register_peer(oauth_ctx.client_id.clone(), peer.clone());

        // Mark the client stream as active immediately - RMCP's session transport
        // handles SSE streaming and message caching internally
        self.notification_bridge
            .mark_client_stream_active(&oauth_ctx.client_id);

        // Pre-populate feature hashes to prevent spurious first notifications
        self.notification_bridge
            .prime_hashes_for_space(oauth_ctx.space_id)
            .await;

        // If the peer advertised the `roots` capability, fetch its reported
        // workspace roots into the session registry so the resolver can pick
        // a binding. Then log + (if no binding matched) prompt the UI.
        if let Some(session_id) = extract_session_id(&context.extensions) {
            let declares_roots = peer
                .peer_info()
                .map(|info| info.capabilities.roots.is_some())
                .unwrap_or(false);
            if declares_roots {
                let peer_for_roots = peer.clone();
                let session_roots = self.services.session_roots.clone();
                let services = self.services.clone();
                let notifier = self.notification_bridge.clone();
                let client_id_str = oauth_ctx.client_id.clone();
                let session_id_for_task = session_id.clone();
                tokio::spawn(async move {
                    match peer_for_roots.list_roots().await {
                        Ok(result) => {
                            let uris: Vec<String> =
                                result.roots.iter().map(|r| r.uri.to_string()).collect();
                            session_roots
                                .set(&session_id_for_task, uris.iter().map(|s| s.as_str()));
                            debug!(
                                client_id = %client_id_str,
                                session_id = %session_id_for_task,
                                roots = ?uris,
                                "[FeatureSetResolver] fetched MCP roots",
                            );

                            // Tell the desktop UI the detected-roots list may
                            // have grown so the Workspaces tab refreshes
                            // without waiting for a polling cycle.
                            services
                                .gateway_state
                                .read()
                                .await
                                .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);

                            // Pick the longest (most specific) normalized
                            // root for the sheet. The resolver has already
                            // normalized them on insert. Passing `Some(root)`
                            // lets log_and_notify_resolution emit
                            // `WorkspaceNeedsBinding` if the resolver ended
                            // up at `source = Default` (i.e. no binding yet).
                            let root_for_prompt =
                                session_roots.get(&session_id_for_task).and_then(|roots| {
                                    roots
                                        .into_iter()
                                        .filter(|r| !r.is_empty())
                                        .max_by_key(|r| r.len())
                                });

                            Self::log_and_notify_resolution(
                                &services,
                                Some(&notifier),
                                &client_id_str,
                                Some(&session_id_for_task),
                                root_for_prompt.as_deref(),
                            )
                            .await;
                        }
                        Err(e) => {
                            debug!(
                                client_id = %client_id_str,
                                session_id = %session_id_for_task,
                                error = %e,
                                "[FeatureSetResolver] peer.list_roots() failed — falling back to active Space default",
                            );
                        }
                    }
                });
            } else {
                // No roots declared — silent default, never prompt
                // (root_for_prompt = None suppresses the emit).
                Self::log_and_notify_resolution(
                    &self.services,
                    Some(&self.notification_bridge),
                    &oauth_ctx.client_id,
                    Some(&session_id),
                    None,
                )
                .await;
            }
        }

        info!(
            client_id = %oauth_ctx.client_id,
            space_id = %oauth_ctx.space_id,
            "Client initialized - peer registered for notifications"
        );
    }

    /// The client told us its roots list changed (e.g. VS Code added a
    /// folder to a multi-root workspace). Re-fetch via `list_roots`,
    /// update the session registry, and re-run the resolver — if any root
    /// is still unbound, `log_and_notify_resolution` fires a fresh
    /// `WorkspaceNeedsBinding` so the sheet pops for the newly-surfaced
    /// folder.
    async fn on_roots_list_changed(&self, context: NotificationContext<RoleServer>) {
        let oauth_ctx = match self.get_oauth_context(&context.extensions) {
            Ok(ctx) => ctx,
            Err(e) => {
                warn!(
                    "Failed to extract OAuth context on_roots_list_changed: {}",
                    e
                );
                return;
            }
        };
        let Some(session_id) = extract_session_id(&context.extensions) else {
            debug!("[FeatureSetResolver] roots/list_changed with no session id — skipping");
            return;
        };
        let peer = std::sync::Arc::new(context.peer);
        let session_roots = self.services.session_roots.clone();
        let services = self.services.clone();
        let notifier = self.notification_bridge.clone();
        let client_id_str = oauth_ctx.client_id.clone();
        let session_id_for_task = session_id.clone();
        tokio::spawn(async move {
            match peer.list_roots().await {
                Ok(result) => {
                    let uris: Vec<String> =
                        result.roots.iter().map(|r| r.uri.to_string()).collect();
                    session_roots.set(&session_id_for_task, uris.iter().map(|s| s.as_str()));
                    debug!(
                        client_id = %client_id_str,
                        session_id = %session_id_for_task,
                        roots = ?uris,
                        "[FeatureSetResolver] refreshed MCP roots (roots/list_changed)",
                    );
                    services
                        .gateway_state
                        .read()
                        .await
                        .emit_domain_event(mcpmux_core::DomainEvent::SessionRootsChanged);

                    let root_for_prompt =
                        session_roots.get(&session_id_for_task).and_then(|roots| {
                            roots
                                .into_iter()
                                .filter(|r| !r.is_empty())
                                .max_by_key(|r| r.len())
                        });
                    Self::log_and_notify_resolution(
                        &services,
                        Some(&notifier),
                        &client_id_str,
                        Some(&session_id_for_task),
                        root_for_prompt.as_deref(),
                    )
                    .await;
                }
                Err(e) => {
                    debug!(
                        client_id = %client_id_str,
                        session_id = %session_id_for_task,
                        error = %e,
                        "[FeatureSetResolver] refresh list_roots failed — silent",
                    );
                }
            }
        });
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
            .get_client_grants(
                &oauth_ctx.client_id,
                &oauth_ctx.space_id,
                extract_session_id(&context.extensions).as_deref(),
            )
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
        let mut mcp_tools: Vec<Tool> = tools
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

        // Append built-in `mcpmux_*` meta tools when enabled. Default is ON;
        // users can set `gateway.meta_tools_enabled = "false"` in settings
        // to hide the entire namespace — useful when a deployment explicitly
        // wants a non-self-managing gateway.
        if self.services.meta_tool_registry.is_enabled().await {
            mcp_tools.extend(self.services.meta_tool_registry.list_as_tools());
        }

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

        let session_id_owned = extract_session_id(&context.extensions);
        let session_id = session_id_owned.as_deref();

        // Intercept meta tools (mcpmux_*) BEFORE feature-set filtering.
        // When the master switch is off we fall through to the feature-set
        // path where the tool will miss and surface a normal "not found"
        // error — same behaviour a client would see for any unknown tool.
        if crate::services::is_meta_tool(&params.name)
            && self.services.meta_tool_registry.contains(&params.name)
            && self.services.meta_tool_registry.is_enabled().await
        {
            let client_uuid = uuid::Uuid::parse_str(&oauth_ctx.client_id)
                .map_err(|e| McpError::invalid_params(format!("bad client_id: {e}"), None))?;
            let args: serde_json::Value = params
                .arguments
                .map(|a| serde_json::to_value(a).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            return match self
                .services
                .meta_tool_registry
                .call(&params.name, &client_uuid, session_id, args)
                .await
            {
                Ok(result) => Ok(result),
                Err(e) => Ok(e.into_call_tool_result()),
            };
        }

        // Get client's feature set grants for authorization
        let feature_set_ids = self
            .services
            .authorization_service
            .get_client_grants(&oauth_ctx.client_id, &oauth_ctx.space_id, session_id)
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

        let result = if tool_result.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
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
            .get_client_grants(
                &oauth_ctx.client_id,
                &oauth_ctx.space_id,
                extract_session_id(&context.extensions).as_deref(),
            )
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
            .get_client_grants(
                &oauth_ctx.client_id,
                &oauth_ctx.space_id,
                extract_session_id(&context.extensions).as_deref(),
            )
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
            .get_client_grants(
                &oauth_ctx.client_id,
                &oauth_ctx.space_id,
                extract_session_id(&context.extensions).as_deref(),
            )
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
            .get_client_grants(
                &oauth_ctx.client_id,
                &oauth_ctx.space_id,
                extract_session_id(&context.extensions).as_deref(),
            )
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

        Ok(ReadResourceResult::new(contents))
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
