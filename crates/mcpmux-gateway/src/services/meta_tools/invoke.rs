//! `mcpmux_invoke_tool` — permission-checked gateway into backend MCP tools.

use async_trait::async_trait;
use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Map, Value};

use super::registry::{MetaTool, MetaToolCall, MetaToolError};
use super::tools::{caller_resolution, caller_space_id};
use crate::pool::{format_invoke_permission_denied, format_server_inactive_error};
use crate::services::tool_discovery::ToolDiscoveryService;
use mcpmux_core::FeatureType;

/// Object keys that commonly hold large list payloads from backend tools.
const HEAVY_ARRAY_KEYS: &[&str] = &[
    "items", "data", "results", "rows", "records", "issues", "entries", "values", "list",
];

/// Optional post-processing controls for invoke results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvokeResultFilter {
    pub max_rows: Option<usize>,
    pub max_bytes: Option<usize>,
    pub fields: Option<Vec<String>>,
    pub format: Option<String>,
}

/// Parse the optional `filter` object from `mcpmux_invoke_tool` arguments.
pub fn parse_invoke_filter(value: Option<&Value>) -> Option<InvokeResultFilter> {
    let filter = value?;
    if !filter.is_object() {
        return None;
    }

    Some(InvokeResultFilter {
        max_rows: filter
            .get("max_rows")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize),
        max_bytes: filter
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize),
        fields: filter.get("fields").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
        }),
        format: filter
            .get("format")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

impl InvokeResultFilter {
    fn is_summary(&self) -> bool {
        self.format.as_deref() == Some("summary")
    }
}

/// Post-process routed tool output before returning it to the MCP client.
pub fn apply_invoke_result_filter(
    content: Vec<Value>,
    structured_content: Option<Value>,
    filter: &InvokeResultFilter,
) -> (Vec<Value>, Option<Value>) {
    let shaped_structured = structured_content.map(|value| shape_json_value(value, filter));
    let shaped_content = content
        .into_iter()
        .map(|block| shape_content_block(block, filter))
        .collect();
    (shaped_content, shaped_structured)
}

/// Meta tool that forwards invocations to [`RoutingService::call_tool`].
pub struct InvokeToolTool;

