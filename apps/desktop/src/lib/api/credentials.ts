/**
 * Central Credential Profiles & Secrets Store API
 *
 * Provides operations for listing, creating, retrieving, updating,
 * and deleting central credential profiles.
 */

import { invoke } from '@tauri-apps/api/core';

export interface CredentialProfile {
  id: string;
  name: string;
  server_id: string;
  server_name: string | null;
  values: Record<string, string>;
  created_at: string;
  updated_at: string;
}

export interface CredentialProfileSummary {
  id: string;
  name: string;
  server_id: string;
  server_name: string | null;
  input_keys: string[];
  usage_count: number;
  created_at: string;
  updated_at: string;
}

/**
 * List all credential profile summaries.
 */
export async function listCredentialProfiles(): Promise<CredentialProfileSummary[]> {
  return invoke('list_credential_profiles');
}

/**
 * List credential profile summaries for a specific server definition ID.
 */
export async function getCredentialProfilesForServer(
  serverId: string
): Promise<CredentialProfileSummary[]> {
  return invoke('get_credential_profiles_for_server', { serverId });
}

/**
 * Get a full credential profile by ID (including decrypted values).
 */
export async function getCredentialProfile(id: string): Promise<CredentialProfile> {
  return invoke('get_credential_profile', { id });
}

/**
 * Create a new credential profile.
 */
export async function createCredentialProfile(
  name: string,
  serverId: string,
  serverName: string | null,
  values: Record<string, string>
): Promise<CredentialProfile> {
  return invoke('create_credential_profile', {
    name,
    serverId,
    serverName,
    values,
  });
}

/**
 * Update an existing credential profile and propagate to all linked servers.
 */
export async function updateCredentialProfile(
  id: string,
  name: string,
  values: Record<string, string>
): Promise<CredentialProfile> {
  return invoke('update_credential_profile', {
    id,
    name,
    values,
  });
}

/**
 * Delete a credential profile by ID.
 */
export async function deleteCredentialProfile(id: string): Promise<void> {
  return invoke('delete_credential_profile', { id });
}
