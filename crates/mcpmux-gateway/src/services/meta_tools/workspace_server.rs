//! Workspace-scope enable/disable for MCP servers via binding FeatureSets.
//!
//! Persists a per-server "all tools" FeatureSet (tagged with
//! [`FeatureSet::server_id`]) and appends it to the matched
//! [`WorkspaceBinding`]'s `feature_set_ids`.

use mcpmux_core::{DomainEvent, FeatureSet, MemberMode, MemberType, WorkspaceBinding};
use rmcp::model::CallToolResult;
use serde_json::json;
use tokio::sync::broadcast;
use tracing::info;
use uuid::Uuid;

use super::registry::{MetaToolCall, MetaToolError};
use super::tools::{text_result, with_approval};

/// Whether a FeatureSet is the workspace-scoped "all tools for server" row.
fn is_server_all_feature_set(fs: &FeatureSet, server_id: &str) -> bool {
    !fs.is_deleted && fs.server_id.as_deref() == Some(server_id)
}

/// Resolve the workspace binding for the caller's first reported root.
async fn resolve_workspace_binding(
    call: &MetaToolCall<'_>,
    space_id: Uuid,
) -> Result<(WorkspaceBinding, String), MetaToolError> {
    let session_id = call
        .session_id
        .ok_or_else(|| MetaToolError::InvalidArgument("workspace scope requires an MCP session id".into()))?;
    let roots = call
        .ctx
        .session_roots
        .get(session_id)
        .unwrap_or_default();
    let root = roots.into_iter().next().ok_or_else(|| {
        MetaToolError::InvalidArgument(
            "caller did not report any MCP roots; cannot resolve workspace".into(),
        )
    })?;
    let normalized = mcpmux_core::normalize_workspace_root(&root);

    let binding = call
        .ctx
        .binding_repo
        .find_longest_prefix_match(&space_id, std::slice::from_ref(&normalized))
        .await?
        .ok_or_else(|| {
            MetaToolError::InvalidArgument(
                "no binding exists for this workspace; create one with \
                 mcpmux_create_feature_set + mcpmux_bind_current_workspace first"
                    .into(),
            )
        })?;
    Ok((binding, normalized))
}

fn emit_workspace_binding_changed(
    event_tx: &broadcast::Sender<DomainEvent>,
    space_id: Uuid,
    workspace_root: &str,
) {
    let _ = event_tx.send(DomainEvent::WorkspaceBindingChanged {
        space_id,
        workspace_root: workspace_root.to_string(),
    });
}

/// Enable `server_id` persistently on the caller's workspace binding.
pub async fn enable_workspace_server(
    call: MetaToolCall<'_>,
    space_id: Uuid,
    server_id: String,
) -> Result<CallToolResult, MetaToolError> {
    let (binding, workspace_root) = resolve_workspace_binding(&call, space_id).await?;
    let summary = format!(
        "Enable server '{server_id}' for workspace '{workspace_root}' (persists across sessions)"
    );

    let fs_repo = call.ctx.feature_set_repo.clone();
    let binding_repo = call.ctx.binding_repo.clone();
    let server_feature_repo = call.ctx.server_feature_repo.clone();
    let event_tx = call.ctx.domain_event_tx.clone();
    let args = call.args.clone();

    let mut binding_for_closure = binding.clone();
    let workspace_root_for_closure = workspace_root.clone();

    with_approval(
        &call,
        "mcpmux_enable_server",
        summary,
        None,
        true,
        args,
        || async move {
            let existing = {
                let sets = fs_repo.list_by_space(&space_id.to_string()).await?;
                Ok::<_, MetaToolError>(
                    sets.into_iter()
                        .find(|fs| is_server_all_feature_set(fs, &server_id)),
                )
            }?;

            let fs_id = if let Some(fs) = existing {
                fs.id
            } else {
                let mut fs =
                    FeatureSet::new_custom(format!("{server_id} — All"), space_id.to_string());
                fs.server_id = Some(server_id.clone());
                fs.description = Some(format!("All tools from {server_id} (workspace scope)"));

                let features = server_feature_repo
                    .list_for_space(&space_id.to_string())
                    .await?
                    .into_iter()
                    .filter(|f| f.server_id == server_id)
                    .collect::<Vec<_>>();

                fs_repo.create(&fs).await?;
                for feature in &features {
                    fs_repo
                        .add_feature_member(&fs.id, &feature.id.to_string(), MemberMode::Include)
                        .await?;
                }
                fs.id
            };

            if binding_for_closure
                .feature_set_ids
                .iter()
                .any(|id| id == &fs_id)
            {
                info!(
                    binding_id = %binding_for_closure.id,
                    server_id = %server_id,
                    "[meta_tools] enable_server workspace already bound"
                );
                return Ok(text_result(json!({
                    "ok": true,
                    "server_id": server_id,
                    "scope": "workspace",
                    "feature_set_id": fs_id,
                    "binding_id": binding_for_closure.id,
                })));
            }

            binding_for_closure.feature_set_ids.push(fs_id.clone());
            binding_for_closure.updated_at = chrono::Utc::now();
            binding_repo.update(&binding_for_closure).await?;
            emit_workspace_binding_changed(&event_tx, space_id, &workspace_root_for_closure);
            info!(
                binding_id = %binding_for_closure.id,
                feature_set_id = %fs_id,
                server_id = %server_id,
                "[meta_tools] enable_server workspace applied"
            );
            Ok(text_result(json!({
                "ok": true,
                "server_id": server_id,
                "scope": "workspace",
                "feature_set_id": fs_id,
                "binding_id": binding_for_closure.id,
            })))
        },
    )
    .await
}

