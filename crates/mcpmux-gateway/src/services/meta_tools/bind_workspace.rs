//! `mcpmux_bind_current_workspace` — persistently layer a FeatureSet onto a workspace binding.

use async_trait::async_trait;
use mcpmux_core::{normalize_workspace_root, WorkspaceBinding, WorkspaceBindingRepository};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;

use super::meta_tool_common::{
    caller_space_id, emit_tools_list_changed, emit_workspace_binding_changed, parse_uuid_arg,
    text_result, with_approval,
};
use super::registry::{MetaTool, MetaToolCall, MetaToolError};

pub struct BindCurrentWorkspaceTool;

/// Machine identity for this bind — header, then OAuth client machine, then gateway local.
async fn effective_machine_for_bind(
    call: &MetaToolCall<'_>,
) -> Result<Option<Uuid>, MetaToolError> {
    call.ctx
        .resolver
        .effective_machine_id(Some(call.client_id), call.request_machine_id)
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))
}

/// Look up an existing binding row using the same scope the write path will use.
async fn find_existing_binding_for_bind(
    binding_repo: &dyn WorkspaceBindingRepository,
    space_id: &Uuid,
    machine_id: Option<Uuid>,
    client_id: &str,
    normalized: &str,
) -> Result<Option<WorkspaceBinding>, MetaToolError> {
    if let Some(mid) = machine_id {
        return binding_repo
            .find_exact_for_machine(&mid, normalized, None)
            .await
            .map_err(|e| MetaToolError::Internal(e.to_string()));
    }
    binding_repo
        .find_longest_prefix_match(space_id, Some(client_id), &[normalized.to_string()])
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))
}

/// Inputs for the bind tool JSON response (post-write re-resolve for `active`).
struct BindToolResultInput<'a> {
    resolver: &'a crate::services::FeatureSetResolverService,
    session_id: Option<&'a str>,
    client_id: &'a str,
    request_machine_id: Option<Uuid>,
    binding_id: Uuid,
    workspace_root: &'a str,
    feature_set_id: Uuid,
    feature_set_ids: Vec<String>,
    already_bound: bool,
    machine_id: Option<Uuid>,
}

async fn bind_tool_result(input: BindToolResultInput<'_>) -> Result<CallToolResult, MetaToolError> {
    let fs_id_str = input.feature_set_id.to_string();
    let resolved = input
        .resolver
        .resolve(
            input.session_id,
            Some(input.client_id),
            input.request_machine_id,
        )
        .await
        .map_err(|e| MetaToolError::Internal(e.to_string()))?;
    let active = resolved.feature_set_ids.iter().any(|id| id == &fs_id_str);

    let mut body = json!({
        "ok": true,
        "binding_id": input.binding_id,
        "workspace_root": input.workspace_root,
        "feature_set_id": input.feature_set_id,
        "feature_set_ids": input.feature_set_ids,
        "already_bound": input.already_bound,
        "active": active,
    });
    if let Some(mid) = input.machine_id {
        body["machine_id"] = json!(mid);
    }
    if !active {
        body["note"] = json!(
            "binding persisted but FeatureSet is not active for this session — \
             verify machine identity (Settings → Machine Identity) and \
             X-Mcpmux-Machine-Id on tunneled clients"
        );
    }
    Ok(text_result(body))
}