#[async_trait]
impl MetaTool for InvokeToolTool {
    fn name(&self) -> &'static str {
        "mcpmux_invoke_tool"
    }

    fn description(&self) -> &'static str {
        "Invoke a backend MCP tool by server_id and tool name. Requires the \
         server to be active (binding or session enable) and the tool to be \
         in the current permission set. Use mcpmux_search_tools and \
         mcpmux_get_tool_schema before calling. Pass an optional filter object \
         to bound large payloads; omit filter to return the backend response as-is."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["server_id", "tool"],
            "properties": {
                "server_id": {
                    "type": "string",
                    "description": "Registry server id (e.g. github)"
                },
                "tool": {
                    "type": "string",
                    "description": "Bare tool name on that server (e.g. list_issues), not the qualified name"
                },
                "args": {
                    "type": "object",
                    "description": "Arguments object passed to the backend tool",
                    "default": {}
                },
                "filter": {
                    "type": "object",
                    "description": "Optional result shaping (max_rows, max_bytes, fields, format). Omit to return the backend response as-is.",
                    "properties": {
                        "max_rows": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum rows/items to return from large arrays"
                        },
                        "max_bytes": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum UTF-8 bytes for text or serialized JSON payloads"
                        },
                        "fields": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "When set, keep only these fields on each object in list results"
                        },
                        "format": {
                            "type": "string",
                            "enum": ["summary", "full"],
                            "description": "When max_rows is set: summary caps the sample at min(max_rows, 5); full returns up to max_rows rows. Ignored when max_rows is omitted."
                        }
                    }
                }
            }
        })
    }

    fn is_write(&self) -> bool {
        false
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let server_id = call
            .args
            .get("server_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `server_id`".into()))?
            .to_string();
        let tool_name = call
            .args
            .get("tool")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MetaToolError::InvalidArgument("missing `tool`".into()))?
            .to_string();
        let args = call.args.get("args").cloned().unwrap_or_else(|| json!({}));
        let filter = parse_invoke_filter(call.args.get("filter"));

        let resolved = caller_resolution(&call).await?;
        let space_id = caller_space_id(&call).await?;
        let session_id = call.session_id;

        let invokable = call
            .ctx
            .feature_service
            .get_invokable_tools_for_grants(
                &space_id.to_string(),
                &resolved.feature_set_ids,
                session_id,
            )
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;

        let binding_features = call
            .ctx
            .feature_service
            .resolve_feature_sets(&space_id.to_string(), &resolved.feature_set_ids)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()))?;
        let binding_servers: std::collections::HashSet<String> = binding_features
            .iter()
            .map(|f| f.server_id.clone())
            .collect();
        let session_enabled = session_id
            .map(|sid| call.ctx.session_overrides.enabled_set(sid))
            .unwrap_or_default();
        let session_disabled = session_id
            .map(|sid| call.ctx.session_overrides.disabled_set(sid))
            .unwrap_or_default();

        let is_server_active = binding_servers.contains(&server_id)
            || (session_enabled.contains(&server_id) && !session_disabled.contains(&server_id));

        if session_disabled.contains(&server_id) {
            return Ok(invoke_error(format!(
                "server '{server_id}' is disabled for this session → mcpmux_enable_server({{ \"server_id\": \"{server_id}\" }})"
            )));
        }

        if !is_server_active {
            return Ok(invoke_error(format_server_inactive_error(&server_id)));
        }

        let qualified_name = invokable
            .iter()
            .find(|f| f.server_id == server_id && f.feature_name == tool_name)
            .map(|f| f.qualified_name())
            .unwrap_or_else(|| format!("{server_id}_{tool_name}"));
        let is_invokable = invokable.iter().any(|f| {
            f.feature_type == FeatureType::Tool
                && f.server_id == server_id
                && f.feature_name == tool_name
                && f.is_available
        });

        if !is_invokable {
            let index = call
                .ctx
                .tool_discovery
                .build_index(&space_id.to_string(), &invokable)
                .await
                .map_err(|e| MetaToolError::Internal(e.to_string()))?;
            let suggestions: Vec<String> = ToolDiscoveryService::search(
                &index,
                Some(&tool_name),
                Some(&server_id),
                crate::services::tool_discovery::DetailLevel::Name,
                5,
                None,
            )
            .tools
            .iter()
            .filter_map(|v| {
                v.get("qualified_name")
                    .and_then(|n| n.as_str().map(String::from))
            })
            .collect();
            return Ok(invoke_error(format_invoke_permission_denied(
                &qualified_name,
                &server_id,
                &tool_name,
                &suggestions,
            )));
        }

        let backend = call
            .ctx
            .invoke_backend
            .as_ref()
            .ok_or_else(|| MetaToolError::Internal("invoke routing not configured".into()))?;
        match backend
            .call_tool(
                space_id,
                &resolved.feature_set_ids,
                session_id,
                &qualified_name,
                args,
            )
            .await
        {
            Ok(result) => {
                if result.is_error {
                    let content: Vec<Content> = result
                        .content
                        .into_iter()
                        .filter_map(|v| serde_json::from_value(v).ok())
                        .collect();
                    let mut mcp_result = CallToolResult::error(content);
                    mcp_result.structured_content = result.structured_content;
                    return Ok(mcp_result);
                }

                let (content, structured_content) = if let Some(ref filter) = filter {
                    apply_invoke_result_filter(result.content, result.structured_content, filter)
                } else {
                    (result.content, result.structured_content)
                };
                let parsed_content: Vec<Content> = content
                    .into_iter()
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect();
                let mut mcp_result = CallToolResult::success(parsed_content);
                mcp_result.structured_content = structured_content;
                Ok(mcp_result)
            }
            Err(e) => Ok(invoke_error(e.to_string())),
        }
    }
}

/// Shape one MCP content block (typically `{ "type": "text", "text": "..." }`).
fn shape_content_block(block: Value, filter: &InvokeResultFilter) -> Value {
    let Some(text) = block.get("text").and_then(|v| v.as_str()) else {
        return block;
    };

    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
        let shaped = shape_json_value(parsed, filter);
        return json!({
            "type": "text",
            "text": shaped.to_string(),
        });
    }

    let Some(max_bytes) = filter.max_bytes else {
        return block;
    };
    if text.len() <= max_bytes {
        return block;
    }

    let envelope = byte_truncation_envelope(text, max_bytes);
    json!({
        "type": "text",
        "text": envelope.to_string(),
    })
}

