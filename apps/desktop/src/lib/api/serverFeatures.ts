import { invoke } from '@tauri-apps/api/core';

/**
 * Type of MCP feature.
 */
export type FeatureType = 'tool' | 'prompt' | 'resource';

/**
 * A discovered feature from an MCP server.
 */
export interface ServerFeature {
  id: string;
  space_id: string;
  server_id: string;
  feature_type: FeatureType;
  feature_name: string;
  display_name: string | null;
  description: string | null;
  input_schema: Record<string, unknown> | null;
  discovered_at: string;
  last_seen_at: string;
  is_available: boolean;
}

/**
 * List all features for a space.
 */
export async function listServerFeatures(spaceId: string): Promise<ServerFeature[]> {
  return invoke('list_server_features', { spaceId });
}

/**
 * List features for a specific server in a space.
 */
export async function listServerFeaturesByServer(
  spaceId: string,
  serverId: string
): Promise<ServerFeature[]> {
  return invoke('list_server_features_by_server', { spaceId, serverId });
}

/**
 * List features by type for a server.
 */
export async function listServerFeaturesByType(
  spaceId: string,
  serverId: string,
  featureType: FeatureType
): Promise<ServerFeature[]> {
  return invoke('list_server_features_by_type', { spaceId, serverId, featureType });
}

/**
 * Get a specific feature by ID.
 */
export async function getServerFeature(id: string): Promise<ServerFeature | null> {
  return invoke('get_server_feature', { id });
}
