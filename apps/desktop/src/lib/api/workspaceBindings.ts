import { invoke } from '@tauri-apps/api/core';

/**
 * A WorkspaceBinding maps one normalized filesystem path to one or more
 * FeatureSets within a Space. When an MCP session reports a root that
 * matches a binding (longest-prefix wins), the resolver hands back the
 * binding's `space_id` and the union of `feature_set_ids` — multiple FSes
 * compose into a single allow set, no "follow active" indirection.
 */
export interface WorkspaceBinding {
  id: string;
  workspace_root: string;
  /** Friendly display name shown instead of the folder path when set. */
  label: string | null;
  space_id: string;
  /**
   * Non-empty by construction. Order is the operator-chosen rendering
   * order; the resolver treats the list as a set. FeatureSet ids are
   * strings (builtins use `fs_default_<space>`, customs use UUIDs).
   */
  feature_set_ids: string[];
  created_at: string;
  updated_at: string;
}

/** Input payload for create / update. `feature_set_ids` must be non-empty. */
export interface WorkspaceBindingInput {
  workspace_root: string;
  label?: string | null;
  space_id: string;
  feature_set_ids: string[];
}

/** List every binding (sorted by workspace_root). */
export async function listWorkspaceBindings(): Promise<WorkspaceBinding[]> {
  return invoke('list_workspace_bindings');
}

/**
 * Every filesystem root that connected MCP clients have reported during
 * their current sessions, deduplicated across sessions. Surfaces folders
 * that aren't bound yet so the user can configure them from the Workspaces
 * tab instead of waiting for the one-shot prompt.
 */
export async function listReportedWorkspaceRoots(): Promise<string[]> {
  return invoke('list_reported_workspace_roots');
}

/**
 * Live path validation for the manual-add form. Runs the SAME rules the
 * create/update commands apply so "validates in UI → saves OK" is a
 * guarantee, not a hope.
 *
 * Resolves with the server's normalized form (e.g. `d:\foo` from raw
 * `D:\foo\`). Rejects with a descriptive message on invalid input; empty
 * input rejects with an empty string so the UI can distinguish
 * "don't nag yet" from "here's a real error".
 */
export async function validateWorkspaceRoot(path: string): Promise<string> {
  return invoke('validate_workspace_root', { path });
}

/** List bindings whose target Space is the given one. */
export async function listWorkspaceBindingsForSpace(
  spaceId: string
): Promise<WorkspaceBinding[]> {
  return invoke('list_workspace_bindings_for_space', { spaceId });
}

/**
 * Create a new binding. `workspace_root` is normalized server-side so
 * callers can pass raw OS paths, `file://` URIs, or MCP-reported roots.
 */
export async function createWorkspaceBinding(
  input: WorkspaceBindingInput
): Promise<WorkspaceBinding> {
  return invoke('create_workspace_binding', { input });
}

/** Update any axis of an existing binding. */
export async function updateWorkspaceBinding(
  id: string,
  input: WorkspaceBindingInput
): Promise<WorkspaceBinding> {
  return invoke('update_workspace_binding', { id, input });
}

/** Delete a binding by id. */
export async function deleteWorkspaceBinding(id: string): Promise<void> {
  return invoke('delete_workspace_binding', { id });
}

/** Convenience: build a `WorkspaceBindingInput` from a binding-shaped object. */
export function toInput(b: WorkspaceBinding): WorkspaceBindingInput {
  return {
    workspace_root: b.workspace_root,
    label: b.label,
    space_id: b.space_id,
    feature_set_ids: b.feature_set_ids,
  };
}

/**
 * Per-feature view returned from `get_workspace_effective_features`.
 *
 * `available` is `true` exactly when the underlying server is currently
 * connected. A `false` value with `server_status = "disconnected"` (or
 * `auth_required` / `error`) is the user's "configured but unavailable"
 * case — the FS still includes this feature, but its server isn't usable
 * right now.
 */
export interface EffectiveFeature {
  id: string;
  feature_name: string;
  display_name: string | null;
  description: string | null;
  server_id: string;
  server_alias: string | null;
  /**
   * snake_case mirror of the gateway's connection status, plus `unknown`
   * when the gateway isn't running.
   */
  server_status:
    | 'connected'
    | 'connecting'
    | 'disconnected'
    | 'refreshing'
    | 'auth_required'
    | 'authenticating'
    | 'error'
    | 'unknown';
  available: boolean;
}

/**
 * Per-server total feature counts in the resolved Space, regardless of FS
 * filter. The right-hand side of the "{mapped} / {total}" badges.
 */
export interface ServerFeatureTotals {
  tools: number;
  prompts: number;
  resources: number;
}

/** One FeatureSet contributing to the resolved view. */
export interface EffectiveFeatureSetSummary {
  id: string;
  name: string;
  feature_set_type: 'starter' | 'default' | 'custom';
}

export interface WorkspaceEffectiveFeatures {
  workspace_root: string;
  /** `binding` when a saved WorkspaceBinding matched; `unbound` when no binding matched — the `feature_sets` field previews the default Space's Default FS but a live session here would be denied. */
  source: 'binding' | 'unbound';
  binding_id: string | null;
  space_id: string;
  space_name: string;
  /** All FeatureSets contributing to the resolved view, in operator-chosen order. ≥ 1. */
  feature_sets: EffectiveFeatureSetSummary[];
  tools: EffectiveFeature[];
  prompts: EffectiveFeature[];
  resources: EffectiveFeature[];
  /** `server_id -> totals` for every server installed in the resolved Space. */
  server_totals: Record<string, ServerFeatureTotals>;
}

/**
 * Resolve the FeatureSet that applies for a given workspace root and return
 * its full configured tool/prompt/resource list with per-feature
 * availability — same view the gateway resolver builds for live sessions.
 */
export async function getWorkspaceEffectiveFeatures(
  workspaceRoot: string
): Promise<WorkspaceEffectiveFeatures> {
  return invoke('get_workspace_effective_features', { workspaceRoot });
}
