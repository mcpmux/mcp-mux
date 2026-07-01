//! JSON and MCP content-block shaping helpers for invoke result filtering.

use serde_json::{json, Map, Value};

use super::invoke_payload_parse::{
    collect_parsed_json_blocks, content_block_text, normalize_json_rows,
    parse_structured_payload_from_text, HEAVY_ARRAY_KEYS,
};
use super::invoke_result_filter::InvokeResultFilter;

/// Shape all MCP content blocks, aggregating multi-block list payloads when needed.
pub(crate) fn shape_content_blocks(blocks: Vec<Value>, filter: &InvokeResultFilter) -> Vec<Value> {
    if blocks.is_empty() {
        return blocks;
    }

    let parsed_blocks = collect_parsed_json_blocks(&blocks);
    if parsed_blocks.len() >= 2 && filter.has_effect() {
        let rows: Vec<Value> = parsed_blocks
            .into_iter()
            .flat_map(normalize_json_rows)
            .collect();
        if rows.len() >= 2 {
            let shaped = shape_json_value(Value::Array(rows), filter);
            return vec![json!({
                "type": "text",
                "text": shaped.to_string(),
            })];
        }
    }

    blocks
        .into_iter()
        .map(|block| shape_content_block(block, filter))
        .collect()
}

/// Shape one MCP content block (typically `{ "type": "text", "text": "..." }`).
pub(crate) fn shape_content_block(block: Value, filter: &InvokeResultFilter) -> Value {
    let Some(text) = content_block_text(&block) else {
        return block;
    };

    if let Some(parsed) = parse_structured_payload_from_text(text) {
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
            if filter.fields.is_some() {
                let mut map = map;
                map.insert(
                    key.to_string(),
                    Value::Array(apply_fields_filter(items, filter)),
                );
                return enforce_byte_limit(Value::Object(map), filter);
            }
        }
    }

    for (key, value) in &map {
        if let Value::Array(items) = value {
            if should_truncate(items.len(), filter) {
                return shape_object_with_truncated_array(map.clone(), key, items.clone(), filter);
            }
            if filter.fields.is_some() {
                let mut map = map.clone();
                map.insert(
                    key.clone(),
                    Value::Array(apply_fields_filter(items.clone(), filter)),
                );
                return enforce_byte_limit(Value::Object(map), filter);
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

/// Cap serialized JSON/text size. Uses `Value::to_string()` byte length as a proxy —
/// not identical to on-wire MCP payload size, but stable enough for agent-facing truncation.
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
///
/// Floors `max_bytes` to the nearest valid UTF-8 char boundary before slicing so that
/// multi-byte characters (emoji, CJK, accented text) never cause a panic.
pub(crate) fn byte_truncation_envelope(text: &str, max_bytes: usize) -> Value {
    let total_bytes = text.len();
    let mut end = max_bytes.min(total_bytes);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = text[..end].to_string();
    truncated.push_str("...[truncated]");
    json!({
        "returned": end,
        "total": total_bytes,
        "truncated": true,
        "text": truncated,
    })
}
