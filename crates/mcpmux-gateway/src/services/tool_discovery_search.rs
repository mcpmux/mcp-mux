//! Hybrid and lexical search execution for the active tool index.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use serde_json::{json, Map, Value};
use tracing::{debug, info, trace};

use crate::services::discovery_rank::{
    build_corpus_doc_freq, filter_and_rank, filter_and_rank_traced, lexical_score_precomputed,
    prepare_query_tokens, tokenize, RankTraceContext,
};
use crate::services::embedding::{EmbeddingService, EmbeddingState};

use super::tool_discovery_index::entry_content_hash;
use super::tool_discovery_types::{DetailLevel, SearchContext, SearchToolsResult, ToolIndexEntry};
use super::ToolDiscoveryService;

/// Lexical weight for hybrid score fusion.
///
/// Tuned against the 20-case intent→tool relevance fixture in
/// `tests/rust/tests/integration/search_relevance_eval.rs` (Phase 4). At 0.4/0.6
/// hybrid passes all fixture cases in top-3 while lexical-only passes ~11/20;
/// lowering lexical (e.g. 0.3) risks exact-name queries losing to semantic noise,
/// raising it (e.g. 0.5) drops intent-only queries with zero token overlap.
const LEXICAL_FUSION_WEIGHT: f32 = 0.4;

/// Semantic weight for hybrid score fusion (complement of [`LEXICAL_FUSION_WEIGHT`]).
const SEMANTIC_FUSION_WEIGHT: f32 = 0.6;

impl ToolDiscoveryService {
    /// Search the index with optional query, server filter, and pagination.
    #[allow(clippy::too_many_arguments)]
    pub fn search(
        index: &[ToolIndexEntry],
        query: Option<&str>,
        server_id: Option<&str>,
        detail_level: DetailLevel,
        limit: usize,
        cursor: Option<&str>,
        query_id: Option<&str>,
        hybrid: Option<SearchContext<'_>>,
        server_readiness: Option<&HashMap<String, &'static str>>,
        server_display_names: Option<&HashMap<String, String>>,
        prefilled_params_by_server: Option<&HashMap<String, Vec<String>>>,
        include_invoke_example: bool,
    ) -> SearchToolsResult {
        let limit = limit.clamp(1, 100);
        let offset = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);

        let haystack_fn = |entry: &ToolIndexEntry| entry_search_haystack(entry);

        let lexical_started = Instant::now();
        let (mut ranked, top_lexical_score) = if let Some(query_id) = query_id {
            let trace = RankTraceContext { query_id };
            filter_and_rank_traced(
                index,
                query,
                server_id,
                |entry| entry.server_id.as_str(),
                haystack_fn,
                &trace,
            )
        } else {
            (
                filter_and_rank(
                    index,
                    query,
                    server_id,
                    |entry| entry.server_id.as_str(),
                    haystack_fn,
                ),
                None,
            )
        };
        let lexical_ms = lexical_started.elapsed().as_millis() as u64;

        let hybrid_started = Instant::now();
        let (ranking, top_fused_score) =
            if let (Some(query), Some(query_id), Some(ctx)) = (query, query_id, hybrid) {
                rank_with_hybrid(
                    &mut ranked,
                    query,
                    query_id,
                    ctx,
                    haystack_fn,
                    top_lexical_score,
                )
            } else {
                ("lexical", top_lexical_score)
            };
        let hybrid_ms = hybrid_started.elapsed().as_millis() as u64;

        let total = ranked.len();

        let paginate_started = Instant::now();
        let page: Vec<Value> = ranked
            .iter()
            .skip(offset)
            .take(limit)
            .map(|entry| {
                let readiness = server_readiness
                    .map(|map| map.get(&entry.server_id).copied().unwrap_or("bindable"));
                let display_name = server_display_names.and_then(|map| map.get(&entry.server_id));
                let prefilled_keys = prefilled_params_by_server
                    .and_then(|map| map.get(&entry.server_id))
                    .map(Vec::as_slice);
                entry_to_json(
                    entry,
                    detail_level,
                    readiness,
                    display_name.map(String::as_str),
                    prefilled_keys,
                    include_invoke_example,
                )
            })
            .collect();
        let paginate_ms = paginate_started.elapsed().as_millis() as u64;