/// Shape a JSON value, applying truncation when explicit filter limits are set.
pub fn shape_json_value(value: Value, filter: &InvokeResultFilter) -> Value {
    match value {
        Value::Array(items) => shape_array(items, filter, "items"),
        Value::Object(map) => shape_object(map, filter),
        other => enforce_byte_limit(other, filter),
    }
}

fn shape_object(map: Map<String, Value>, filter: &InvokeResultFilter) -> Value {
    for key in HEAVY_ARRAY_KEYS {
        if let Some(Value::Array(items)) = map.get(*key).cloned() {
            if should_truncate(items.len(), filter) {
                return shape_object_with_truncated_array(map, key, items, filter);
            }
        }
    }

    for (key, value) in &map {
        if let Value::Array(items) = value {
            if should_truncate(items.len(), filter) {
                return shape_object_with_truncated_array(map.clone(), key, items.clone(), filter);
            }
        }
    }

    enforce_byte_limit(Value::Object(map), filter)
}

fn shape_object_with_truncated_array(
    mut map: Map<String, Value>,
    array_key: &str,
    items: Vec<Value>,
    filter: &InvokeResultFilter,
) -> Value {
    let shaped_array = shape_array(items, filter, array_key);
    if let Value::Object(truncation) = &shaped_array {
        if truncation.get("truncated") == Some(&Value::Bool(true)) {
            for (meta_key, meta_value) in truncation {
                if meta_key != array_key {
                    map.insert(meta_key.clone(), meta_value.clone());
                }
            }
            if let Some(data) = truncation.get(array_key) {
                map.insert(array_key.to_string(), data.clone());
            }
            return enforce_byte_limit(Value::Object(map), filter);
        }
    }

    map.insert(array_key.to_string(), shaped_array);
    enforce_byte_limit(Value::Object(map), filter)
}

fn shape_array(items: Vec<Value>, filter: &InvokeResultFilter, data_key: &str) -> Value {
    let total = items.len();
    let filtered_items = apply_fields_filter(items, filter);

    let Some(max_rows) = filter.max_rows else {
        return enforce_byte_limit(Value::Array(filtered_items), filter);
    };

    if total <= max_rows {
        return enforce_byte_limit(Value::Array(filtered_items), filter);
    }

    let sample_size = if filter.is_summary() {
        max_rows.min(5)
    } else {
        max_rows
    };
    let sample: Vec<Value> = filtered_items.into_iter().take(sample_size).collect();
    let returned = sample.len();

    json!({
        "returned": returned,
        "total": total,
        "truncated": true,
        data_key: sample,
    })
}

fn apply_fields_filter(items: Vec<Value>, filter: &InvokeResultFilter) -> Vec<Value> {
    let Some(fields) = &filter.fields else {
        return items;
    };

    items
        .into_iter()
        .map(|item| pick_fields(item, fields))
        .collect()
}

fn pick_fields(value: Value, fields: &[String]) -> Value {
    let Value::Object(map) = value else {
        return value;
    };

    let mut picked = Map::new();
    for field in fields {
        if let Some(v) = map.get(field) {
            picked.insert(field.clone(), v.clone());
        }
    }
    Value::Object(picked)
}

fn should_truncate(length: usize, filter: &InvokeResultFilter) -> bool {
    match filter.max_rows {
        Some(max_rows) => length > max_rows,
        None => false,
    }
}

fn enforce_byte_limit(value: Value, filter: &InvokeResultFilter) -> Value {
    let Some(max_bytes) = filter.max_bytes else {
        return value;
    };

    let serialized = value.to_string();
    if serialized.len() <= max_bytes {
        return value;
    }

    byte_truncation_envelope(&serialized, max_bytes)
}

/// Build a `{ returned, total, truncated, text }` envelope for byte-capped plain text or JSON.
fn byte_truncation_envelope(text: &str, max_bytes: usize) -> Value {
    let total_bytes = text.len();
    let mut truncated = text.to_string();
    truncated.truncate(max_bytes);
    truncated.push_str("...[truncated]");
    json!({
        "returned": truncated.len(),
        "total": total_bytes,
        "truncated": true,
        "text": truncated,
    })
}

