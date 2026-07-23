/** @deprecated Prefer `@/lib/backend` — shim during facade migration. */
import { apiCall } from './transport';

/** How this binding is keyed during resolver lookup. */
export type BindingType = 'path' | 'id';

/**
 * A WorkspaceBinding maps one normalized filesystem path to one or more
 * FeatureSets within a Space. When an MCP session reports a root that
 * matches a binding (longest-prefix wins), the resolver hands back the
 * binding's `space_id` and the union of `feature_set_ids` — multiple FSes
 * compose into a single allow set, no "follow active" indirection.
 *
 * Id-type bindings (`binding_type: 'id'`) route a rootless OAuth/API client
 * by `workspace_root` (the client id) instead of a folder path.
 */
export interface WorkspaceBinding {
  id: string;
  workspace_root: string;
  /** `path` (default) or `id` — see interface doc. */
  binding_type?: BindingType;
  /** When set, binding applies only to this OAuth client. */
  client_id?: string | null;
  /** When set, binding applies only on this machine; null = global canonical. */
  machine_id: string | null;
  /** Friendly display name shown instead of the folder path when set. */
  label: string | null;
  /** Optional icon: emoji, URL, or local:workspace-icons ref. */
  icon: string | null;
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
  icon?: string | null;
  space_id: string;
  feature_set_ids: string[];
  /** When set, creates a client-scoped binding. */
  client_id?: string | null;
  /** When set, scopes the binding to this machine; null = global canonical. */
  machine_id?: string | null;
  /** `path` folder binding (default) or `id` client-id binding. */
  binding_type?: BindingType;
}

/** List every binding (sorted by workspace_root). */
export async function listWorkspaceBindings(): Promise<WorkspaceBinding[]> {
  return apiCall('list_workspace_bindings');
}

/**
 * Every filesystem root that connected MCP clients have reported during
 * their current sessions, deduplicated across sessions. Surfaces folders
 * that aren't bound yet so the user can configure them from the Workspaces
 * tab instead of waiting for the one-shot prompt.
 */
export async function listReportedWorkspaceRoots(): Promise<string[]> {
  return apiCall('list_reported_workspace_roots');
}

/**
 * Forget every reported workspace root that has no binding ("unmapped").
 * Clears them from the Workspaces tab in one action; the gateway then offers
 * the "map this folder?" prompt again the next time those apps report a
 * folder (or reconnect). Mapped folders are left untouched. Resolves with
 * the number of roots cleared.
 */
export async function clearUnmappedReportedRoots(): Promise<number> {
  return apiCall('clear_unmapped_reported_roots');
}

/**
 * Remove a single reported workspace root from the session registry.
 * Drops it from every active MCP session that holds it; unlike
 * `clearUnmappedReportedRoots` this targets one specific path. Returns `true`
 * when the root was found and removed.
 */
export async function forgetReportedRoot(root: string): Promise<boolean> {
  return apiCall('forget_reported_root', { root });
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
  return apiCall('validate_workspace_root', { path });
}

/** List bindings whose target Space is the given one. */
export async function listWorkspaceBindingsForSpace(
  spaceId: string
): Promise<WorkspaceBinding[]> {
  return apiCall('list_workspace_bindings_for_space', { spaceId });
}

/**
 * Create a new binding. `workspace_root` is normalized server-side so
 * callers can pass raw OS paths, `file://` URIs, or MCP-reported roots.
 */
export async function createWorkspaceBinding(
  input: WorkspaceBindingInput
): Promise<WorkspaceBinding> {
  return apiCall('create_workspace_binding', { input });
}

/** Update any axis of an existing binding. */
export async function updateWorkspaceBinding(
  id: string,
  input: WorkspaceBindingInput
): Promise<WorkspaceBinding> {
  return apiCall('update_workspace_binding', { id, input });
}

/** Delete a binding by id. */
export async function deleteWorkspaceBinding(id: string): Promise<void> {
  return apiCall('delete_workspace_binding', { id });
}

/** Persist a WorkspaceNeedsBinding panel dismissal for a client/root pair. */
export async function dismissWorkspaceBindingPrompt(
  clientId: string,
  workspaceRoot: string,
): Promise<void> {
  return apiCall('dismiss_workspace_binding_prompt', { clientId, workspaceRoot });
}

/**
 * True when the user previously closed the binding prompt without saving.
 * Omit `clientId` to check whether any client dismissed that workspace root.
 */
export async function isWorkspaceBindingPromptDismissed(
  workspaceRoot: string,
  clientId?: string | null,
): Promise<boolean> {
  return apiCall('is_workspace_binding_prompt_dismissed', {
    workspaceRoot,
    ...(clientId ? { clientId } : {}),
  });
}

/** Convenience: build a `WorkspaceBindingInput` from a binding-shaped object. */
export function toInput(b: WorkspaceBinding): WorkspaceBindingInput {
  return {
    workspace_root: b.workspace_root,
    label: b.label,
    icon: b.icon,
    space_id: b.space_id,
    feature_set_ids: b.feature_set_ids,
    client_id: b.client_id,
    machine_id: b.machine_id,
    binding_type: b.binding_type,
  };
}

/** True when the binding routes by OAuth/API client id instead of folder path. */
export function isIdBinding(binding: WorkspaceBinding): boolean {
  return binding.binding_type === 'id';
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
  /** `binding` when a saved WorkspaceBinding matched; `unbound` when no binding matched — caller has zero backend tools by default. Bind to enable access. */
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
  workspaceRoot: string,
  machineId?: string | null,
): Promise<WorkspaceEffectiveFeatures> {
  return apiCall('get_workspace_effective_features', {
    workspaceRoot,
    ...(machineId ? { machineId } : {}),
  });
}