#[async_trait]
impl MetaTool for BindCurrentWorkspaceTool {
    fn name(&self) -> &'static str {
        "mcpmux_bind_current_workspace"
    }

    fn description(&self) -> &'static str {
        "Canonical activation path: persistently append an existing FeatureSet \
         onto the caller's workspace binding (layers with existing bundles, \
         deduped). Use after mcpmux_search_tools (include_inactive: true) or \
         mcpmux_list_feature_sets to obtain feature_set_id. Every future \
         connection reporting the same root inherits the binding. Requires \
         approval; the client MUST have declared MCP roots."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["feature_set_id"],
            "properties": {
                "feature_set_id": { "type": "string" }
            }
        })
    }

    fn is_write(&self) -> bool {
        true
    }

    async fn call(&self, call: MetaToolCall<'_>) -> Result<CallToolResult, MetaToolError> {
        let fs_id = parse_uuid_arg(&call.args, "feature_set_id")?;

        let space_id = caller_space_id(&call).await?;
        let machine_id = effective_machine_for_bind(&call).await?;
        let roots = call
            .session_id
            .and_then(|sid| call.ctx.session_roots.get(sid))
            .unwrap_or_default();
        let root = roots.into_iter().next().ok_or_else(|| {
            MetaToolError::InvalidArgument(
                "caller did not report any MCP roots; cannot bind — \
                 call mcpmux_set_workspace_root first to declare your workspace path, \
                 then retry mcpmux_bind_current_workspace"
                    .into(),
            )
        })?;
        let normalized = normalize_workspace_root(&root);

        let fs_name = call
            .ctx
            .feature_set_repo
            .get(&fs_id.to_string())
            .await?
            .map(|fs| fs.name)
            .unwrap_or_else(|| fs_id.to_string());

        let binding_repo = call.ctx.binding_repo.clone();
        let fs_id_str = fs_id.to_string();
        let caller_client_id = call.client_id.to_string();

        // Dedup before consent: repeat binds must not re-prompt the user.
        if let Some(existing) = find_existing_binding_for_bind(
            binding_repo.as_ref(),
            &space_id,
            machine_id,
            &caller_client_id,
            &normalized,
        )
        .await?
        {
            if existing.feature_set_ids.iter().any(|id| id == &fs_id_str) {
                return bind_tool_result(BindToolResultInput {
                    resolver: call.ctx.resolver.as_ref(),
                    session_id: call.session_id,
                    client_id: call.client_id,
                    request_machine_id: call.request_machine_id,
                    binding_id: existing.id,
                    workspace_root: &normalized,
                    feature_set_id: fs_id,
                    feature_set_ids: existing.feature_set_ids,
                    already_bound: true,
                    machine_id,
                })
                .await;
            }
        }

        let summary = match machine_id {
            Some(mid) => format!(
                "Append FeatureSet '{fs_name}' to workspace '{normalized}' binding \
                 for machine '{mid}' (existing bundles preserved)."
            ),
            None => format!(
                "Append FeatureSet '{fs_name}' to workspace '{normalized}' binding \
                 for client '{caller_client_id}' (existing bundles preserved)."
            ),
        };

        let event_tx = call.ctx.domain_event_tx.clone();
        let resolver = call.ctx.resolver.clone();
        let session_id_owned = call.session_id.map(str::to_owned);
        let caller_client_id_for_response = caller_client_id.clone();
        let request_machine_id = call.request_machine_id;
        with_approval(
            &call,
            "mcpmux_bind_current_workspace",
            summary,
            None,
            true,
            call.args.clone(),
            || async move {
                let existing = find_existing_binding_for_bind(
                    binding_repo.as_ref(),
                    &space_id,
                    machine_id,
                    &caller_client_id,
                    &normalized,
                )
                .await?;

                let (binding_id, feature_set_ids, already_bound) = if let Some(mut binding) =
                    existing
                {
                    binding.space_id = space_id;
                    let already_bound = binding.feature_set_ids.iter().any(|id| id == &fs_id_str);
                    if !already_bound {
                        binding.feature_set_ids.push(fs_id_str.clone());
                        binding.updated_at = chrono::Utc::now();
                        binding_repo.update(&binding).await?;
                        emit_workspace_binding_changed(&event_tx, space_id, &normalized);
                    }
                    info!(
                        %space_id,
                        client_id = %caller_client_id,
                        ?machine_id,
                        binding_id = %binding.id,
                        workspace_root = %normalized,
                        feature_set_id = %fs_id,
                        already_bound,
                        feature_set_count = binding.feature_set_ids.len(),
                        "[meta_tools] bind_current_workspace updated existing binding",
                    );
                    (binding.id, binding.feature_set_ids.clone(), already_bound)
                } else if let Some(mid) = machine_id {
                    let binding = WorkspaceBinding::new_machine_scoped_multi(
                        normalized.clone(),
                        space_id,
                        mid,
                        vec![fs_id_str.clone()],
                    );
                    let binding_id = binding.id;
                    let feature_set_ids = binding.feature_set_ids.clone();
                    binding_repo.create(&binding).await?;
                    emit_workspace_binding_changed(&event_tx, space_id, &normalized);
                    info!(
                        %space_id,
                        %mid,
                        binding_id = %binding_id,
                        workspace_root = %normalized,
                        feature_set_id = %fs_id,
                        "[meta_tools] bind_current_workspace created machine-scoped binding",
                    );
                    (binding_id, feature_set_ids, false)
                } else {
                    let binding = WorkspaceBinding::new_scoped_multi(
                        normalized.clone(),
                        space_id,
                        Some(caller_client_id.clone()),
                        vec![fs_id_str.clone()],
                    );
                    let binding_id = binding.id;
                    let feature_set_ids = binding.feature_set_ids.clone();
                    binding_repo.create(&binding).await?;
                    emit_workspace_binding_changed(&event_tx, space_id, &normalized);
                    info!(
                        %space_id,
                        client_id = %caller_client_id,
                        binding_id = %binding_id,
                        workspace_root = %normalized,
                        feature_set_id = %fs_id,
                        "[meta_tools] bind_current_workspace created client-scoped binding",
                    );
                    (binding_id, feature_set_ids, false)
                };

                if !already_bound {
                    emit_tools_list_changed(&event_tx, space_id);
                }
                bind_tool_result(BindToolResultInput {
                    resolver: resolver.as_ref(),
                    session_id: session_id_owned.as_deref(),
                    client_id: &caller_client_id_for_response,
                    request_machine_id,
                    binding_id,
                    workspace_root: &normalized,
                    feature_set_id: fs_id,
                    feature_set_ids,
                    already_bound,
                    machine_id,
                })
                .await
            },
        )
        .await
    }
}
