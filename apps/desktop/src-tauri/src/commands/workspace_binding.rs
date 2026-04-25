//! Tauri commands for workspace-root FeatureSet bindings.
//!
//! Every binding hard-pins a concrete (space_id, feature_set_id) pair. No
//! "follow active" modes — the mapping from root on disk to the toolset that
//! clients see is fully explicit, which is what our users actually want.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mcpmux_core::{
    validate_workspace_root as validate_root, DomainEvent, FeatureSet, FeatureSetType, MemberMode,
    MemberType, ServerFeature, WorkspaceBinding, WorkspaceRootValidation,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use uuid::Uuid;

use super::gateway::GatewayAppState;
use super::server_manager::ServerManagerState;
use crate::state::AppState;

/// Publish `WorkspaceBindingChanged` on the gateway's domain bus so
/// MCPNotifier broadcasts `list_changed` to every peer whose session now
/// routes through the changed binding.
///
/// Best-effort: gateway not running (no subscribers) is a normal condition
/// at startup and must not fail the command.
async fn emit_binding_changed(
    gateway_state: &Arc<RwLock<GatewayAppState>>,
    space_id: Uuid,
    workspace_root: String,
) {
    let gw_state = gateway_state.read().await;
    let Some(ref gw) = gw_state.gateway_state else {
        debug!("[workspace_binding] gateway not running — skipping emit");
        return;
    };
    gw.read()
        .await
        .emit_domain_event(DomainEvent::WorkspaceBindingChanged {
            space_id,
            workspace_root,
        });
}

/// DTO returned to the React layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBindingDto {
    pub id: String,
    pub workspace_root: String,
    pub space_id: String,
    pub feature_set_id: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<WorkspaceBinding> for WorkspaceBindingDto {
    fn from(b: WorkspaceBinding) -> Self {
        Self {
            id: b.id.to_string(),
            workspace_root: b.workspace_root,
            space_id: b.space_id.to_string(),
            feature_set_id: b.feature_set_id,
            created_at: b.created_at.to_rfc3339(),
            updated_at: b.updated_at.to_rfc3339(),
        }
    }
}

/// Input for creating or updating a binding. Both `space_id` (UUID) and
/// `feature_set_id` (stringy — custom sets use UUIDs, builtins use
/// `fs_default_<uuid>`) are required.
#[derive(Debug, Deserialize)]
pub struct WorkspaceBindingInput {
    pub workspace_root: String,
    pub space_id: String,
    pub feature_set_id: String,
}

fn parse_space_id(input: &WorkspaceBindingInput) -> Result<Uuid, String> {
    Uuid::parse_str(&input.space_id).map_err(|e| format!("bad space_id: {e}"))
}

fn validate_non_empty_fs(input: &WorkspaceBindingInput) -> Result<String, String> {
    if input.feature_set_id.trim().is_empty() {
        Err("feature_set_id required".into())
    } else {
        Ok(input.feature_set_id.clone())
    }
}