        if let Some(query_id) = query_id {
            debug!(
                query_id,
                index_entries = index.len(),
                ranked_count = total,
                lexical_ms,
                hybrid_ms,
                paginate_ms,
                rank_total_ms = lexical_ms + hybrid_ms + paginate_ms,
                "[search] rank phase"
            );
        }

        let next_offset = offset + page.len();
        let next_cursor = if next_offset < total {
            Some(next_offset.to_string())
        } else {
            None
        };

        SearchToolsResult {
            tools: page,
            next_cursor,
            total,
            ranking,
            top_fused_score,
        }
    }

    /// Resolve schemas for one or more qualified tool names.
    pub fn get_schemas(
        index: &[ToolIndexEntry],
        tool_names: &[String],
        compact: bool,
    ) -> Vec<Value> {
        tool_names
            .iter()
            .filter_map(|name| {
                let entry = index
                    .iter()
                    .find(|e| e.qualified_name == *name || e.feature_name == *name)?;
                Some(schema_entry_to_json(entry, compact))
            })
            .collect()
    }
}

/// Haystack text for lexical and semantic ranking (`feature_name + qualified_name + description`).
fn entry_search_haystack(entry: &ToolIndexEntry) -> String {
    format!(
        "{} {} {}",
        entry.feature_name,
        entry.qualified_name,
        entry.description.as_deref().unwrap_or("")
    )
}

