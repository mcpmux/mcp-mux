//! Text and structured-content parsing helpers for invoke result filtering.

use serde_json::{Map, Value};

/// Object keys that commonly hold large list payloads from backend tools.
pub(crate) const HEAVY_ARRAY_KEYS: &[&str] = &[
    "items", "data", "results", "rows", "records", "issues", "entries", "values", "list",
    "insights",
];

/// Merge structuredContent and JSON text blocks into one payload for filtering.
pub(crate) fn coalesce_structured_payload(
    content: &[Value],
    structured: Option<Value>,
) -> Option<Value> {
    if let Some(value) = structured.filter(|v| !v.is_null()) {
        return Some(value);
    }

    let parsed_blocks = collect_parsed_json_blocks(content);
    if parsed_blocks.len() >= 2 {
        let rows: Vec<Value> = parsed_blocks
            .into_iter()
            .flat_map(normalize_json_rows)
            .collect();
        if rows.len() >= 2 {
            return Some(Value::Array(rows));
        }
        if rows.len() == 1 {
            return Some(rows[0].clone());
        }
        return None;
    }

    parsed_blocks.into_iter().next()
}

/// Extract human-readable payload text from an MCP content block value.
pub(crate) fn content_block_text(block: &Value) -> Option<&str> {
    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
        return Some(text);
    }

    block
        .get("resource")
        .and_then(|resource| resource.get("text"))
        .and_then(|v| v.as_str())
}

/// Parse structured payloads from plain text (JSON first, then YAML).
pub(crate) fn parse_structured_payload_from_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        return normalize_parsed_payload(parsed);
    }

    if let Some(fenced) = extract_markdown_json_fence(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<Value>(&fenced) {
            return normalize_parsed_payload(parsed);
        }
    }

    if let Ok(parsed) = serde_yaml::from_str::<Value>(trimmed) {
        if parsed.is_object() || parsed.is_array() {
            return normalize_parsed_payload(parsed);
        }
    }

    if let Some(candidate) = extract_json_object_substring(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<Value>(&candidate) {
            return normalize_parsed_payload(parsed);
        }
    }

    None
}

/// If a backend double-encodes JSON as a string, parse one more level.
fn normalize_parsed_payload(value: Value) -> Option<Value> {
    if let Value::String(nested) = value {
        let trimmed = nested.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return parse_structured_payload_from_text(&nested);
        }
        return Some(Value::String(nested));
    }

    if let Value::Object(map) = value {
        return Some(Value::Object(normalize_bracketed_array_keys(map)));
    }

    Some(value)
}

/// Normalize YAML keys like `results[16]` to `results` for list shaping.
fn normalize_bracketed_array_keys(mut map: Map<String, Value>) -> Map<String, Value> {
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        let Some(normalized) = bracketed_array_key_base(&key) else {
            continue;
        };
        if normalized == key {
            continue;
        }
        if let Some(value) = map.remove(&key) {
            map.insert(normalized, value);
        }
    }
    map
}

/// Return the heavy-array base name when `key` looks like `results[16]`.
pub(crate) fn bracketed_array_key_base(key: &str) -> Option<String> {
    for base in HEAVY_ARRAY_KEYS {
        let prefix = format!("{base}[");
        if !key.starts_with(&prefix) || !key.ends_with(']') {
            continue;
        }
        let index = &key[prefix.len()..key.len() - 1];
        if index.chars().all(|c| c.is_ascii_digit()) {
            return Some(base.to_string());
        }
    }
    None
}

/// Extract JSON from ```json fenced blocks when backends wrap payloads in markdown.
fn extract_markdown_json_fence(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("```json")?;
    let after_start = &text[start + "```json".len()..];
    let end = after_start.find("```")?;
    Some(after_start[..end].trim().to_string())
}

/// Best-effort extraction of the first top-level JSON object/array substring.
fn extract_json_object_substring(text: &str) -> Option<String> {
    let start = text.find(['{', '['])?;
    let slice = &text[start..];
    let end = slice.rfind(if slice.starts_with('{') { '}' } else { ']' })?;
    Some(slice[..=end].to_string())
}

/// Parse one JSON payload per content block (text or resource).
pub(crate) fn collect_parsed_json_blocks(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| content_block_text(block).and_then(parse_structured_payload_from_text))
        .collect()
}

/// Flatten a parsed JSON value into individual row objects for list shaping.
pub(crate) fn normalize_json_rows(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        Value::Object(map) => {
            for key in HEAVY_ARRAY_KEYS {
                if let Some(Value::Array(items)) = map.get(*key) {
                    return items.clone();
                }
            }
            for value in map.values() {
                if let Value::Array(items) = value {
                    return items.clone();
                }
            }
            vec![Value::Object(map)]
        }
        other => vec![other],
    }
}