/// List every filesystem path connected MCP clients have reported as a
/// workspace root, deduplicated across sessions. The Workspaces tab
/// renders this next to the persisted bindings so users can configure
/// folders they missed the one-shot prompt for.
///
/// Returns an empty list when the gateway isn't running — that's a normal
/// startup condition, not an error.
#[tauri::command]
pub async fn list_reported_workspace_roots(
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<Vec<String>, String> {
    let guard = gateway_state.read().await;
    Ok(guard
        .session_roots
        .as_ref()
        .map(|reg| reg.list_all_roots())
        .unwrap_or_default())
}

/// List every binding (sorted by workspace_root).
#[tauri::command]
pub async fn list_workspace_bindings(
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceBindingDto>, String> {
    state
        .workspace_binding_repository
        .list()
        .await
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| {
            error!("[workspace_binding::list] {e}");
            e.to_string()
        })
}

/// Bindings whose target Space is the given one.
#[tauri::command]
pub async fn list_workspace_bindings_for_space(
    space_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceBindingDto>, String> {
    let space_uuid = Uuid::parse_str(&space_id).map_err(|e| e.to_string())?;
    state
        .workspace_binding_repository
        .list_for_space(&space_uuid)
        .await
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

/// Live path validation for the UI — returns `Ok(normalized)` or
/// `Err(reason)`. Runs the same rules the create/update commands apply, so
/// the form can show the real error message without round-tripping a save.
#[tauri::command]
pub async fn validate_workspace_root(path: String) -> Result<String, String> {
    match validate_root(&path) {
        WorkspaceRootValidation::Empty => Err(String::new()),
        WorkspaceRootValidation::Ok { normalized } => Ok(normalized),
        WorkspaceRootValidation::Invalid { reason } => Err(reason),
    }
}

/// Normalize + validate a manually-entered workspace root, returning the
/// canonical form to store. Rejects relative paths, filesystem roots, and
/// (for Windows-style paths) reserved characters — these are the exact
/// conditions that would produce a binding no session could ever match.
fn normalize_and_validate(raw: &str) -> Result<String, String> {
    match validate_root(raw) {
        WorkspaceRootValidation::Empty => Err("workspace_root cannot be empty".into()),
        WorkspaceRootValidation::Ok { normalized } => Ok(normalized),
        WorkspaceRootValidation::Invalid { reason } => Err(reason),
    }
}

/// Create a binding. Path is normalized + validated server-side so the UI
/// can pass raw input (Windows paths, file:// URIs, trailing slashes).
#[tauri::command]
pub async fn create_workspace_binding(
    input: WorkspaceBindingInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<WorkspaceBindingDto, String> {
    let space_id = parse_space_id(&input)?;
    let feature_set_id = validate_non_empty_fs(&input)?;
    let normalized = normalize_and_validate(&input.workspace_root)?;

    let binding = WorkspaceBinding::new(normalized.clone(), space_id, feature_set_id);

    state
        .workspace_binding_repository
        .create(&binding)
        .await
        .map_err(|e| e.to_string())?;

    info!(
        binding_id = %binding.id,
        root = %binding.workspace_root,
        %space_id,
        feature_set_id = %binding.feature_set_id,
        "[workspace_binding] created",
    );

    emit_binding_changed(
        gateway_state.inner(),
        binding.space_id,
        binding.workspace_root.clone(),
    )
    .await;
    Ok(binding.into())
}

/// Update an existing binding. Accepts full input so the UI can edit any
/// axis (root, target space, target FS) in one call.
#[tauri::command]
pub async fn update_workspace_binding(
    id: String,
    input: WorkspaceBindingInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<WorkspaceBindingDto, String> {
    let id_uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let space_id = parse_space_id(&input)?;
    let feature_set_id = validate_non_empty_fs(&input)?;
    let normalized = normalize_and_validate(&input.workspace_root)?;

    let existing = state
        .workspace_binding_repository
        .get(&id_uuid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("binding not found: {}", id))?;
    let old_space_id = existing.space_id;

    let updated = WorkspaceBinding {
        id: existing.id,
        workspace_root: normalized,
        space_id,
        feature_set_id,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now(),
    };

    state
        .workspace_binding_repository
        .update(&updated)
        .await
        .map_err(|e| e.to_string())?;

    // Notify the NEW target space first (peers that now route via this
    // binding). If the space changed, also notify the OLD target so peers
    // that resolved there lose the stale route.
    emit_binding_changed(
        gateway_state.inner(),
        updated.space_id,
        updated.workspace_root.clone(),
    )
    .await;
    if old_space_id != updated.space_id {
        emit_binding_changed(
            gateway_state.inner(),
            old_space_id,
            updated.workspace_root.clone(),
        )
        .await;
    }
    Ok(updated.into())
}

/// Delete a binding by id.
#[tauri::command]
pub async fn delete_workspace_binding(
    id: String,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<(), String> {
    let id_uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    // Capture the binding before delete so we know which space to notify.
    let existing = state
        .workspace_binding_repository
        .get(&id_uuid)
        .await
        .map_err(|e| e.to_string())?;

    state
        .workspace_binding_repository
        .delete(&id_uuid)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(b) = existing {
        emit_binding_changed(gateway_state.inner(), b.space_id, b.workspace_root).await;
    }
    Ok(())
}

// ============================================================================
// Workspace effective-features inspection
//
// Surfaces the same view the gateway resolver builds for live sessions, so
// the desktop UI can answer: "for this folder, what tools/prompts/resources
// would a connected client see right now — and which are configured-but-
// unavailable because their backend server is currently disconnected?"
//
// Pure read-only — no mutations, no event emission.
// ============================================================================

/// Per-feature view returned by `get_workspace_effective_features`.
///
/// `available` is `true` exactly when the underlying server is currently
/// connected. A `false` value with `server_status = "disconnected"`
/// (or `auth_required` / `error`) is the user's "configured but
/// unavailable" case — the FS still includes this feature, but its
/// server isn't usable right now so the gateway hides it from clients.
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveFeatureDto {
    pub id: String,
    pub feature_name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub server_id: String,
    pub server_alias: Option<String>,
    /// snake_case mirror of `mcpmux_gateway::pool::ConnectionStatus`, plus
    /// `unknown` when the gateway isn't running (so the UI can grey-out
    /// without lying about the cause).
    pub server_status: String,
    pub available: bool,
}

/// Top-level DTO: the resolved (Space, FeatureSet) pair for a given root,
/// plus its full configured tool/prompt/resource lists with availability.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEffectiveFeaturesDto {
    /// Normalized form of the input root (lower-case drive letter, no
    /// trailing slash, etc.).
    pub workspace_root: String,
    /// `binding` when a `WorkspaceBinding` matched the longest prefix of
    /// the root; `fallback` when no binding matched and the resolver fell
    /// through to the default Space's Default FS.
    pub source: String,
    /// `Some(id)` only when `source == "binding"`.
    pub binding_id: Option<String>,
    pub space_id: String,
    pub space_name: String,
    pub feature_set_id: String,
    pub feature_set_name: String,
    pub feature_set_type: String,
    /// Configured features by type (includes unavailable ones).
    pub tools: Vec<EffectiveFeatureDto>,
    pub prompts: Vec<EffectiveFeatureDto>,
    pub resources: Vec<EffectiveFeatureDto>,
}

/// Walk a FeatureSet's members (with nested-FS recursion) to compute the
/// allowed and excluded feature-id sets — same shape the gateway resolver
/// uses, but kept here so we can omit the `is_available` filter and surface
/// "configured but disconnected" features to the UI.
fn collect_member_ids(
    fs: &FeatureSet,
    fs_lookup: &HashMap<String, FeatureSet>,
    allowed: &mut HashSet<String>,
    excluded: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) {
    if !visited.insert(fs.id.clone()) {
        return; // cycle guard
    }
    for m in &fs.members {
        match m.member_type {
            MemberType::Feature => match m.mode {
                MemberMode::Include => {
                    allowed.insert(m.member_id.clone());
                }
                MemberMode::Exclude => {
                    excluded.insert(m.member_id.clone());
                }
            },
            MemberType::FeatureSet => {
                if let Some(nested) = fs_lookup.get(&m.member_id) {
                    collect_member_ids(nested, fs_lookup, allowed, excluded, visited);
                }
            }
        }
    }
}

fn server_status_str(status: mcpmux_gateway::ConnectionStatus) -> &'static str {
    use mcpmux_gateway::ConnectionStatus as S;
    match status {
        S::Disconnected => "disconnected",
        S::Connecting => "connecting",
        S::Connected => "connected",
        S::Refreshing => "refreshing",
        S::AuthRequired => "auth_required",
        S::Authenticating => "authenticating",
        S::Error => "error",
    }
}

fn enrich_feature(
    f: &ServerFeature,
    server_statuses: &HashMap<String, mcpmux_gateway::ConnectionStatus>,
    gateway_running: bool,
) -> EffectiveFeatureDto {
    let status = server_statuses.get(&f.server_id).copied();
    let server_status = match status {
        Some(s) => server_status_str(s).to_string(),
        // No status entry usually means "gateway not running yet". Fall
        // back to the cached `is_available` flag so the UI can still mark
        // unavailable features without claiming a status it doesn't know.
        None if !gateway_running => "unknown".to_string(),
        None => "disconnected".to_string(),
    };
    let available = matches!(status, Some(mcpmux_gateway::ConnectionStatus::Connected))
        || (!gateway_running && f.is_available);

    EffectiveFeatureDto {
        id: f.id.to_string(),
        feature_name: f.feature_name.clone(),
        display_name: f.display_name.clone(),
        description: f.description.clone(),
        server_id: f.server_id.clone(),
        server_alias: f.server_alias.clone(),
        server_status,
        available,
    }
}

/// Compute the resolved (Space, FeatureSet) for a workspace root and return
/// its full configured feature list with per-feature availability.
///
/// The frontend calls this from the Workspaces tab inspector to answer the
/// "what tools does this folder actually see?" question. It's safe to call
/// even when the gateway isn't running — we degrade gracefully to
/// `server_status = "unknown"` and lean on the cached `is_available` flag.
#[tauri::command]
pub async fn get_workspace_effective_features(
    workspace_root: String,
    state: State<'_, AppState>,
    sm_state: State<'_, Arc<RwLock<ServerManagerState>>>,
) -> Result<WorkspaceEffectiveFeaturesDto, String> {
    // 1. Normalize the input the same way the resolver does.
    let normalized = match validate_root(&workspace_root) {
        WorkspaceRootValidation::Empty => return Err("workspace_root cannot be empty".into()),
        WorkspaceRootValidation::Ok { normalized } => normalized,
        WorkspaceRootValidation::Invalid { reason } => return Err(reason),
    };

    // 2. Default Space — the routing fallback.
    let default_space = state
        .space_service
        .get_default()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No default Space configured")?;

    // 3. Tier 1: longest-prefix workspace binding match.
    let binding = state
        .workspace_binding_repository
        .find_longest_prefix_match(&default_space.id, std::slice::from_ref(&normalized))
        .await
        .map_err(|e| e.to_string())?;

    let (source, binding_id, space_id, fs_id) = match binding {
        Some(b) => (
            "binding".to_string(),
            Some(b.id.to_string()),
            b.space_id,
            b.feature_set_id,
        ),
        None => {
            let default_fs = state
                .feature_set_repository
                .get_default_for_space(&default_space.id.to_string())
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Default Space has no Default FeatureSet")?;
            (
                "fallback".to_string(),
                None,
                default_space.id,
                default_fs.id,
            )
        }
    };

    let space = state
        .space_service
        .get(&space_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Resolved Space no longer exists")?;

    // 4. The FS itself — with members for the walk below.
    let fs = state
        .feature_set_repository
        .get_with_members(&fs_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Resolved FeatureSet not found")?;

    // 5. Pre-fetch every FS in the same Space so nested-FS members can be
    //    resolved without N round trips. Cheap — this is just a metadata
    //    table and Spaces typically hold a handful of sets.
    let space_sets = state
        .feature_set_repository
        .list_by_space(&space_id.to_string())
        .await
        .map_err(|e| e.to_string())?;
    let mut fs_lookup: HashMap<String, FeatureSet> = HashMap::new();
    for sibling in space_sets {
        if let Ok(Some(full)) = state
            .feature_set_repository
            .get_with_members(&sibling.id)
            .await
        {
            fs_lookup.insert(full.id.clone(), full);
        }
    }
    fs_lookup.insert(fs.id.clone(), fs.clone());

    // 6. Walk members → allowed / excluded id sets.
    let mut allowed = HashSet::<String>::new();
    let mut excluded = HashSet::<String>::new();
    let mut visited = HashSet::<String>::new();
    collect_member_ids(&fs, &fs_lookup, &mut allowed, &mut excluded, &mut visited);

    // 7. Pull every feature in the Space, then keep only those that pass
    //    the FS filter — without the `is_available` gate, so we can show
    //    "configured but disconnected" rows.
    let all_features = state
        .server_feature_repository_core
        .list_for_space(&space_id.to_string())
        .await
        .map_err(|e| e.to_string())?;
    let filtered: Vec<ServerFeature> = all_features
        .into_iter()
        .filter(|f| {
            let fid = f.id.to_string();
            allowed.contains(&fid) && !excluded.contains(&fid)
        })
        .collect();

    // 8. Server statuses — only available when the gateway is running.
    let (server_statuses, gateway_running): (
        HashMap<String, mcpmux_gateway::ConnectionStatus>,
        bool,
    ) = {
        let sm = sm_state.read().await;
        match sm.manager.as_ref() {
            Some(mgr) => {
                let map = mgr
                    .get_all_statuses(space_id)
                    .await
                    .into_iter()
                    .map(|(id, (status, _, _, _))| (id, status))
                    .collect();
                (map, true)
            }
            None => (HashMap::new(), false),
        }
    };

    // 9. Bucket by feature type.
    let mut tools = Vec::new();
    let mut prompts = Vec::new();
    let mut resources = Vec::new();
    for f in &filtered {
        let dto = enrich_feature(f, &server_statuses, gateway_running);
        match f.feature_type {
            mcpmux_core::FeatureType::Tool => tools.push(dto),
            mcpmux_core::FeatureType::Prompt => prompts.push(dto),
            mcpmux_core::FeatureType::Resource => resources.push(dto),
        }
    }
    // Stable order: alphabetical by qualified-ish name so the UI doesn't
    // jitter between calls.
    let sort_key = |a: &EffectiveFeatureDto| {
        format!(
            "{}/{}",
            a.server_alias
                .clone()
                .unwrap_or_else(|| a.server_id.clone()),
            a.feature_name
        )
    };
    tools.sort_by_key(sort_key);
    prompts.sort_by_key(sort_key);
    resources.sort_by_key(sort_key);

    let feature_set_type = match fs.feature_set_type {
        FeatureSetType::Default => "default",
        FeatureSetType::Custom => "custom",
    };

    Ok(WorkspaceEffectiveFeaturesDto {
        workspace_root: normalized,
        source,
        binding_id,
        space_id: space_id.to_string(),
        space_name: space.name,
        feature_set_id: fs.id,
        feature_set_name: fs.name,
        feature_set_type: feature_set_type.to_string(),
        tools,
        prompts,
        resources,
    })
}
