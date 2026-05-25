/**
 * Server clone API — Tauri wrappers for multi-account cloning.
 */

import { invoke } from '@tauri-apps/api/core';
import type { InstalledServerState } from '@/types/registry';

/** Default suffix suggestions shown in the clone wizard */
export const CLONE_SUFFIX_SUGGESTIONS = ['work', 'personal', 'prod', 'staging'] as const;

/** Installed server row returned by clone_server (includes clone lineage). */
export interface ClonedInstalledServer extends InstalledServerState {
  cloned_from?: string | null;
}

/**
 * Clone an installed server into a new suffixed manual-entry install in the same space.
 *
 * `displayName` is optional; when set, it is stored as the user-supplied display label
 * (`display_name_override`) and survives later definition refreshes. When omitted, the
 * UI falls back to the auto `"Source (suffix)"` cached definition name.
 */
export async function cloneServer(
  spaceId: string,
  sourceServerId: string,
  suffix: string,
  alias?: string,
  displayName?: string
): Promise<ClonedInstalledServer> {
  return invoke<ClonedInstalledServer>('clone_server', {
    spaceId,
    sourceServerId,
    suffix,
    alias: alias ?? null,
    displayName: displayName ?? null,
  });
}

/**
 * Return whether a suffixed clone ID is available in the given space.
 */
export async function isCloneIdAvailable(
  spaceId: string,
  sourceServerId: string,
  suffix: string
): Promise<boolean> {
  return invoke<boolean>('is_clone_id_available', {
    spaceId,
    sourceServerId,
    suffix,
  });
}

/**
 * Suggest the first available default suffix for cloning a server.
 */
export async function suggestCloneSuffix(spaceId: string, sourceServerId: string): Promise<string> {
  return invoke<string>('suggest_clone_suffix', {
    spaceId,
    sourceServerId,
  });
}

/**
 * List account clones that were created from the given source server in a space.
 */
export async function listCloneDependents(
  spaceId: string,
  sourceServerId: string
): Promise<ClonedInstalledServer[]> {
  return invoke<ClonedInstalledServer[]>('list_clone_dependents', {
    spaceId,
    sourceServerId,
  });
}

/**
 * Normalize a server ID the same way the backend does (lowercase, strip underscores/spaces).
 */
export function normalizeServerId(id: string): string {
  return id
    .split('')
    .filter((c) => /[a-zA-Z0-9]/.test(c) || c === '-' || c === '.')
    .map((c) => (/[a-zA-Z0-9]/.test(c) ? c.toLowerCase() : c))
    .join('');
}

/**
 * Derive the clone server ID preview from a base install ID and user suffix.
 */
export function deriveCloneServerId(baseServerId: string, suffix: string): string {
  const normalizedSuffix = normalizeServerId(suffix);
  if (!normalizedSuffix) {
    return '';
  }
  return normalizeServerId(`${baseServerId}-${normalizedSuffix}`);
}

/**
 * Derive the tool-name alias preview for a clone suffix.
 */
export function deriveCloneAlias(suffix: string): string {
  return normalizeServerId(suffix).replace(/_/g, '-');
}