/// Build a structured MCP error payload for invoke failures.
fn invoke_error(message: String) -> CallToolResult {
    let payload = json!({
        "error": "invoke_failed",
        "message": message,
    });
    CallToolResult::error(vec![Content::text(payload.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_rows(count: usize) -> Vec<Value> {
        (0..count)
            .map(|i| {
                json!({
                    "id": i,
                    "title": format!("issue-{i}"),
                    "body": format!("body-{i}")
                })
            })
            .collect()
    }

    #[test]
    fn no_filter_passes_through_large_array() {
        let items: Vec<Value> = (0..100).map(|i| json!({ "id": i, "name": format!("n{i}") })).collect();
        let shaped = shape_json_value(Value::Array(items.clone()), &InvokeResultFilter::default());
        assert_eq!(shaped, Value::Array(items));
    }

    #[test]
    fn explicit_max_rows_truncates_top_level_array() {
        let items: Vec<Value> = issue_rows(20);
        let filter = InvokeResultFilter {
            max_rows: Some(3),
            ..Default::default()
        };
        let shaped = shape_json_value(Value::Array(items), &filter);
        assert_eq!(shaped.get("returned"), Some(&json!(3)));
        assert_eq!(shaped.get("total"), Some(&json!(20)));
        assert_eq!(shaped.get("truncated"), Some(&json!(true)));
        let sample = shaped.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(sample.len(), 3);
    }

    #[test]
    fn explicit_max_rows_truncates_nested_issues_key() {
        let issues = issue_rows(20);
        let filter = InvokeResultFilter {
            max_rows: Some(3),
            ..Default::default()
        };
        let shaped = shape_json_value(json!({ "issues": issues }), &filter);
        assert_eq!(shaped.get("returned"), Some(&json!(3)));
        assert_eq!(shaped.get("total"), Some(&json!(20)));
        assert_eq!(shaped.get("truncated"), Some(&json!(true)));
        let sample = shaped.get("issues").and_then(|v| v.as_array()).unwrap();
        assert_eq!(sample.len(), 3);
    }

    #[test]
    fn json_in_text_block_truncates_with_metadata() {
        let rows: Vec<Value> = (0..80).map(|i| json!({ "n": i })).collect();
        let content = vec![json!({
            "type": "text",
            "text": json!({ "results": rows }).to_string(),
        })];
        let filter = parse_invoke_filter(Some(&json!({ "max_rows": 10 }))).unwrap();

        let (shaped_content, _) = apply_invoke_result_filter(content, None, &filter);
        let text = shaped_content[0].get("text").and_then(|t| t.as_str()).unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();

        assert_eq!(parsed.get("returned"), Some(&json!(10)));
        assert_eq!(parsed.get("total"), Some(&json!(80)));
        assert_eq!(parsed.get("truncated"), Some(&json!(true)));
    }

    #[test]
    fn structured_content_and_text_both_shaped() {
        let items = issue_rows(20);
        let structured = json!({ "items": items });
        let content = vec![json!({
            "type": "text",
            "text": structured.to_string(),
        })];
        let filter = InvokeResultFilter {
            max_rows: Some(5),
            fields: Some(vec!["id".into(), "title".into()]),
            ..Default::default()
        };

        let (shaped_content, shaped_structured) =
            apply_invoke_result_filter(content, Some(structured), &filter);

        let parsed_text: Value =
            serde_json::from_str(shaped_content[0].get("text").and_then(|t| t.as_str()).unwrap())
                .unwrap();
        assert_eq!(parsed_text.get("returned"), Some(&json!(5)));
        assert_eq!(parsed_text.get("total"), Some(&json!(20)));

        let shaped = shaped_structured.unwrap();
        let structured_sample = shaped.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(structured_sample.len(), 5);
        assert_eq!(structured_sample[0], json!({ "id": 0, "title": "issue-0" }));
    }

    #[test]
    fn fields_filter_keeps_only_requested_columns() {
        let items = vec![
            json!({ "id": 1, "name": "a", "secret": "x" }),
            json!({ "id": 2, "name": "b", "secret": "y" }),
        ];
        let filter = InvokeResultFilter {
            fields: Some(vec!["id".into(), "name".into()]),
            ..Default::default()
        };
        let shaped = shape_json_value(Value::Array(items), &filter);
        let kept = shaped.as_array().unwrap();
        assert_eq!(kept[0], json!({ "id": 1, "name": "a" }));
        assert_eq!(kept[1], json!({ "id": 2, "name": "b" }));
    }

    #[test]
    fn max_rows_and_fields_together() {
        let items: Vec<Value> = (0..30)
            .map(|i| json!({ "id": i, "label": format!("row-{i}") }))
            .collect();
        let filter = parse_invoke_filter(Some(&json!({ "max_rows": 5, "fields": ["id"] }))).unwrap();
        let shaped = shape_json_value(Value::Array(items), &filter);

        assert_eq!(shaped.get("returned"), Some(&json!(5)));
        assert_eq!(shaped.get("total"), Some(&json!(30)));
        assert_eq!(shaped.get("truncated"), Some(&json!(true)));
        let sample = shaped.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(sample.len(), 5);
        assert_eq!(sample[0], json!({ "id": 0 }));
    }

    #[test]
    fn summary_format_no_op_when_max_rows_at_most_five() {
        let items = issue_rows(20);
        let filter = InvokeResultFilter {
            max_rows: Some(3),
            format: Some("summary".into()),
            ..Default::default()
        };
        let shaped = shape_json_value(Value::Array(items), &filter);
        assert_eq!(shaped.get("returned"), Some(&json!(3)));
    }

    #[test]
    fn summary_format_caps_sample_at_five() {
        let items = issue_rows(20);
        let filter = InvokeResultFilter {
            max_rows: Some(10),
            format: Some("summary".into()),
            ..Default::default()
        };
        let shaped = shape_json_value(Value::Array(items), &filter);
        assert_eq!(shaped.get("returned"), Some(&json!(5)));
        assert_eq!(shaped.get("total"), Some(&json!(20)));
    }

    #[test]
    fn full_format_returns_up_to_max_rows() {
        let items = issue_rows(20);
        let filter = InvokeResultFilter {
            max_rows: Some(10),
            format: Some("full".into()),
            ..Default::default()
        };
        let shaped = shape_json_value(Value::Array(items), &filter);
        assert_eq!(shaped.get("returned"), Some(&json!(10)));
        let sample = shaped.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(sample.len(), 10);
    }

    #[test]
    fn parse_invoke_filter_ignores_invalid_types() {
        let filter = parse_invoke_filter(Some(&json!({
            "max_rows": "not-a-number",
            "max_bytes": true,
            "fields": "id",
            "format": 123
        })))
        .unwrap();
        assert_eq!(filter.max_rows, None);
        assert_eq!(filter.max_bytes, None);
        assert_eq!(filter.fields, None);
        assert_eq!(filter.format, None);
    }

    #[test]
    fn parse_invoke_filter_accepts_partial_objects() {
        let filter = parse_invoke_filter(Some(&json!({ "max_rows": 3 }))).unwrap();
        assert_eq!(filter.max_rows, Some(3));
        assert_eq!(filter.max_bytes, None);
    }

    #[test]
    fn max_bytes_only_truncates_top_level_json_array() {
        let items: Vec<Value> = (0..50)
            .map(|i| json!({ "id": i, "label": format!("row-{i}-padding") }))
            .collect();
        let filter = InvokeResultFilter {
            max_bytes: Some(512),
            ..Default::default()
        };
        let shaped = shape_json_value(Value::Array(items), &filter);
        assert_eq!(shaped.get("truncated"), Some(&json!(true)));
        assert!(shaped.get("total").and_then(|v| v.as_u64()).unwrap_or(0) > 512);
    }

    #[test]
    fn plain_text_byte_trunc_includes_metadata() {
        let text = "x".repeat(100);
        let filter = InvokeResultFilter {
            max_bytes: Some(50),
            ..Default::default()
        };
        let block = json!({ "type": "text", "text": text });
        let shaped = shape_content_block(block, &filter);
        let parsed: Value = serde_json::from_str(shaped.get("text").unwrap().as_str().unwrap()).unwrap();
        assert_eq!(parsed.get("truncated"), Some(&json!(true)));
        assert_eq!(parsed.get("total"), Some(&json!(100)));
    }
}
