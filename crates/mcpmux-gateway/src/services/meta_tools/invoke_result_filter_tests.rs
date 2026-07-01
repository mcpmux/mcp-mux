use super::super::invoke_payload_parse::{
    bracketed_array_key_base, parse_structured_payload_from_text,
};
use super::super::invoke_result_shaping::{byte_truncation_envelope, shape_content_block};
use super::*;
use serde_json::{json, Value};

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
    let items: Vec<Value> = (0..100)
        .map(|i| json!({ "id": i, "name": format!("n{i}") }))
        .collect();
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
    let text = shaped_content[0]
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap();
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

    let parsed_text: Value = serde_json::from_str(
        shaped_content[0]
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap(),
    )
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
fn posthog_paginated_results_truncates_from_content_json() {
    let results: Vec<Value> = (0..16)
        .map(|i| {
            json!({
                "name": format!("Insight {i}"),
                "short_id": format!("ins-{i}"),
                "description": "noise"
            })
        })
        .collect();
    let payload = json!({
        "count": 16,
        "next": null,
        "previous": null,
        "results": results,
    });
    let content = vec![json!({
        "type": "text",
        "text": payload.to_string(),
    })];
    let filter = parse_invoke_filter(Some(&json!({
        "max_rows": 3,
        "fields": ["name", "short_id"]
    })))
    .unwrap();

    let (shaped_content, shaped_structured) = apply_invoke_result_filter(content, None, &filter);

    let structured = shaped_structured.expect("structured shaped from content JSON");
    assert_eq!(structured.get("returned"), Some(&json!(3)));
    assert_eq!(structured.get("total"), Some(&json!(16)));
    assert_eq!(structured.get("truncated"), Some(&json!(true)));
    let sample = structured
        .get("results")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(sample.len(), 3);
    assert_eq!(
        sample[0],
        json!({ "name": "Insight 0", "short_id": "ins-0" })
    );

    let text = shaped_content[0]
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap();
    let parsed_text: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed_text.get("returned"), Some(&json!(3)));
}

#[test]
fn yaml_payload_parses_posthog_insights_list_shape() {
    let mut yaml = String::from("count: 16\nnext: null\nprevious: null\nresults[16]:\n");
    for i in 0..16 {
        yaml.push_str(&format!(
            "  - id: {i}\n    short_id: ins-{i}\n    name: Insight {i}\n    description: noise\n"
        ));
    }
    let parsed = parse_structured_payload_from_text(&yaml).expect("yaml parses");
    let results = parsed
        .get("results")
        .and_then(|v| v.as_array())
        .expect("results array");
    assert_eq!(results.len(), 16);

    let filter = parse_invoke_filter(Some(&json!({
        "max_rows": 3,
        "fields": ["name", "short_id"]
    })))
    .unwrap();
    let shaped = shape_json_value(parsed, &filter);
    assert_eq!(shaped.get("returned"), Some(&json!(3)), "shaped: {shaped}");
}

#[test]
fn posthog_paginated_results_truncates_from_content_yaml() {
    let mut yaml = String::from("count: 16\nnext: null\nprevious: null\nresults[16]:\n");
    for i in 0..16 {
        yaml.push_str(&format!(
            "  - id: {i}\n    short_id: ins-{i}\n    name: Insight {i}\n    description: noise\n"
        ));
    }
    let content = vec![json!({
        "type": "text",
        "text": yaml,
    })];
    let filter = parse_invoke_filter(Some(&json!({
        "max_rows": 3,
        "fields": ["name", "short_id"]
    })))
    .unwrap();

    let (shaped_content, shaped_structured) = apply_invoke_result_filter(content, None, &filter);

    let structured = shaped_structured.expect("structured shaped from content YAML");
    assert_eq!(structured.get("returned"), Some(&json!(3)));
    assert_eq!(structured.get("total"), Some(&json!(16)));
    assert_eq!(structured.get("truncated"), Some(&json!(true)));
    let sample = structured
        .get("results")
        .and_then(|v| v.as_array())
        .unwrap();
    assert_eq!(sample.len(), 3);
    assert_eq!(
        sample[0],
        json!({ "name": "Insight 0", "short_id": "ins-0" })
    );

    let text = shaped_content[0]
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap();
    let parsed_text: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed_text.get("returned"), Some(&json!(3)));
}

