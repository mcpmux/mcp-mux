/** @deprecated Prefer `@/lib/backend` — shim during facade migration. */
import { openSpaceConfigFile as shellOpenSpaceConfigFile } from '@/lib/backend/shell';

import { apiCall } from './transport';

/**
 * A Space represents an isolated environment with its own credentials and
 * server configs. Every Space has exactly one auto-seeded Default FeatureSet
 * which is the routing fallback when no WorkspaceBinding matches. Exactly
 * one Space carries `is_default = true` — that's the gateway's fallback
 * when a session reports no root or its root has no binding.
 */
export interface Space {
  id: string;
  name: string;
  icon: string | null;
  description: string | null;
  is_default: boolean;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

/** List all spaces. */
export async function listSpaces(): Promise<Space[]> {
  return apiCall('list_spaces');
}

/** Get a space by ID. */
export async function getSpace(id: string): Promise<Space | null> {
  return apiCall('get_space', { id });
}

/** Create a new space. */
export async function createSpace(name: string, icon?: string): Promise<Space> {
  return apiCall('create_space', { name, icon });
}

/** Delete a space by ID. */
export async function deleteSpace(id: string): Promise<void> {
  return apiCall('delete_space', { id });
}

/** Read the raw space config file contents. */
export async function readSpaceConfig(spaceId: string): Promise<string> {
  return apiCall('read_space_config', { spaceId });
}

/** Save raw space config file contents. */
export async function saveSpaceConfig(spaceId: string, content: string): Promise<void> {
  return apiCall('save_space_config', { spaceId, content });
}

/**
 * Remove a server from the space configuration file.
 * Returns true if the server was found and removed, false if it wasn't in the config.
 */
export async function removeServerFromConfig(spaceId: string, serverId: string): Promise<boolean> {
  return apiCall('remove_server_from_config', { spaceId, serverId });
}

/**
 * Replace a custom server's entry in the space configuration file.
 * `entry` is the standard MCP format object (command/args/env or url/headers, etc.)
 * that goes under the server's `mcpServers` key.
 */
export async function updateServerInConfig(
  spaceId: string,
  serverId: string,
  entry: Record<string, unknown>,
): Promise<void> {
  return apiCall('update_server_in_config', { spaceId, serverId, entry });
}

/**
 * Persist a manual-entry clone's definition to `installed_servers.cached_definition`.
 * `entry` uses the same standard MCP format as `updateServerInConfig`.
 */
export async function updateClonedServerDefinition(
  spaceId: string,
  serverId: string,
  entry: Record<string, unknown>,
): Promise<void> {
  return apiCall('update_cloned_server_definition', { spaceId, serverId, entry });
}

/** Reveal a space config file in the system editor (desktop only). */
export async function openSpaceConfigFile(spaceId: string): Promise<void> {
  return shellOpenSpaceConfigFile(spaceId);
}

/**
 * A base directory claimed by a Space. Any workspace root a connected client
 * opens at or under `path` is scoped to that Space (longest-prefix wins): an
 * unmapped folder there falls back to the Space's Starter set, and the
 * meta-tools / mapping popup restrict to that Space. `path` is normalized and
 * globally unique (one owner per folder).
 */
export interface SpaceBaseDir {
  id: string;
  space_id: string;
  path: string;
  created_at: string;
}

/** List a Space's base directories. */
export async function listSpaceBaseDirs(spaceId: string): Promise<SpaceBaseDir[]> {
  return apiCall('list_space_base_dirs', { spaceId });
}

/**
 * Add a base directory to a Space. `path` is validated (absolute folder) and
 * normalized backend-side; rejects a folder already claimed by another Space.
 */
export async function addSpaceBaseDir(spaceId: string, path: string): Promise<SpaceBaseDir> {
  return apiCall('add_space_base_dir', { spaceId, path });
}

/** Remove a base directory by its row id. */
export async function removeSpaceBaseDir(id: string): Promise<void> {
  return apiCall('remove_space_base_dir', { id });
}

export interface UpdateSpaceInput {
  name?: string;
  icon?: string;
  description?: string;
}

/** Update a space's display metadata (name, icon, description). */
export async function updateSpace(id: string, input: UpdateSpaceInput): Promise<Space> {
  return apiCall('update_space', {
    id,
    name: input.name,
    icon: input.icon,
    description: input.description,
    input,
  });
}
