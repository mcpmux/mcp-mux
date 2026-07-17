//! Tauri commands for workspace-root FeatureSet bindings.
//!
//! Every binding hard-pins a concrete (space_id, feature_set_id) pair. No
//! "follow active" modes — the mapping from root on disk to the toolset that
//! clients see is fully explicit, which is what our users actually want.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mcpmux_core::{
    normalize_optional_metadata, validate_workspace_root as validate_root, BindingType, DomainEvent,
    FeatureSet, FeatureSetType, MemberMode, MemberType, ServerFeature, WorkspaceBinding,
    WorkspaceRootValidation,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use uuid::Uuid;

use super::gateway::GatewayAppState;
use super::server_manager::ServerManagerState;
use super::workspace_appearance::maybe_remove_orphaned_icon_file;
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
///
/// `feature_set_ids` is non-empty by construction — empty bindings are
/// rejected at the create/update commands. Order is the operator-chosen
/// rendering order; the resolver treats the list as a set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBindingDto {
    pub id: String,
    pub workspace_root: String,
    pub label: Option<String>,
    pub icon: Option<String>,
    pub space_id: String,
    pub feature_set_ids: Vec<String>,
    pub machine_id: Option<String>,
    #[serde(default)]
    pub binding_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<WorkspaceBinding> for WorkspaceBindingDto {
    fn from(b: WorkspaceBinding) -> Self {
        Self {
            id: b.id.to_string(),
            workspace_root: b.workspace_root,
            label: b.label,
            icon: b.icon,
            space_id: b.space_id.to_string(),
            feature_set_ids: b.feature_set_ids,
            machine_id: b.machine_id.map(|id| id.to_string()),
            binding_type: Some(b.binding_type.as_db_str().to_string()),
            created_at: b.created_at.to_rfc3339(),
            updated_at: b.updated_at.to_rfc3339(),
        }
    }
}

/// Input for creating or updating a binding.
///
/// `feature_set_ids` MAY be empty — an empty list means "this folder gets no
/// Space tools" (built-in servers still apply per Space). Order matters for UI
/// rendering only; the resolver merges them.
#[derive(Debug, Deserialize)]
pub struct WorkspaceBindingInput {
    pub workspace_root: String,
    pub label: Option<String>,
    pub icon: Option<String>,
    pub space_id: String,
    pub feature_set_ids: Vec<String>,
    #[serde(default)]
    pub machine_id: Option<String>,
    #[serde(default)]
    pub binding_type: Option<String>,
}

fn parse_optional_machine_id(value: Option<&str>) -> Result<Option<Uuid>, String> {
    match value {
        None | Some("") => Ok(None),
        Some(raw) => Uuid::parse_str(raw)
            .map(Some)
            .map_err(|e| format!("bad machine_id: {e}")),
    }
}

/// True when two bindings would collide on the partial unique indexes.
fn binding_scope_conflicts(
    existing: &WorkspaceBinding,
    root: &str,
    machine_id: Option<Uuid>,
    client_id: Option<&str>,
) -> bool {
    if existing.workspace_root != root {
        return false;
    }
    existing.machine_id == machine_id && existing.client_id.as_deref() == client_id
}

fn parse_binding_type(value: Option<&str>) -> BindingType {
    match value {
        Some("id") => BindingType::Id,
        _ => BindingType::Path,
    }
}

fn parse_space_id(input: &WorkspaceBindingInput) -> Result<Uuid, String> {
    Uuid::parse_str(&input.space_id).map_err(|e| format!("bad space_id: {e}"))
}

/// Resolve label from input, preserving existing on update when omitted.
fn resolve_binding_label(
    input: &WorkspaceBindingInput,
    existing: Option<&WorkspaceBinding>,
) -> Option<String> {
    if input.label.is_some() {
        normalize_optional_metadata(&input.label)
    } else {
        existing.and_then(|b| b.label.clone())
    }
}

