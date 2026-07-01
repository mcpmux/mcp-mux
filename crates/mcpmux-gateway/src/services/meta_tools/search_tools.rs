//! `mcpmux_search_tools` — hybrid search and browse over the active tool index.

use async_trait::async_trait;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Instant;
use tracing::{debug, info};
use uuid::Uuid;

use super::meta_tool_common::{
    build_installed_server_meta_maps, build_server_readiness_map, caller_resolution,
    is_query_empty, text_result,
};
use super::registry::{feature_set_ids_fingerprint, MetaTool, MetaToolCall, MetaToolError};
use super::search_tools_index::{
    build_active_index, build_and_cache_active_index, hydrate_active_embeddings,
};

pub struct SearchToolsTool;

#[async_trait]
impl MetaTool for SearchToolsTool {
    fn name(&self) -> &'static str {
        "mcpmux_search_tools"
    }

    fn description(&self) -> &'static str {
        "Search backend tools in the caller's resolved Space. Each match includes \
         qualified_name, bare_name (use as mcpmux_invoke_tool.tool), required_params, \
         optional_params (name + type, capped), server_readiness (bindable | bound | ready), \
         schema_complex (call mcpmux_get_tool_schema when true), and invoke_example on browse \
         hits (copy-paste into mcpmux_invoke_tool). Browse mode: omit query with server_id for \
         that server's A–Z catalog, or set mode: \"browse\" alone for the whole Space (default \
         limit 50, paginated). Ranked search uses default limit 20. By default only invokable \
         tools match; set include_inactive: true (or scope \"all\") for unbound FeatureSets. \
         Supports detail_level (name | description | schema) and cursor pagination."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "server_id": { "type": "string" },
                "include_inactive": {
                    "type": "boolean",
                    "default": false,
                    "description": "When true, include tools from FeatureSets not bound to this workspace (inactive matches carry bindable_feature_set_id). Alias: scope \"all\" — same effect."
                },
                "scope": {
                    "type": "string",
                    "description": "Optional alias for include_inactive: use \"all\" to search active and inactive tools (prefer include_inactive in new calls)"
                },
                "detail_level": {
                    "type": "string",
                    "enum": ["name", "description", "schema"],
                    "default": "description"
                },
                "mode": {
                    "type": "string",
                    "enum": ["browse"],
                    "description": "Explicit browse alias: paginated A–Z catalog (default limit 50). With server_id, scopes to that server; without server_id, lists invokable tools across the whole Space. Same as omitting query when server_id is set."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "default": 20,
                    "description": "Default 20 for ranked search; 50 when browsing (empty query + server_id or mode browse)"
                },
                "cursor": { "type": "string" }
            }
        })
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let started = Instant::now();
        let query_id: String = Uuid::new_v4()
            .to_string()
            .chars()
            .filter(|c| *c != '-')
            .take(8)
            .collect();

        let resolve_started = Instant::now();
        let resolved = caller_resolution(&call).await?;
        let resolve_ms = resolve_started.elapsed().as_millis() as u64;

        // Derive from the already-resolved result — avoids a second resolver round-trip.
        let space_id = resolved.space_id.ok_or_else(|| {
            MetaToolError::Internal(
                "no Space resolved for this caller (no default Space configured?)".into(),
            )
        })?;

        debug!(
            query_id = %query_id,
            resolve_ms,
            feature_set_count = resolved.feature_set_ids.len(),
            "[search] resolver timing"
        );

        let query_str = call.args.get("query").and_then(|v| v.as_str());

        let server_id_filter = call.args.get("server_id").and_then(|v| v.as_str());
        let mode_browse = call
            .args
            .get("mode")
            .and_then(|v| v.as_str())
            .is_some_and(|m| m == "browse");
        let is_browse = mode_browse || (is_query_empty(query_str) && server_id_filter.is_some());
        let effective_query = if is_browse { None } else { query_str };

        let detail_level = call
            .args
            .get("detail_level")
            .and_then(|v| v.as_str())
            .and_then(crate::services::tool_discovery::DetailLevel::parse)
            .unwrap_or(crate::services::tool_discovery::DetailLevel::Description);

        let default_limit = if is_browse { 50 } else { 20 };
        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(default_limit) as usize;

        let scope_all = call
            .args
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s == "all")
            .unwrap_or(false);
        let include_inactive = scope_all
            || call
                .args
                .get("include_inactive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

        let fingerprint = feature_set_ids_fingerprint(&resolved.feature_set_ids);

        info!(
            query_id = %query_id,
            session_id = ?call.session_id,
            fingerprint,
            query_len = query_str.map(str::len).unwrap_or(0),
            detail_level = ?detail_level,
            limit,
            is_browse,
            include_inactive,
            "[search] call entry"
        );
        if let Some(query) = effective_query {
            debug!(query_id = %query_id, query, "[search] query text");
        }

        let readiness_map = build_server_readiness_map(&call, &space_id, &resolved).await?;
        let (server_display_names, prefilled_params_by_server) =
            build_installed_server_meta_maps(&call, &space_id).await?;

        let mut index_cache_hit = false;
        let active_index_started = Instant::now();
        let active_index = if let Some(session_id) = call.session_id {
            if let Some(entry) = call.ctx.search_cache.get(session_id) {
                let (cached_fp, cached_index) = entry.value();
                if *cached_fp == fingerprint {
                    index_cache_hit = true;
                    cached_index.clone()
                } else {
                    drop(entry);
                    build_and_cache_active_index(
                        &call,
                        &space_id,
                        &resolved,
                        fingerprint,
                        session_id,
                        query_id.as_str(),
                    )
                    .await?
                }
            } else {
                build_and_cache_active_index(
                    &call,
                    &space_id,
                    &resolved,
                    fingerprint,
                    session_id,
                    query_id.as_str(),
                )
                .await?
            }
        } else {
            build_active_index(&call, &space_id, &resolved, query_id.as_str()).await?
        };
        let active_index_ms = active_index_started.elapsed().as_millis() as u64;

        debug!(
            query_id = %query_id,
            index_cache_hit,
            active_tools = active_index.len(),
            active_index_ms,
            "[search] active index ready"
        );

        let clone_started = Instant::now();
        let mut index = active_index.clone();
        let index_clone_ms = clone_started.elapsed().as_millis() as u64;

        let mut inactive_tool_count = 0usize;
        let mut inactive_widen_ms = 0_u64;

        if include_inactive {
            debug!(
                query_id = %query_id,
                "[search] inactive scan starting"
            );
            let inactive_started = Instant::now();
            let inactive = call
                .ctx
                .feature_service
                .list_inactive_discovery_tools(
                    &space_id.to_string(),
                    &resolved.feature_set_ids,
                    Some(query_id.as_str()),
                )
                .await
                .map_err(|e| MetaToolError::Internal(e.to_string()))?;
            inactive_tool_count = inactive.len();
            let inactive_index =
                crate::services::tool_discovery::ToolDiscoveryService::build_inactive_index(
                    &inactive,
                );
            let active_keys: HashSet<(String, String)> = index
                .iter()
                .map(|e| (e.server_id.clone(), e.feature_name.clone()))
                .collect();
            let before_merge = index.len();
            for entry in inactive_index {
                let key = (entry.server_id.clone(), entry.feature_name.clone());
                if !active_keys.contains(&key) {
                    index.push(entry);
                }
            }
            index.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
            inactive_widen_ms = inactive_started.elapsed().as_millis() as u64;
            debug!(
                query_id = %query_id,
                inactive_tools = inactive_tool_count,
                merged_index = index.len(),
                added_inactive = index.len().saturating_sub(before_merge),
                inactive_widen_ms,
                "[search] inactive widen complete"
            );
        }

        let hydrate_ms = if effective_query.is_some() {
            hydrate_active_embeddings(&call, query_id.as_str(), active_index.as_slice()).await?
        } else {
            0
        };

        let hybrid = effective_query.map(|_| crate::services::tool_discovery::SearchContext {
            embeddings: call.ctx.embeddings.as_ref(),
            embedding_store: call.ctx.embedding_store.as_ref(),
            active_index: active_index.as_slice(),
            index_cache_hit,
        });

        let rank_started = Instant::now();
        let result = crate::services::tool_discovery::ToolDiscoveryService::search(
            &index,
            effective_query,
            server_id_filter,
            detail_level,
            limit,
            call.args.get("cursor").and_then(|v| v.as_str()),
            Some(query_id.as_str()),
            hybrid,
            Some(&readiness_map),
            Some(&server_display_names),
            Some(&prefilled_params_by_server),
            is_browse,
        );
        let rank_ms = rank_started.elapsed().as_millis() as u64;

        let top_qualified_name = result
            .tools
            .first()
            .and_then(|tool| tool.get("qualified_name"))
            .and_then(|value| value.as_str())
            .unwrap_or("");

        let post_started = Instant::now();
        let mut payload = json!({
            "tools": result.tools,
            "next_cursor": result.next_cursor,
            "total": result.total,
            "ranking": result.ranking,
            "scope": if include_inactive { "active_and_inactive" } else { "active_only" },
        });

        if is_browse {
            payload["mode"] = json!("browse");
        }

        if include_inactive && inactive_tool_count > 50 && server_id_filter.is_none() {
            payload["hint"] = json!("Narrow with `server_id` for faster results.");
        }

        if !include_inactive && result.total == 0 && effective_query.is_some() {
            let inactive_started = Instant::now();
            let inactive = call
                .ctx
                .feature_service
                .list_inactive_discovery_tools(
                    &space_id.to_string(),
                    &resolved.feature_set_ids,
                    Some(query_id.as_str()),
                )
                .await
                .map_err(|e| MetaToolError::Internal(e.to_string()))?;

            let ready_inactive: Vec<_> = inactive
                .into_iter()
                .filter(|entry| {
                    readiness_map
                        .get(&entry.feature.server_id)
                        .is_some_and(|readiness| *readiness == "ready")
                })
                .collect();

            if ready_inactive.is_empty() {
                payload["hint"] = json!(
                    "No active tools matched. Call mcpmux_list_servers to see installed servers \
                     and their readiness — bindable servers can be activated via \
                     mcpmux_bind_current_workspace. To browse all available tools across \
                     FeatureSets, retry with include_inactive: true."
                );
            } else {
                let preview_index =
                    crate::services::tool_discovery::ToolDiscoveryService::build_inactive_index(
                        &ready_inactive,
                    );
                let preview = crate::services::tool_discovery::ToolDiscoveryService::search(
                    &preview_index,
                    effective_query,
                    server_id_filter,
                    detail_level,
                    3,
                    None,
                    Some(query_id.as_str()),
                    None,
                    Some(&readiness_map),
                    Some(&server_display_names),
                    Some(&prefilled_params_by_server),
                    false,
                );
                payload["inactive_preview"] = json!(preview.tools);
                payload["hint"] = json!(
                    "No active tools matched, but ready-to-invoke tools exist in an unbound \
                     FeatureSet (see inactive_preview). Call mcpmux_bind_current_workspace with \
                     the bindable_feature_set_id shown on each preview entry to activate them."
                );
            }
            inactive_widen_ms = inactive_started.elapsed().as_millis() as u64;
            debug!(
                query_id = %query_id,
                ready_inactive_preview = payload
                    .get("inactive_preview")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0),
                inactive_widen_ms,
                "[search] zero-result inactive preview"
            );
        } else if include_inactive && result.total == 0 {
            let catalog = call
                .ctx
                .tool_discovery
                .build_catalog_index(&space_id.to_string())
                .await
                .map_err(|e| MetaToolError::Internal(e.to_string()))?;
            let catalog_result = crate::services::tool_discovery::ToolDiscoveryService::search(
                &catalog,
                effective_query,
                call.args.get("server_id").and_then(|v| v.as_str()),
                detail_level,
                limit,
                call.args.get("cursor").and_then(|v| v.as_str()),
                Some(query_id.as_str()),
                None,
                Some(&readiness_map),
                Some(&server_display_names),
                Some(&prefilled_params_by_server),
                is_browse,
            );
            if catalog_result.total > 0 {
                payload["hint"] = json!(
                    "Matching tools exist in this Space but no FeatureSet contains them. \
                     Ask the user to create a bundle in the McpMux desktop or web UI \
                     (Workspaces → Feature Sets), then mcpmux_bind_current_workspace \
                     with the new feature_set_id."
                );
            }
        }
        let post_ms = post_started.elapsed().as_millis() as u64;

        let total_ms = started.elapsed().as_millis() as u64;
        let accounted_ms = resolve_ms
            + active_index_ms
            + index_clone_ms
            + inactive_widen_ms
            + hydrate_ms
            + rank_ms
            + post_ms;

        info!(
            query_id = %query_id,
            ranking = result.ranking,
            total = result.total,
            returned = result.tools.len(),
            top_qualified_name,
            top_fused_score = ?result.top_fused_score,
            total_ms,
            "[search] result summary"
        );
        info!(
            query_id = %query_id,
            resolve_ms,
            active_index_ms,
            index_clone_ms,
            inactive_widen_ms,
            hydrate_ms,
            rank_ms,
            post_ms,
            accounted_ms,
            unaccounted_ms = total_ms.saturating_sub(accounted_ms),
            merged_index = index.len(),
            "[search] timing breakdown"
        );

        Ok(text_result(payload))
    }
}
