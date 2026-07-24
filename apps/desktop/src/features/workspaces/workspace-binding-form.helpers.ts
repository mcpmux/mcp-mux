import type { WorkspaceBinding, WorkspaceBindingInput } from '@/lib/api/workspaceBindings';

export type SaveStatus =
  | { kind: 'idle' }
  | { kind: 'saving' }
  | { kind: 'saved' }
  | { kind: 'error'; message: string };

/**
 * Structural equality between two binding inputs. The autosave effect
 * uses this to skip writes when the user re-toggled their way back to
 * the last-saved state.
 */
export function normalizeLabel(label: string | null | undefined): string | null {
  const trimmed = label?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : null;
}

export function normalizeIcon(icon: string | null | undefined): string | null {
  const trimmed = icon?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : null;
}

/**
 * Last path segment of a workspace root, normalized for cross-platform matching.
 */
export function folderName(root: string): string {
  const segments = root.replace(/\\/g, '/').replace(/\/$/, '').split('/');
  return segments[segments.length - 1] ?? root;
}

/**
 * Bindings on other machines (or scopes) that can seed a new create-from-live row.
 * Same folder name is enough; identical absolute paths count when machine differs.
 */
export function findAdoptableSiblingBindings(
  allBindings: WorkspaceBinding[],
  workspaceRoot: string,
  targetMachineId: string | null,
): WorkspaceBinding[] {
  const currentFolder = folderName(workspaceRoot).toLowerCase();
  const normalizedRoot = workspaceRoot.toLowerCase();
  return allBindings.filter((binding) => {
    if (folderName(binding.workspace_root).toLowerCase() !== currentFolder) return false;
    const samePath = binding.workspace_root.toLowerCase() === normalizedRoot;
    if (!samePath) return true;
    return (binding.machine_id ?? null) !== targetMachineId;
  });
}

/**
 * Space, feature sets, label, and icon to copy from an adopt source binding.
 */
export function adoptBindingSeed(
  source: WorkspaceBinding,
  workspaceRoot: string,
): Pick<WorkspaceBinding, 'space_id' | 'feature_set_ids' | 'label' | 'icon'> {
  const trimmedLabel = source.label?.trim() ?? '';
  return {
    space_id: source.space_id,
    feature_set_ids: source.feature_set_ids,
    label: trimmedLabel.length > 0 ? trimmedLabel : folderName(workspaceRoot),
    icon: source.icon,
  };
}

/** True when the icon value is an uploaded file ref or URL, not a plain emoji. */
export function isWorkspaceFileIcon(icon: string): boolean {
  const trimmed = icon.trim();
  return trimmed.startsWith('local:') || trimmed.startsWith('http://') || trimmed.startsWith('https://');
}

export type RootValidationState =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'ok'; normalized: string }
  | { state: 'error'; reason: string; duplicate?: boolean };

/** True when two bindings would collide on the partial unique indexes. */
export function bindingScopeConflicts(
  existing: WorkspaceBinding,
  root: string,
  machineId: string | null,
  clientId: string | null | undefined,
): boolean {
  if (existing.workspace_root !== root) return false;
  return (
    (existing.machine_id ?? null) === machineId &&
    (existing.client_id ?? null) === (clientId ?? null)
  );
}

/** Map empty machine picker value to null for API payloads. */
export function bindingMachineId(value: string): string | null {
  return value.trim() ? value : null;
}

/** Build a workspace binding input from lifted form field values. */
export function buildBindingPayload(params: {
  root: string;
  label: string;
  icon: string;
  spaceId: string;
  fsIds: string[];
  machineId: string;
  clientId?: string;
  resolvedMachineId: string | null;
}): WorkspaceBindingInput {
  return {
    workspace_root: params.root.trim(),
    label: params.label.trim() || null,
    icon: params.icon.trim() || null,
    space_id: params.spaceId,
    feature_set_ids: params.fsIds,
    machine_id: params.resolvedMachineId,
    client_id: params.resolvedMachineId ? null : params.clientId,
  };
}

export function sameBindingInput(
  a: WorkspaceBindingInput,
  b: {
    workspace_root: string;
    label?: string | null;
    icon?: string | null;
    space_id: string;
    feature_set_ids: string[];
    machine_id?: string | null;
  }
): boolean {
  if (a.workspace_root.trim() !== b.workspace_root.trim()) return false;
  if (normalizeLabel(a.label) !== normalizeLabel(b.label)) return false;
  if (normalizeIcon(a.icon) !== normalizeIcon(b.icon)) return false;
  if (a.space_id !== b.space_id) return false;
  if ((a.machine_id ?? null) !== (b.machine_id ?? null)) return false;
  if (a.feature_set_ids.length !== b.feature_set_ids.length) return false;
  return a.feature_set_ids.every((id, i) => id === b.feature_set_ids[i]);
}