/// Apply hybrid fusion when the embedding model is ready; otherwise lexical-only.
fn rank_with_hybrid<'a, T, FHaystack>(
    ranked: &mut Vec<&'a T>,
    query: &str,
    query_id: &str,
    ctx: SearchContext<'_>,
    haystack_fn: FHaystack,
    top_lexical_score: Option<f64>,
) -> (&'static str, Option<f64>)
where
    T: AsRef<ToolIndexEntry> + 'a,
    FHaystack: Fn(&T) -> String,
{
    let model_state = ctx.embeddings.state();
    let model_ready = matches!(model_state, EmbeddingState::Ready);
    if !model_ready {
        ctx.embeddings.ensure_init_started();
    }

    if !model_ready || ranked.is_empty() {
        let skip_reason = if !model_ready {
            "model_not_ready"
        } else {
            "empty_ranked"
        };
        log_cache_decision(
            query_id,
            ctx.index_cache_hit,
            "skipped",
            Some(skip_reason),
            Some(&model_state),
            ctx.active_index.len(),
            ranked.len(),
        );
        return ("lexical", top_lexical_score);
    }

    let vectors_started = Instant::now();
    let vectors_present = ctx
        .active_index
        .iter()
        .filter(|entry| {
            let content_hash = entry_content_hash(entry);
            ctx.embedding_store.contains_key(&content_hash)
        })
        .count();
    let vectors_scan_ms = vectors_started.elapsed().as_millis() as u64;
    let lexical_only_docs = ctx.active_index.len().saturating_sub(vectors_present);
    debug!(
        query_id,
        active_tools = ctx.active_index.len(),
        vectors_present,
        lexical_only_docs,
        vectors_scan_ms,
        "[search] read"
    );

    let active_keys: HashSet<&str> = ctx
        .active_index
        .iter()
        .map(|e| e.qualified_name.as_str())
        .collect();

    log_cache_decision(
        query_id,
        ctx.index_cache_hit,
        if vectors_present > 0 { "hit" } else { "miss" },
        None,
        None,
        ctx.active_index.len(),
        ranked.len(),
    );

    let inline_embed_started = Instant::now();
    let Some(query_vector) = ctx.embeddings.embed_query(query, Some(query_id)) else {
        debug!(
            query_id,
            model_state = ?ctx.embeddings.state(),
            embed_ms = inline_embed_started.elapsed().as_millis() as u64,
            skip_reason = "query_embed_failed",
            "[search] hybrid abort"
        );
        return ("lexical", top_lexical_score);
    };
    info!(
        target: "embed",
        query_id,
        docs_embedded = 1,
        embed_ms = inline_embed_started.elapsed().as_millis() as u64,
        "[embed] inline query embed"
    );

    // Precompute corpus statistics and per-doc tokens once. The public
    // `lexical_score` helper rebuilt the corpus doc-frequency map on every
    // call, making this loop O(N^2) in tokenization; building the stats a
    // single time keeps it O(N) (matches the lexical pass in discovery_rank).
    let corpus_started = Instant::now();
    let haystacks: Vec<String> = ranked.iter().map(|entry| haystack_fn(entry)).collect();
    let (corpus_size, corpus_doc_freq) = build_corpus_doc_freq(&haystacks);
    let query_tokens = prepare_query_tokens(query);
    let corpus_ms = corpus_started.elapsed().as_millis() as u64;

    let lexical_scores_started = Instant::now();
    let lexical_scores: Vec<f64> = haystacks
        .iter()
        .map(|haystack| {
            let doc_tokens = tokenize(haystack);
            lexical_score_precomputed(&query_tokens, &doc_tokens, corpus_size, &corpus_doc_freq)
        })
        .collect();
    let lexical_scores_ms = lexical_scores_started.elapsed().as_millis() as u64;

    let max_lexical = lexical_scores
        .iter()
        .copied()
        .fold(0.0_f64, f64::max)
        .max(1e-9);

    let fusion_started = Instant::now();
    let mut fused_scores: Vec<f64> = Vec::with_capacity(ranked.len());
    for (idx, entry) in ranked.iter().enumerate() {
        let tool_entry = entry.as_ref();
        let norm_lexical = (lexical_scores[idx] / max_lexical) as f32;
        let maybe_doc_vector = if active_keys.contains(tool_entry.qualified_name.as_str()) {
            let content_hash = entry_content_hash(tool_entry);
            ctx.embedding_store.get(&content_hash)
        } else {
            None
        };
        let semantic = maybe_doc_vector
            .as_ref()
            .map(|doc_vector| EmbeddingService::cosine(&query_vector, doc_vector.value()))
            .unwrap_or(0.0);
        let has_vector = maybe_doc_vector.is_some();
        let fused = if active_keys.contains(tool_entry.qualified_name.as_str()) && has_vector {
            (LEXICAL_FUSION_WEIGHT * norm_lexical + SEMANTIC_FUSION_WEIGHT * semantic) as f64
        } else {
            lexical_scores[idx]
        };
        trace!(
            query_id,
            qualified_name = %tool_entry.qualified_name,
            lexical_score = lexical_scores[idx],
            semantic_score = semantic,
            fused_score = fused,
            "[search] entry score"
        );
        fused_scores.push(fused);
    }
    let fusion_ms = fusion_started.elapsed().as_millis() as u64;

    let sort_started = Instant::now();
    let mut scored: Vec<(&T, f64)> = ranked.drain(..).zip(fused_scores).collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| haystack_fn(a.0).cmp(&haystack_fn(b.0)))
    });
    let top_fused_score = scored.first().map(|(_, score)| *score);
    *ranked = scored.into_iter().map(|(entry, _)| entry).collect();
    let sort_ms = sort_started.elapsed().as_millis() as u64;

    if vectors_present == 0 {
        debug!(
            query_id,
            ranked_count = ranked.len(),
            corpus_ms,
            lexical_scores_ms,
            fusion_ms,
            sort_ms,
            skip_reason = "vectors_present_zero",
            "[search] hybrid abort"
        );
        return ("lexical", top_lexical_score);
    }

    debug!(
        query_id,
        ranking = "hybrid",
        ranked_count = ranked.len(),
        corpus_ms,
        lexical_scores_ms,
        fusion_ms,
        sort_ms,
        hybrid_compute_ms = corpus_ms + lexical_scores_ms + fusion_ms + sort_ms,
        lexical_weight = LEXICAL_FUSION_WEIGHT,
        semantic_weight = SEMANTIC_FUSION_WEIGHT,
        "[search] fusion"
    );

    ("hybrid", top_fused_score)
}

