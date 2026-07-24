//! Active tool index build, session cache, and embedding hydration for `mcpmux_search_tools`.

use std::collections::HashSet;
use std::time::Instant;
use tracing::debug;
use uuid::Uuid;

use super::registry::{MetaToolCall, MetaToolError};
use crate::services::ResolvedFeatureSet;

/// Build the active tool index from DB grants (no cache write).
pub(crate) async fn build_active_index(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
    resolved: &ResolvedFeatureSet,
    query_id: &str,
) -> Result<Vec<crate::services::ToolIndexEntry>, MetaToolError> {
    let invokable_started = Instant::now();
    let invokable = call
        .ctx
        .feature_service
        .get_invokable_tools_for_grants(&space_id.to_string(), &resolved.feature_set_ids)
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;
    let invokable_ms = invokable_started.elapsed().as_millis() as u64;

    let build_index_started = Instant::now();
    let index = call
        .ctx
        .tool_discovery
        .build_index(&space_id.to_string(), &invokable)
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;
    let build_index_ms = build_index_started.elapsed().as_millis() as u64;

    debug!(
        query_id,
        invokable_count = invokable.len(),
        index_entries = index.len(),
        invokable_ms,
        build_index_ms,
        active_index_build_ms = invokable_ms + build_index_ms,
        "[search] active index build"
    );

    Ok(index)
}

/// Build the active index and store it in the per-session search cache.
pub(crate) async fn build_and_cache_active_index(
    call: &MetaToolCall<'_>,
    space_id: &Uuid,
    resolved: &ResolvedFeatureSet,
    fingerprint: u64,
    session_id: &str,
    query_id: &str,
) -> Result<Vec<crate::services::ToolIndexEntry>, MetaToolError> {
    let index = build_active_index(call, space_id, resolved, query_id).await?;
    call.ctx
        .search_cache
        .insert(session_id.to_string(), (fingerprint, index.clone()));
    Ok(index)
}

/// Load missing active-tool vectors from persistent storage into the global embedding map.
pub(crate) async fn hydrate_active_embeddings(
    call: &MetaToolCall<'_>,
    query_id: &str,
    active_index: &[crate::services::ToolIndexEntry],
) -> Result<u64, MetaToolError> {
    let hydrate_started = Instant::now();
    let missing_hashes: HashSet<String> = active_index
        .iter()
        .map(crate::services::tool_discovery::entry_content_hash)
        .filter(|content_hash| !call.ctx.embedding_store.contains_key(content_hash))
        .collect();
    let hashes_requested = missing_hashes.len();

    if missing_hashes.is_empty() {
        let store_hits = active_index
            .iter()
            .map(crate::services::tool_discovery::entry_content_hash)
            .filter(|content_hash| call.ctx.embedding_store.contains_key(content_hash))
            .count();
        let hydrate_ms = hydrate_started.elapsed().as_millis() as u64;
        debug!(
            query_id,
            hashes_requested = 0,
            store_hits,
            store_misses = 0,
            hydrate_ms,
            "[embed] store hydrate"
        );
        return Ok(hydrate_ms);
    }

    let missing_hashes: Vec<String> = missing_hashes.into_iter().collect();
    let db_started = Instant::now();
    let records = call
        .ctx
        .embedding_repo
        .get_many(&missing_hashes, call.ctx.embeddings.model_version())
        .await
        .map_err(|error| MetaToolError::Internal(error.to_string()))?;
    let db_ms = db_started.elapsed().as_millis() as u64;

    for record in records {
        call.ctx
            .embedding_store
            .insert(record.content_hash, record.vector);
    }
    let store_hits = missing_hashes
        .iter()
        .filter(|content_hash| call.ctx.embedding_store.contains_key(*content_hash))
        .count();
    let hydrate_ms = hydrate_started.elapsed().as_millis() as u64;
    debug!(
        query_id,
        hashes_requested,
        store_hits,
        store_misses = hashes_requested.saturating_sub(store_hits),
        db_ms,
        hydrate_ms,
        "[embed] store hydrate"
    );

    Ok(hydrate_ms)
}