/// Resolve icon from input, existing row, or unmapped appearance fallback.
async fn resolve_binding_icon(
    state: &AppState,
    normalized_root: &str,
    input: &WorkspaceBindingInput,
    existing: Option<&WorkspaceBinding>,
) -> Result<Option<String>, String> {
    let mut icon = if input.icon.is_some() {
        normalize_optional_metadata(&input.icon)
    } else {
        existing.and_then(|b| b.icon.clone())
    };
    if icon.is_none() {
        if let Some(appearance) = state
            .workspace_appearance_repository
            .get(normalized_root)
            .await
            .map_err(|e| e.to_string())?
        {
            icon = Some(appearance.icon);
        }
    }
    Ok(icon)
}

/// Drop appearance rows once a binding owns the root.
async fn clear_appearance_for_bound_root(
    state: &AppState,
    normalized_root: &str,
) -> Result<(), String> {
    if state
        .workspace_appearance_repository
        .get(normalized_root)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        state
            .workspace_appearance_repository
            .delete(normalized_root)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Clean + dedup the feature-set list (preserving order). An empty result is
/// valid — it persists as a "no Space tools" binding.
fn validate_fs_list(input: &WorkspaceBindingInput) -> Result<Vec<String>, String> {
    let cleaned = input
        .feature_set_ids
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // Dedup while preserving order so the operator's intent ("primary then
    // overlay") survives a duplicate they may have accidentally supplied.
    let mut seen = HashSet::new();
    let deduped: Vec<String> = cleaned.filter(|id| seen.insert(id.clone())).collect();
    Ok(deduped)
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

/// Forget every reported workspace root that has no binding ("unmapped").
///
/// The Workspaces tab surfaces folders connected clients reported but that
/// aren't mapped to a FeatureSet yet. This drops them from the in-memory
/// session-roots registry so the "Unmapped" list clears in one action; the
/// next time those sessions report a root (or reconnect) the resolver lands
/// on `Deny` again and re-fires the "map this folder?" prompt. Mapped roots
/// are left untouched.
///
/// Returns the number of distinct roots cleared. A not-running gateway has
/// nothing reported, so it returns `0` rather than erroring.
#[tauri::command]
pub async fn clear_unmapped_reported_roots(
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<usize, String> {
    // Snapshot the bound roots (case-folded) so we can tell a mapped folder
    // from an unmapped one — the same exact-match rule the Workspaces tab
    // uses to label a card "Unmapped".
    let bound: HashSet<String> = state
        .workspace_binding_repository
        .list()
        .await
        .map_err(|e| {
            error!("[workspace_binding::clear_unmapped] {e}");
            e.to_string()
        })?
        .into_iter()
        .map(|b| b.workspace_root.to_lowercase())
        .collect();

    let guard = gateway_state.read().await;
    let Some(reg) = guard.session_roots.as_ref() else {
        // Gateway not running — nothing has been reported.
        return Ok(0);
    };
    let dropped = reg.forget_unmapped_roots(|root| bound.contains(&root.to_lowercase()));
    let count = dropped.len();

    if count > 0 {
        info!(count, roots = ?dropped, "[workspace_binding] cleared unmapped reported roots");
        // Nudge the Workspaces tab to re-read `list_reported_workspace_roots`.
        if let Some(ref gw) = guard.gateway_state {
            gw.read()
                .await
                .emit_domain_event(DomainEvent::SessionRootsChanged);
        }
    }
    Ok(count)
}

/// Remove a single reported workspace root from the session registry.
///
/// Drops the root from every active MCP session that holds it, evicting
/// sessions left with no roots. Unlike `clear_unmapped_reported_roots` this
/// targets one specific path regardless of whether it has a binding. Returns
/// `true` when the root was found and removed, `false` when the gateway is not
/// running or the root wasn't in any session.
#[tauri::command]
pub async fn forget_reported_root(
    root: String,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<bool, String> {
    let guard = gateway_state.read().await;
    let Some(reg) = guard.session_roots.as_ref() else {
        return Ok(false);
    };
    let found = reg.forget_root(&root);
    if found {
        if let Some(ref gw) = guard.gateway_state {
            gw.read()
                .await
                .emit_domain_event(DomainEvent::SessionRootsChanged);
        }
    }
    Ok(found)
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
    let feature_set_ids = validate_fs_list(&input)?;
    let binding_type = parse_binding_type(input.binding_type.as_deref());
    let machine_id = parse_optional_machine_id(input.machine_id.as_deref())?;

    let (normalized, binding) = if binding_type == BindingType::Id {
        let client_key = input.workspace_root.trim();
        if client_key.is_empty() {
            return Err("client id cannot be empty".into());
        }
        let mut binding = WorkspaceBinding::new_id_multi(client_key, space_id, feature_set_ids);
        binding.machine_id = machine_id;
        binding.label = resolve_binding_label(&input, None);
        binding.icon = normalize_optional_metadata(&input.icon);
        (client_key.to_string(), binding)
    } else {
        let normalized = normalize_and_validate(&input.workspace_root)?;
        let binding = WorkspaceBinding {
            id: Uuid::new_v4(),
            workspace_root: normalized.clone(),
            binding_type: BindingType::Path,
            client_id: None,
            machine_id,
            label: resolve_binding_label(&input, None),
            icon: resolve_binding_icon(&state, &normalized, &input, None).await?,
            space_id,
            feature_set_ids,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        (normalized, binding)
    };

    // Reject a duplicate folder up front with a readable message. The schema
    // already enforces `UNIQUE(workspace_root)`, but that surfaces an opaque
    // SQLite constraint error — this gives the UI something a user can act on.

    let existing = state
        .workspace_binding_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;
    if existing.iter().any(|b| {
        b.binding_type == binding_type
            && binding_scope_conflicts(b, &normalized, machine_id, None)
    }) {
        let noun = if binding_type == BindingType::Id {
            "client mapping"
        } else {
            "mapping"
        };
        return Err(format!(
            "A {noun} already exists for {normalized}. Edit the existing mapping instead of adding a second one."
        ));
    }

    state
        .workspace_binding_repository
        .create(&binding)
        .await
        .map_err(|e| e.to_string())?;

    if binding_type == BindingType::Path {
        clear_appearance_for_bound_root(&state, &normalized).await?;
    }

    info!(
        binding_id = %binding.id,
        root = %binding.workspace_root,
        ?binding_type,
        %space_id,
        feature_sets = ?binding.feature_set_ids,
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

#[tauri::command]
pub async fn update_workspace_binding(
    id: String,
    input: WorkspaceBindingInput,
    state: State<'_, AppState>,
    gateway_state: State<'_, Arc<RwLock<GatewayAppState>>>,
) -> Result<WorkspaceBindingDto, String> {
    let id_uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let space_id = parse_space_id(&input)?;
    let feature_set_ids = validate_fs_list(&input)?;
    let binding_type = parse_binding_type(input.binding_type.as_deref());
    let machine_id = parse_optional_machine_id(input.machine_id.as_deref())?;

    let existing = state
        .workspace_binding_repository
        .get(&id_uuid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("binding not found: {}", id))?;

    let normalized = if binding_type == BindingType::Id {
        let client_key = input.workspace_root.trim();
        if client_key.is_empty() {
            return Err("client id cannot be empty".into());
        }
        client_key.to_string()
    } else {
        normalize_and_validate(&input.workspace_root)?
    };

    // If the edit moved the folder onto a path another mapping already owns,
    // reject with a readable message rather than tripping the DB UNIQUE
    // constraint. Exclude this binding's own row.
    let all = state
        .workspace_binding_repository
        .list()
        .await
        .map_err(|e| e.to_string())?;
    if all.iter().any(|b| {
        b.id != id_uuid
            && b.binding_type == binding_type
            && binding_scope_conflicts(b, &normalized, machine_id, None)
    }) {
        return Err(format!(
            "Another mapping already uses {normalized}. Pick a different folder."
        ));
    }

    let old_space_id = existing.space_id;
    let previous_icon = existing.icon.clone();
    let client_id = existing.client_id.clone();
    let label = resolve_binding_label(&input, Some(&existing));
    let icon = if binding_type == BindingType::Id {
        if input.icon.is_some() {
            normalize_optional_metadata(&input.icon)
        } else {
            existing.icon.clone()
        }
    } else {
        resolve_binding_icon(&state, &normalized, &input, Some(&existing)).await?
    };

    let updated = WorkspaceBinding {
        id: existing.id,
        workspace_root: normalized.clone(),
        binding_type,
        client_id,
        machine_id,
        label,
        icon,
        space_id,
        feature_set_ids,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now(),
    };

    state
        .workspace_binding_repository
        .update(&updated)
        .await
        .map_err(|e| e.to_string())?;

    if binding_type == BindingType::Path {
        clear_appearance_for_bound_root(&state, &normalized).await?;
    }

    if previous_icon.as_deref() != updated.icon.as_deref() {
        maybe_remove_orphaned_icon_file(&state, previous_icon.as_deref()).await?;
    }

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
        maybe_remove_orphaned_icon_file(&state, b.icon.as_deref()).await?;
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

/// Per-server total counts in the resolved Space, regardless of the
/// FeatureSet filter. The UI shows badges like "3 / {total}" — the right
/// side is the total the server exposes in the Space, so the user can see
/// "this FS includes 3 of the 10 cloudflare-docs tools available."
#[derive(Debug, Clone, Serialize)]
pub struct ServerFeatureTotalsDto {
    pub tools: usize,
    pub prompts: usize,
    pub resources: usize,
}

/// One FeatureSet that the binding resolves through. The Workspaces UI
/// renders these as a chip strip ("FS-A + FS-B"); the resolver merges
/// their members into a single allow set.
#[derive(Debug, Clone, Serialize)]
pub struct EffectiveFeatureSetDto {
    pub id: String,
    pub name: String,
    /// `default` | `custom` — matches `FeatureSetType`.
    pub feature_set_type: String,
}

/// Top-level DTO: the resolved (Space, FeatureSet…) for a given root,
/// plus the union of their tool/prompt/resource lists with availability.
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEffectiveFeaturesDto {
    /// Normalized form of the input root (lower-case drive letter, no
    /// trailing slash, etc.).
    pub workspace_root: String,
    /// `binding` when a `WorkspaceBinding` matched the longest prefix of
    /// the root; `unbound` when no binding matched. An `unbound` folder is
    /// **not** denied — it falls back to the default Space's Starter FS, so
    /// the `feature_sets` field below is exactly what a live session for this
    /// folder sees right now (the active routing target), until the user
    /// attaches an explicit binding to override it.
    pub source: String,
    /// `Some(id)` only when `source == "binding"`.
    pub binding_id: Option<String>,
    pub space_id: String,
    pub space_name: String,
    /// All FeatureSets contributing to the resolved view, in
    /// operator-chosen order. Always ≥ 1 entry (resolved or preview).
    pub feature_sets: Vec<EffectiveFeatureSetDto>,
    /// Configured features (union across all `feature_sets`) by type;
    /// includes unavailable ones for the "configured but disconnected"
    /// rendering case.
    pub tools: Vec<EffectiveFeatureDto>,
    pub prompts: Vec<EffectiveFeatureDto>,
    pub resources: Vec<EffectiveFeatureDto>,
    /// `server_id -> totals` over every feature the server exposes in the
    /// resolved Space (no FS filter applied). Used by the UI to render
    /// "{mapped} / {server total}" badges.
    pub server_totals: HashMap<String, ServerFeatureTotalsDto>,
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
    machine_id: Option<String>,
    state: State<'_, AppState>,
    sm_state: State<'_, Arc<RwLock<ServerManagerState>>>,
) -> Result<WorkspaceEffectiveFeaturesDto, String> {
    // 1. Normalize the input the same way the resolver does.
    let normalized = match validate_root(&workspace_root) {
        WorkspaceRootValidation::Empty => return Err("workspace_root cannot be empty".into()),
        WorkspaceRootValidation::Ok { normalized } => normalized,
        WorkspaceRootValidation::Invalid { reason } => return Err(reason),
    };

    let parsed_machine_id = parse_optional_machine_id(machine_id.as_deref())?;

    // 2. Default Space — the routing fallback.
    let default_space = state
        .space_service
        .get_default()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No default Space configured")?;

    // 3. Binding lookup: machine-scoped when machine_id is set, else legacy exact match.
    let binding = if let Some(mid) = parsed_machine_id {
        match state
            .workspace_binding_repository
            .find_exact_for_machine(&mid, &normalized, None)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(b) => Some(b),
            None => state
                .workspace_binding_repository
                .find_exact_global(&normalized)
                .await
                .map_err(|e| e.to_string())?,
        }
    } else {
        state
            .workspace_binding_repository
            .find_exact_for_roots(std::slice::from_ref(&normalized))
            .await
            .map_err(|e| e.to_string())?
    };

    let (source, binding_id, space_id, fs_ids) = match binding {
        Some(b) => (
            "binding".to_string(),
            Some(b.id.to_string()),
            b.space_id,
            b.feature_set_ids,
        ),
        None => {
            // Source = `unbound` mirrors the resolver: an unmapped folder
            // falls back to the default Space's Starter FS. This is the
            // active routing target a live session here resolves to, not a
            // hypothetical preview — the user can attach a binding to give
            // the folder something other than the default.
            let starter_fs = state
                .feature_set_repository
                .get_starter_for_space(&default_space.id.to_string())
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Default Space has no Starter FeatureSet")?;
            (
                "unbound".to_string(),
                None,
                default_space.id,
                vec![starter_fs.id],
            )
        }
    };

    let space = state
        .space_service
        .get(&space_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Resolved Space no longer exists")?;

    // 4. Resolve every FeatureSet the binding points to (preserving order)
    //    so we can walk their members below for the union allow set.
    let mut resolved_sets: Vec<FeatureSet> = Vec::with_capacity(fs_ids.len());
    for fs_id in &fs_ids {
        let fs = state
            .feature_set_repository
            .get_with_members(fs_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Resolved FeatureSet {fs_id} not found"))?;
        resolved_sets.push(fs);
    }

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
    for fs in &resolved_sets {
        fs_lookup.insert(fs.id.clone(), fs.clone());
    }

    // 6. Walk every FS in the binding → union allow set, union exclude set.
    //    Excludes win over includes within a single FS (collect_member_ids
    //    contract); when multiple FSes disagree we keep the include because
    //    the user's intent for adding the FS to the binding was to surface
    //    its members. Visiting state is shared across the loop so a nested
    //    FS shared between two parent FSes is walked once.
    let mut allowed = HashSet::<String>::new();
    let mut excluded = HashSet::<String>::new();
    let mut visited = HashSet::<String>::new();
    for fs in &resolved_sets {
        collect_member_ids(fs, &fs_lookup, &mut allowed, &mut excluded, &mut visited);
    }
    // Cross-FS exclude → include resolution: if any FS lists the feature as
    // an explicit include, override an exclude from a sibling FS. This is
    // the operator-friendly default — adding an FS is additive.
    excluded.retain(|id| !allowed.contains(id));

    // 7. Pull every feature in the Space, compute per-server totals (the
    //    badge denominator), then keep only the FS-filtered subset for the
    //    rendered list. The `is_available` gate is intentionally not
    //    applied here — disconnected features still appear, dimmed.
    let all_features = state
        .server_feature_repository_core
        .list_for_space(&space_id.to_string())
        .await
        .map_err(|e| e.to_string())?;

    let mut server_totals: HashMap<String, ServerFeatureTotalsDto> = HashMap::new();
    for f in &all_features {
        let entry = server_totals
            .entry(f.server_id.clone())
            .or_insert(ServerFeatureTotalsDto {
                tools: 0,
                prompts: 0,
                resources: 0,
            });
        match f.feature_type {
            mcpmux_core::FeatureType::Tool => entry.tools += 1,
            mcpmux_core::FeatureType::Prompt => entry.prompts += 1,
            mcpmux_core::FeatureType::Resource => entry.resources += 1,
        }
    }

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

    let feature_sets: Vec<EffectiveFeatureSetDto> = resolved_sets
        .into_iter()
        .map(|fs| EffectiveFeatureSetDto {
            id: fs.id,
            name: fs.name,
            feature_set_type: match fs.feature_set_type {
                FeatureSetType::Starter => "starter".to_string(),
                FeatureSetType::Custom => "custom".to_string(),
            },
        })
        .collect();

    Ok(WorkspaceEffectiveFeaturesDto {
        workspace_root: normalized,
        source,
        binding_id,
        space_id: space_id.to_string(),
        space_name: space.name,
        feature_sets,
        tools,
        prompts,
        resources,
        server_totals,
    })
}
