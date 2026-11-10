/**
 * Credential and Prior Input Reuse API for Tauri IPC (Issue #227).
 */

import { invoke } from '@tauri-apps/api/core';

export interface PriorCredentialSummary {
  source_space_id: string;
  source_space_name: string;
  server_id: string;
  available_keys: string[];
  updated_at?: string;
}

/**
 * Check if the same server has been configured with credentials in another Space.
 * Returns a summary without secret values.
 */
export async function findPriorCredential(
  spaceId: string,
  serverId: string,
  inputId?: string
): Promise<PriorCredentialSummary | null> {
  return invoke<PriorCredentialSummary | null>('find_prior_credential', {
    spaceId,
    serverId,
    inputId,
  });
}

/**
 * Fetch decrypted credentials / inputs for a server from the source Space.
 * Only called when the user explicitly clicks "[ Use ]".
 */
export async function getPriorCredentialValues(
  sourceSpaceId: string,
  serverId: string
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>('get_prior_credential_values', {
    sourceSpaceId,
    serverId,
  });
}

/**
 * Fill only blank configuration fields from a prior installation.
 * Values the user has already entered always take precedence.
 */
export function fillEmptyCredentialValues(
  currentValues: Record<string, string>,
  priorValues: Record<string, string>
): Record<string, string> {
  return {
    ...currentValues,
    ...Object.fromEntries(
      Object.entries(priorValues).filter(([key]) => !currentValues[key]?.trim())
    ),
  };
}
