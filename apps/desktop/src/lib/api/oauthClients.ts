/**
 * OAuth Client Grants API
 * 
 * For managing feature set grants for OAuth/inbound clients (Cursor, VS Code, etc.)
 */

import { invoke } from '@tauri-apps/api/core';

/**
 * Get grants for an OAuth client in a specific space.
 */
export async function getOAuthClientGrants(
  clientId: string,
  spaceId: string
): Promise<string[]> {
  return invoke('get_oauth_client_grants', { clientId, spaceId });
}

/**
 * Grant a feature set to an OAuth client in a specific space.
 */
export async function grantOAuthClientFeatureSet(
  clientId: string,
  spaceId: string,
  featureSetId: string
): Promise<void> {
  return invoke('grant_oauth_client_feature_set', { clientId, spaceId, featureSetId });
}

/**
 * Revoke a feature set from an OAuth client in a specific space.
 */
export async function revokeOAuthClientFeatureSet(
  clientId: string,
  spaceId: string,
  featureSetId: string
): Promise<void> {
  return invoke('revoke_oauth_client_feature_set', { clientId, spaceId, featureSetId });
}

/**
 * Resolved features for a client
 */
export interface ResolvedClientFeatures {
  space_id: string;
  feature_set_ids: string[];
  tools: Array<{ name: string; description?: string; server_id: string }>;
  prompts: Array<{ name: string; description?: string; server_id: string }>;
  resources: Array<{ name: string; description?: string; server_id: string }>;
}

/**
 * Get resolved features (tools/prompts/resources) for an OAuth client in a specific space.
 * This resolves all feature sets granted to the client into actual features.
 * 
 * The caller is responsible for determining which space to query:
 * - For locked clients: pass the client's locked_space_id
 * - For follow_active clients: pass the currently active space_id
 * 
 * @param clientId - The OAuth client ID
 * @param spaceId - The space ID to resolve features for (required)
 */
export async function getOAuthClientResolvedFeatures(
  clientId: string,
  spaceId: string
): Promise<ResolvedClientFeatures> {
  return invoke('get_oauth_client_resolved_features', { clientId, spaceId });
}