fn log_cache_decision(
    query_id: &str,
    index_cache_hit: bool,
    embedding_store: &str,
    skip_reason: Option<&str>,
    model_state: Option<&EmbeddingState>,
    active_tools: usize,
    ranked_count: usize,
) {
    let model_state_label = model_state.map(|s| match s {
        EmbeddingState::NotDownloaded => "not_downloaded",
        EmbeddingState::Downloading => "downloading",
        EmbeddingState::Ready => "ready",
        EmbeddingState::Failed { .. } => "failed",
    });
    debug!(
        query_id,
        index_cache = if index_cache_hit { "hit" } else { "miss" },
        embedding_store,
        skip_reason,
        model_state = model_state_label,
        active_tools,
        ranked_count,
        "[search] cache decision"
    );
}

/// JSON Schema `type` for one property (string or first element of a type array).
fn schema_property_type(prop: &Value) -> String {
    match prop.get("type") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => arr
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        _ => "unknown".to_string(),
    }
}

/// Max optional params inlined in search hits (token budget guard).
const OPTIONAL_PARAM_CAP: usize = 8;

/// Required parameter name + type for search results (minimal schema-lite).
fn extract_required_param_specs(input_schema: Option<&Value>) -> Vec<Value> {
    let Some(schema) = input_schema else {
        return Vec::new();
    };
    let Some(required) = schema.get("required").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    let properties = schema.get("properties").and_then(|p| p.as_object());

    required
        .iter()
        .filter_map(|v| v.as_str())
        .map(|name| {
            let param_type = properties
                .and_then(|props| props.get(name))
                .map(schema_property_type)
                .unwrap_or_else(|| "unknown".to_string());
            json!({ "name": name, "type": param_type })
        })
        .collect()
}

/// Optional parameter name + type for search results (minimal schema-lite, capped).
fn extract_optional_param_specs(input_schema: Option<&Value>) -> Vec<Value> {
    let Some(schema) = input_schema else {
        return Vec::new();
    };
    let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let required: HashSet<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut optional: Vec<(String, &Value)> = properties
        .iter()
        .filter(|(name, _)| !required.contains(name.as_str()))
        .map(|(name, prop)| (name.clone(), prop))
        .collect();
    optional.sort_by(|a, b| a.0.cmp(&b.0));

    optional
        .into_iter()
        .take(OPTIONAL_PARAM_CAP)
        .map(|(name, prop)| {
            let param_type = schema_property_type(prop);
            json!({ "name": name, "type": param_type })
        })
        .collect()
}

/// Whether a property schema exceeds shallow type resolution (oneOf, $ref, nested object, …).
fn schema_property_is_complex(prop: &Value) -> bool {
    if prop.get("oneOf").is_some() || prop.get("anyOf").is_some() || prop.get("$ref").is_some() {
        return true;
    }
    match prop.get("type") {
        Some(Value::String(t)) if t == "object" => prop.get("properties").is_some(),
        Some(Value::Array(types)) => types
            .iter()
            .any(|t| t.as_str() == Some("object") && prop.get("properties").is_some()),
        _ => false,
    }
}

/// Whether the input schema needs a full read via get_tool_schema.
fn input_schema_is_complex(input_schema: Option<&Value>) -> bool {
    let Some(schema) = input_schema else {
        return false;
    };
    if schema.get("oneOf").is_some()
        || schema.get("anyOf").is_some()
        || schema.get("$ref").is_some()
    {
        return true;
    }
    let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) else {
        return false;
    };
    properties
        .values()
        .any(|prop| schema_property_type(prop) == "unknown" || schema_property_is_complex(prop))
}

