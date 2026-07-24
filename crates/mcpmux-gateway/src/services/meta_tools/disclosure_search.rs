//! Meta tools for searching resources and prompts (progressive disclosure).

use std::collections::HashSet;

use async_trait::async_trait;
use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Value};

use super::meta_tool_common::{caller_resolution, caller_space_id, text_result};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use crate::pool::{format_server_inactive_error, format_server_not_in_binding_error};
use crate::services::{
    PromptDetailLevel, PromptDiscoveryService, ResourceDetailLevel, ResourceDiscoveryService,
};

/// Returns whether `server_id` is active via the caller's binding.
fn is_server_active(server_id: &str, binding_servers: &HashSet<String>) -> bool {
    binding_servers.contains(server_id)
}

/// Collect binding server ids for the caller's resolved FeatureSets.
async fn binding_servers_for_call(
    call: &MetaToolCall<'_>,
) -> Result<HashSet<String>, MetaToolError> {
    let resolved = caller_resolution(call).await?;
    let space_id = caller_space_id(call).await?;
    let binding_features = call
        .ctx
        .feature_service
        .resolve_feature_sets(&space_id.to_string(), &resolved.feature_set_ids)
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;
    Ok(binding_features
        .iter()
        .map(|f| f.server_id.clone())
        .collect())
}

/// Validate optional `server_id` filter against binding state.
async fn validate_server_filter(
    call: &MetaToolCall<'_>,
    server_id: Option<&str>,
    readable_count_for_server: impl FnOnce() -> usize,
) -> Result<Option<String>, MetaToolError> {
    let Some(server_id) = server_id else {
        return Ok(None);
    };

    let binding_servers = binding_servers_for_call(call).await?;

    if !is_server_active(server_id, &binding_servers) {
        return Ok(Some(format_server_inactive_error(server_id)));
    }

    if readable_count_for_server() == 0 {
        return Ok(Some(format_server_not_in_binding_error(server_id)));
    }

    Ok(None)
}

fn disclosure_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(
        json!({ "error": "disclosure_denied", "message": message }).to_string(),
    )])
}

// ---------------------------------------------------------------------------
// mcpmux_search_resources — read
// ---------------------------------------------------------------------------

pub struct SearchResourcesTool;

#[async_trait]
impl MetaTool for SearchResourcesTool {
    fn name(&self) -> &'static str {
        "mcpmux_search_resources"
    }

    fn description(&self) -> &'static str {
        "Search readable backend resources in the caller's resolved Space. \
         Supports query substring match, optional server_id filter, \
         detail_level (name | description | full), and pagination."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "server_id": { "type": "string" },
                "detail_level": {
                    "type": "string",
                    "enum": ["name", "description", "full"],
                    "default": "description"
                },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                "cursor": { "type": "string" }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;
        let server_filter = call.args.get("server_id").and_then(|v| v.as_str());

        let detail_level = call
            .args
            .get("detail_level")
            .and_then(|v| v.as_str())
            .and_then(ResourceDetailLevel::parse)
            .unwrap_or(ResourceDetailLevel::Description);

        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let readable = call
            .ctx
            .feature_service
            .get_readable_resources_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        if let Some(message) = validate_server_filter(&call, server_filter, || {
            readable
                .iter()
                .filter(|f| server_filter.is_none_or(|sid| f.server_id == sid))
                .count()
        })
        .await?
        {
            return Ok(disclosure_error(message));
        }

        let index = call
            .ctx
            .resource_discovery
            .build_index(&space_id.to_string(), &readable)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let result = ResourceDiscoveryService::search(
            &index,
            call.args.get("query").and_then(|v| v.as_str()),
            server_filter,
            detail_level,
            limit,
            call.args.get("cursor").and_then(|v| v.as_str()),
        );

        let mut payload = json!({
            "resources": result.resources,
            "next_cursor": result.next_cursor,
            "total": result.total,
        });

        if result.total == 0 {
            payload["hint"] = json!(
                "No readable resources matched. Verify FeatureSet grants include resource members, \
                 or use mcpmux_bind_current_workspace when the server is inactive."
            );
        }

        Ok(text_result(payload))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_search_prompts — read
// ---------------------------------------------------------------------------

pub struct SearchPromptsTool;

#[async_trait]
impl MetaTool for SearchPromptsTool {
    fn name(&self) -> &'static str {
        "mcpmux_search_prompts"
    }

    fn description(&self) -> &'static str {
        "Search fetchable backend prompts in the caller's resolved Space. \
         Supports query substring match, optional server_id filter, \
         detail_level (name | description | full), and pagination."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "server_id": { "type": "string" },
                "detail_level": {
                    "type": "string",
                    "enum": ["name", "description", "full"],
                    "default": "description"
                },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 20 },
                "cursor": { "type": "string" }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;
        let server_filter = call.args.get("server_id").and_then(|v| v.as_str());

        let detail_level = call
            .args
            .get("detail_level")
            .and_then(|v| v.as_str())
            .and_then(PromptDetailLevel::parse)
            .unwrap_or(PromptDetailLevel::Description);

        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let fetchable = call
            .ctx
            .feature_service
            .get_fetchable_prompts_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        if let Some(message) = validate_server_filter(&call, server_filter, || {
            fetchable
                .iter()
                .filter(|f| server_filter.is_none_or(|sid| f.server_id == sid))
                .count()
        })
        .await?
        {
            return Ok(disclosure_error(message));
        }

        let index = call
            .ctx
            .prompt_discovery
            .build_index(&space_id.to_string(), &fetchable)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let result = PromptDiscoveryService::search(
            &index,
            call.args.get("query").and_then(|v| v.as_str()),
            server_filter,
            detail_level,
            limit,
            call.args.get("cursor").and_then(|v| v.as_str()),
        );

        let mut payload = json!({
            "prompts": result.prompts,
            "next_cursor": result.next_cursor,
            "total": result.total,
        });

        if result.total == 0 {
            payload["hint"] = json!(
                "No fetchable prompts matched. Verify FeatureSet grants include prompt members, \
                 or use mcpmux_bind_current_workspace when the server is inactive."
            );
        }

        Ok(text_result(payload))
    }
}