/// Returns true when `server_id` tools are exposed via a non-server-all FS on the binding.
async fn binding_exposes_server_via_custom_fs(
    call: &MetaToolCall<'_>,
    binding: &WorkspaceBinding,
    space_id: &str,
    server_id: &str,
) -> Result<bool, MetaToolError> {
    for fs_id in &binding.feature_set_ids {
        let Some(fs) = call.ctx.feature_set_repo.get(fs_id).await? else {
            continue;
        };
        if is_server_all_feature_set(&fs, server_id) {
            continue;
        }
        let members = call.ctx.feature_set_repo.get_feature_members(fs_id).await?;
        for member in members {
            if member.member_type != MemberType::Feature {
                continue;
            }
            let Ok(feature_id) = Uuid::parse_str(&member.member_id) else {
                continue;
            };
            if let Some(feature) = call.ctx.server_feature_repo.get(&feature_id).await? {
                if feature.space_id == space_id && feature.server_id == server_id {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Disable `server_id` on the caller's workspace binding (server-all FS only).
pub async fn disable_workspace_server(
    call: MetaToolCall<'_>,
    space_id: Uuid,
    server_id: String,
) -> Result<CallToolResult, MetaToolError> {
    let (binding, workspace_root) = resolve_workspace_binding(&call, space_id).await?;

    if binding_exposes_server_via_custom_fs(&call, &binding, &space_id.to_string(), &server_id)
        .await?
    {
        return Err(MetaToolError::InvalidArgument(format!(
            "server '{server_id}' is enabled via a custom FeatureSet on this binding; \
             edit or remove it in the Workspaces UI instead"
        )));
    }

    let server_all_id = {
        let mut found: Option<String> = None;
        for fs_id in &binding.feature_set_ids {
            if let Some(fs) = call.ctx.feature_set_repo.get(fs_id).await? {
                if is_server_all_feature_set(&fs, &server_id) {
                    found = Some(fs_id.clone());
                    break;
                }
            }
        }
        found
    };

    let Some(server_all_id) = server_all_id else {
        return Ok(text_result(json!({
            "ok": true,
            "server_id": server_id,
            "scope": "workspace",
            "removed": false,
        })));
    };

    let summary = format!(
        "Disable server '{server_id}' for workspace '{workspace_root}' (persistent binding change)"
    );
    let binding_repo = call.ctx.binding_repo.clone();
    let event_tx = call.ctx.domain_event_tx.clone();
    let mut binding_for_closure = binding.clone();
    let args = call.args.clone();

    with_approval(
        &call,
        "mcpmux_disable_server",
        summary,
        None,
        true,
        args,
        || async move {
            binding_for_closure
                .feature_set_ids
                .retain(|id| id != &server_all_id);
            binding_for_closure.updated_at = chrono::Utc::now();
            binding_repo.update(&binding_for_closure).await?;
            emit_workspace_binding_changed(&event_tx, space_id, &workspace_root);
            info!(
                binding_id = %binding_for_closure.id,
                feature_set_id = %server_all_id,
                server_id = %server_id,
                "[meta_tools] disable_server workspace applied"
            );
            Ok(text_result(json!({
                "ok": true,
                "server_id": server_id,
                "scope": "workspace",
                "removed": true,
                "feature_set_id": server_all_id,
                "binding_id": binding_for_closure.id,
            })))
        },
    )
    .await
}
