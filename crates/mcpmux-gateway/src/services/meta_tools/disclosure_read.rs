//! Meta tools for reading resources and fetching prompts (progressive disclosure).

use std::collections::HashSet;

use async_trait::async_trait;
use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Value};

use super::meta_tool_common::{caller_resolution, caller_space_id, text_result};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use crate::pool::{format_server_inactive_error, FeatureService};
use crate::services::levenshtein_suggestions;

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

fn disclosure_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(
        json!({ "error": "disclosure_denied", "message": message }).to_string(),
    )])
}

// ---------------------------------------------------------------------------
// mcpmux_read_resource — read
// ---------------------------------------------------------------------------

pub struct ReadResourceTool;

#[async_trait]
impl MetaTool for ReadResourceTool {
    fn name(&self) -> &'static str {
        "mcpmux_read_resource"
    }

    fn description(&self) -> &'static str {
        "Read a backend resource URI after grant checks. Use mcpmux_search_resources \
         to discover readable URIs."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["uri"],
            "properties": {
                "uri": { "type": "string" }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let uri = call
            .args
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `uri`".into()))?
            .to_string();

        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;

        let readable = call
            .ctx
            .feature_service
            .get_readable_resources_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let server_id = match FeatureService::resolve_resource_server_from_grants(&readable, &uri) {
            Some(server_id) => server_id,
            None => {
                let index = call
                    .ctx
                    .resource_discovery
                    .build_index(&space_id.to_string(), &readable)
                    .await
                    .map_err(|e| MetaToolError::Internal(e.to_string()))?;
                let candidates: Vec<String> = index.iter().map(|e| e.uri.clone()).collect();
                let suggestions = levenshtein_suggestions(&uri, &candidates, 3);
                let message = if suggestions.is_empty() {
                    format!("resource '{uri}' is not readable with current grants")
                } else {
                    format!(
                        "resource '{uri}' is not readable — did you mean {}?",
                        suggestions.join(", ")
                    )
                };
                return Ok(disclosure_error(message));
            }
        };

        let binding_servers = binding_servers_for_call(&call).await?;

        if !is_server_active(&server_id, &binding_servers) {
            return Ok(disclosure_error(format_server_inactive_error(&server_id)));
        }

        let backend =
            call.ctx.disclosure_backend.as_ref().ok_or_else(|| {
                MetaToolError::Internal("disclosure routing not configured".into())
            })?;

        let contents = backend
            .read_resource(space_id, &server_id, &uri)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        Ok(text_result(json!({ "uri": uri, "contents": contents })))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_fetch_prompt — read
// ---------------------------------------------------------------------------

pub struct FetchPromptTool;

#[async_trait]
impl MetaTool for FetchPromptTool {
    fn name(&self) -> &'static str {
        "mcpmux_fetch_prompt"
    }

    fn description(&self) -> &'static str {
        "Fetch a backend prompt after grant checks. Use mcpmux_search_prompts \
         to discover fetchable prompt names."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["server_id", "prompt"],
            "properties": {
                "server_id": { "type": "string" },
                "prompt": { "type": "string" },
                "args": { "type": "object" }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let server_id = call
            .args
            .get("server_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `server_id`".into()))?
            .to_string();
        let prompt_name = call
            .args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `prompt`".into()))?
            .to_string();
        let args = call.args.get("args").cloned().unwrap_or_else(|| json!({}));

        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;

        let binding_servers = binding_servers_for_call(&call).await?;

        if !is_server_active(&server_id, &binding_servers) {
            return Ok(disclosure_error(format_server_inactive_error(&server_id)));
        }

        let fetchable = call
            .ctx
            .feature_service
            .get_fetchable_prompts_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let qualified_name = fetchable
            .iter()
            .find(|f| f.server_id == server_id && f.feature_name == prompt_name)
            .map(|f| f.qualified_name())
            .unwrap_or_else(|| format!("{server_id}_{prompt_name}"));

        let is_fetchable = fetchable
            .iter()
            .any(|f| f.server_id == server_id && f.feature_name == prompt_name && f.is_available);

        if !is_fetchable {
            let index = call
                .ctx
                .prompt_discovery
                .build_index(&space_id.to_string(), &fetchable)
                .await
                .map_err(|e| MetaToolError::Internal(e.to_string()))?;
            let candidates: Vec<String> = index
                .iter()
                .filter(|e| e.server_id == server_id)
                .map(|e| e.feature_name.clone())
                .collect();
            let suggestions = levenshtein_suggestions(&prompt_name, &candidates, 5);
            let message = if suggestions.is_empty() {
                format!(
                    "prompt '{qualified_name}' is not fetchable with current grants (server_id='{server_id}', prompt='{prompt_name}')"
                )
            } else {
                format!(
                    "prompt '{qualified_name}' is not fetchable — did you mean {}?",
                    suggestions.join(", ")
                )
            };
            return Ok(disclosure_error(message));
        }

        let backend =
            call.ctx.disclosure_backend.as_ref().ok_or_else(|| {
                MetaToolError::Internal("disclosure routing not configured".into())
            })?;

        let arguments = args.as_object().cloned();
        let result = backend
            .fetch_prompt(space_id, &server_id, &prompt_name, arguments)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        Ok(text_result(json!({
            "server_id": server_id,
            "prompt": prompt_name,
            "qualified_name": qualified_name,
            "result": result,
        })))
    }
}