#[test]
fn bracketed_array_key_base_normalizes_posthog_results_key() {
    assert_eq!(
        bracketed_array_key_base("results[16]"),
        Some("results".to_string())
    );
    assert_eq!(bracketed_array_key_base("results"), None);
}

#[test]
fn posthog_paginated_results_truncates_from_resource_block_json() {
    let results: Vec<Value> = (0..16)
        .map(|i| {
            json!({
                "name": format!("Insight {i}"),
                "short_id": format!("ins-{i}"),
                "description": "noise"
            })
        })
        .collect();
    let payload = json!({
        "count": 16,
        "next": null,
        "previous": null,
        "results": results,
    });
    let content = vec![json!({
        "type": "resource",
        "resource": {
            "uri": "posthog://insights",
            "mimeType": "application/json",
            "text": payload.to_string(),
        }
    })];
    let filter = parse_invoke_filter(Some(&json!({
        "max_rows": 3,
        "fields": ["name", "short_id"]
    })))
    .unwrap();

    let (_, shaped_structured) = apply_invoke_result_filter(content, None, &filter);

    let structured = shaped_structured.expect("structured shaped from resource JSON");
    assert_eq!(structured.get("returned"), Some(&json!(3)));
    assert_eq!(structured.get("total"), Some(&json!(16)));
    assert_eq!(structured.get("truncated"), Some(&json!(true)));
}

#[test]
fn fields_only_projects_columns_on_nested_results() {
    let results = vec![
        json!({ "name": "a", "short_id": "1", "description": "x" }),
        json!({ "name": "b", "short_id": "2", "description": "y" }),
    ];
    let filter = InvokeResultFilter {
        fields: Some(vec!["name".into(), "short_id".into()]),
        ..Default::default()
    };
    let shaped = shape_json_value(json!({ "count": 2, "results": results }), &filter);
    let sample = shaped.get("results").and_then(|v| v.as_array()).unwrap();
    assert_eq!(sample[0], json!({ "name": "a", "short_id": "1" }));
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
    let parsed: Value =
        serde_json::from_str(shaped.get("text").unwrap().as_str().unwrap()).unwrap();
    assert_eq!(parsed.get("truncated"), Some(&json!(true)));
    assert_eq!(parsed.get("total"), Some(&json!(100)));
}

#[test]
fn byte_trunc_mid_multibyte_char_does_not_panic() {
    // Each rocket emoji is 4 bytes. A max_bytes of 5 would land in the middle of
    // the second emoji — the char-boundary floor must step back to byte 4.
    let text = "🚀🚀🚀🚀".to_string(); // 16 bytes total
    let envelope = byte_truncation_envelope(&text, 5);
    assert_eq!(envelope.get("truncated"), Some(&json!(true)));
    assert_eq!(envelope.get("total"), Some(&json!(16)));
    // returned must be ≤ max_bytes and land on a char boundary (4, not 5)
    let returned = envelope.get("returned").and_then(|v| v.as_u64()).unwrap();
    assert!(returned <= 5, "returned {returned} exceeds max_bytes 5");
    // The text field must be valid UTF-8 (would panic on deser otherwise)
    let text_val = envelope.get("text").and_then(|v| v.as_str()).unwrap();
    assert!(text_val.ends_with("...[truncated]"));
}

#[test]
fn byte_trunc_exact_char_boundary_does_not_regress() {
    // "café" = 5 bytes (c-a-f-é where é is 2 bytes). max_bytes=4 lands exactly at
    // a boundary — no backward walk needed, output is "caf".
    let text = "café";
    let envelope = byte_truncation_envelope(text, 4);
    assert_eq!(envelope.get("truncated"), Some(&json!(true)));
    let text_val = envelope.get("text").and_then(|v| v.as_str()).unwrap();
    assert!(text_val.starts_with("caf"), "got: {text_val}");
}
