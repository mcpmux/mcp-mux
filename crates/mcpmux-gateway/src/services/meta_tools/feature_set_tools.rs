//! `mcpmux_list_feature_sets` and `mcpmux_get_tool_schema` — read-only discovery helpers.

use async_trait::async_trait;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use std::collections::HashSet;

use super::meta_tool_common::{caller_resolution, caller_space_id, text_result};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};

// ---------------------------------------------------------------------------
// mcpmux_list_feature_sets — read
// ---------------------------------------------------------------------------

pub struct ListFeatureSetsTool;

#[async_trait]
impl MetaTool for ListFeatureSetsTool {
    fn name(&self) -> &'static str {
        "mcpmux_list_feature_sets"
    }

    fn description(&self) -> &'static str {
        "List every FeatureSet defined in the caller's resolved Space — \
         built-ins and custom. Each entry carries `id`, `name`, `description`, \
         `type`, `is_builtin`, and `status` (`active` when bound to this \
         workspace, `inactive` when available to bind). To activate capability, \
         call mcpmux_bind_current_workspace with an inactive entry's `id`."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;
        let space = call
            .ctx
            .space_repo
            .get(&space_id)
            .await?
            .ok_or_else(|| MetaToolError::Internal("space missing".into()))?;
        let bound_ids: HashSet<String> = resolved.feature_set_ids.iter().cloned().collect();
        let sets = call
            .ctx
            .feature_set_repo
            .list_by_space(&space_id.to_string())
            .await?;
        let sets: Vec<_> = sets
            .iter()
            .filter(|fs| !fs.is_deleted)
            .map(|fs| {
                let status = if bound_ids.contains(&fs.id) {
                    "active"
                } else {
                    "inactive"
                };
                json!({
                    "id": fs.id,
                    "name": fs.name,
                    "description": fs.description,
                    "type": fs.feature_set_type,
                    "is_builtin": fs.is_builtin,
                    "status": status,
                })
            })
            .collect();
        Ok(text_result(
            json!({ "space_id": space.id, "feature_sets": sets }),
        ))
    }
}

// ---------------------------------------------------------------------------
// mcpmux_get_tool_schema — read
// ---------------------------------------------------------------------------

/// Parsed `tools` argument for schema lookup, retaining invalid entries for `missing`.
struct ToolSchemaNameRequest {
    valid_names: Vec<String>,
    invalid_entries: Vec<String>,
}

/// Parse the `tools` argument from `mcpmux_get_tool_schema` call args.
///
/// Accepts a qualified name string, a string array, or a JSON-encoded array
/// string (common when agents double-serialize through MCP clients).
fn parse_tool_schema_names(value: Option<&Value>) -> Result<ToolSchemaNameRequest, MetaToolError> {
    let Some(value) = value else {
        return Err(MetaToolError::InvalidArgument(
            "missing or invalid `tools` — expected string or string array".into(),
        ));
    };

    match value {
        Value::String(s) => {
            if let Ok(Value::Array(arr)) = serde_json::from_str(s) {
                return names_from_json_array(&arr);
            }
            Ok(ToolSchemaNameRequest {
                valid_names: vec![s.clone()],
                invalid_entries: Vec::new(),
            })
        }
        Value::Array(arr) => names_from_json_array(arr),
        _ => Err(MetaToolError::InvalidArgument(
            "missing or invalid `tools` — expected string or string array".into(),
        )),
    }
}

/// Split a JSON string array into valid qualified names and invalid entries (e.g. empty strings).
fn names_from_json_array(arr: &[Value]) -> Result<ToolSchemaNameRequest, MetaToolError> {
    let mut valid_names = Vec::new();
    let mut invalid_entries = Vec::new();

    for value in arr {
        match value.as_str() {
            Some(name) if name.trim().is_empty() => invalid_entries.push(name.to_string()),
            Some(name) => valid_names.push(name.trim().to_string()),
            None => invalid_entries.push(value.to_string()),
        }
    }

    if valid_names.is_empty() && invalid_entries.is_empty() {
        return Err(MetaToolError::InvalidArgument(
            "`tools` must contain at least one qualified name".into(),
        ));
    }

    Ok(ToolSchemaNameRequest {
        valid_names,
        invalid_entries,
    })
}

pub struct GetToolSchemaTool;

#[async_trait]
impl MetaTool for GetToolSchemaTool {
    fn name(&self) -> &'static str {
        "mcpmux_get_tool_schema"
    }

    fn description(&self) -> &'static str {
        "Load input schemas for one or more qualified tool names before \
         invoking via mcpmux_invoke_tool. Pass tools as a single qualified \
         name string or a string array (e.g. [\"github_list_issues\"]). \
         Set compact: true to omit descriptions. Tools must be invokable \
         with current grants — use mcpmux_search_tools to discover names."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["tools"],
            "properties": {
                "tools": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ]
                },
                "tool_name": {
                    "type": "string",
                    "description": "Alias for tools (single qualified name)"
                },
                "tool": {
                    "type": "string",
                    "description": "Alias for tools (single qualified name)"
                },
                "compact": { "type": "boolean", "default": false }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;

        let tools_value = call
            .args
            .get("tools")
            .or_else(|| call.args.get("tool_name"))
            .or_else(|| call.args.get("tool"));
        let schema_request = parse_tool_schema_names(tools_value)?;
        let tool_names = schema_request.valid_names;

        let compact = call
            .args
            .get("compact")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let invokable = call
            .ctx
            .feature_service
            .get_invokable_tools_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let index = call
            .ctx
            .tool_discovery
            .build_index(&space_id.to_string(), &invokable)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let schemas = crate::services::tool_discovery::ToolDiscoveryService::get_schemas(
            &index,
            &tool_names,
            compact,
        );

        let found_names: HashSet<String> = schemas
            .iter()
            .flat_map(|s| {
                [
                    s.get("qualified_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    s.get("feature_name")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                ]
                .into_iter()
                .flatten()
            })
            .collect();
        let mut missing: Vec<String> = tool_names
            .iter()
            .filter(|name| !found_names.contains(*name))
            .cloned()
            .collect();
        missing.extend(schema_request.invalid_entries);

        if missing.is_empty() {
            return Ok(text_result(json!({ "schemas": schemas })));
        }

        let missing_list: Vec<&str> = missing.iter().map(String::as_str).collect();
        Ok(text_result(json!({
            "schemas": schemas,
            "missing": missing_list,
            "message": format!(
                "{} tool(s) not invokable or unknown with current grants → use mcpmux_search_tools to discover allowed names",
                missing.len()
            ),
        })))
    }
}
