//! Post-processing filters for `mcpmux_invoke_tool` backend payloads.

use serde_json::{json, Value};

pub use super::invoke_result_shaping::shape_json_value;

use super::invoke_payload_parse::coalesce_structured_payload;
use super::invoke_result_shaping::shape_content_blocks;

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
    pub(crate) fn is_summary(&self) -> bool {
        self.format.as_deref() == Some("summary")
    }

    /// Whether any shaping limit is set (row/byte/field caps).
    pub fn has_effect(&self) -> bool {
        self.max_rows.is_some() || self.max_bytes.is_some() || self.fields.is_some()
    }
}

/// Post-process routed tool output before returning it to the MCP client.
pub fn apply_invoke_result_filter(
    content: Vec<Value>,
    structured_content: Option<Value>,
    filter: &InvokeResultFilter,
) -> (Vec<Value>, Option<Value>) {
    if !filter.has_effect() {
        return (content, structured_content);
    }

    let Some(payload) = coalesce_structured_payload(&content, structured_content) else {
        let shaped_content = shape_content_blocks(content, filter);
        return (shaped_content, None);
    };

    let shaped = shape_json_value(payload, filter);
    let shaped_content = vec![json!({
        "type": "text",
        "text": shaped.to_string(),
    })];
    (shaped_content, Some(shaped))
}

#[cfg(test)]
#[path = "invoke_result_filter_tests.rs"]
mod tests;