/// Placeholder value for one required param in a copy-paste `invoke_example`.
fn param_invoke_placeholder(param_type: &str) -> String {
    format!("<{param_type}>")
}

/// Copy-paste-ready `mcpmux_invoke_tool` shape for browse hits.
fn build_invoke_example(entry: &ToolIndexEntry, required_params: &[Value]) -> Value {
    let mut args = Map::new();
    for param in required_params {
        let Some(name) = param.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let param_type = param
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        args.insert(
            name.to_string(),
            json!(param_invoke_placeholder(param_type)),
        );
    }
    json!({
        "server_id": entry.server_id,
        "tool": entry.feature_name,
        "args": Value::Object(args),
    })
}

fn entry_to_json(
    entry: &ToolIndexEntry,
    detail_level: DetailLevel,
    server_readiness: Option<&str>,
    server_display_name: Option<&str>,
    prefilled_keys: Option<&[String]>,
    include_invoke_example: bool,
) -> Value {
    let required_params = extract_required_param_specs(entry.input_schema.as_ref());
    let optional_params = extract_optional_param_specs(entry.input_schema.as_ref());
    let schema_complex = input_schema_is_complex(entry.input_schema.as_ref());
    let required_params = mark_prefilled_required_params(&required_params, prefilled_keys);
    let mut obj = json!({
        "server_id": entry.server_id,
        "qualified_name": entry.qualified_name,
        "bare_name": entry.feature_name,
        "available": entry.is_available,
        "required_params": required_params,
        "optional_params": optional_params,
        "schema_complex": schema_complex,
    });
    if let Some(display_name) = server_display_name {
        obj["display_name"] = json!(display_name);
    }
    if include_invoke_example {
        obj["invoke_example"] = build_invoke_example(entry, &required_params);
    }
    if let Some(readiness) = server_readiness {
        obj["server_readiness"] = json!(readiness);
    }
    if let Some(status) = &entry.status {
        obj["status"] = json!(status);
    }
    if let Some(fs_id) = &entry.bindable_feature_set_id {
        obj["bindable_feature_set_id"] = json!(fs_id);
    }
    match detail_level {
        DetailLevel::Name => {}
        DetailLevel::Description | DetailLevel::Schema => {
            if let Some(desc) = &entry.description {
                obj["description"] = json!(desc);
            }
        }
    }
    if detail_level == DetailLevel::Schema {
        if let Some(schema) = &entry.input_schema {
            obj["input_schema"] = schema.clone();
        }
    }
    obj
}

/// Annotate required params that are auto-filled from server `default_params`.
fn mark_prefilled_required_params(
    required_params: &[Value],
    prefilled_keys: Option<&[String]>,
) -> Vec<Value> {
    let Some(prefilled_keys) = prefilled_keys else {
        return required_params.to_vec();
    };
    if prefilled_keys.is_empty() {
        return required_params.to_vec();
    }

    required_params
        .iter()
        .map(|param| {
            let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if prefilled_keys.iter().any(|key| key == name) {
                let mut marked = param.clone();
                marked["prefilled"] = json!(true);
                marked
            } else {
                param.clone()
            }
        })
        .collect()
}

fn schema_entry_to_json(entry: &ToolIndexEntry, compact: bool) -> Value {
    let mut obj = json!({
        "qualified_name": entry.qualified_name,
        "server_id": entry.server_id,
        "feature_name": entry.feature_name,
    });
    if !compact {
        if let Some(desc) = &entry.description {
            obj["description"] = json!(desc);
        }
    }
    if let Some(schema) = &entry.input_schema {
        obj["input_schema"] = schema.clone();
    } else {
        obj["input_schema"] = json!({"type": "object", "properties": {}});
    }
    obj
}
