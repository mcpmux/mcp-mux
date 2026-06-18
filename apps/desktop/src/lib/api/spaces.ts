import { invoke } from '@tauri-apps/api/core';

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

export async function listSpaces(): Promise<Space[]> {
  return invoke('list_spaces');
}

export async function getSpace(id: string): Promise<Space | null> {
  return invoke('get_space', { id });
}

export async function createSpace(name: string, icon?: string): Promise<Space> {
  return invoke('create_space', { name, icon });
}

export async function deleteSpace(id: string): Promise<void> {
  return invoke('delete_space', { id });
}

export async function readSpaceConfig(spaceId: string): Promise<string> {
  return invoke('read_space_config', { spaceId });
}

export async function saveSpaceConfig(spaceId: string, content: string): Promise<void> {
  return invoke('save_space_config', { spaceId, content });
}

/**
 * Remove a server from the space configuration file.
 * Returns true if the server was found and removed, false if it wasn't in the config.
 */
export async function removeServerFromConfig(spaceId: string, serverId: string): Promise<boolean> {
  return invoke('remove_server_from_config', { spaceId, serverId });
}

export async function openSpaceConfigFile(spaceId: string): Promise<void> {
  return invoke('open_space_config_file', { spaceId });
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
  return invoke('list_space_base_dirs', { spaceId });
}

/**
 * Add a base directory to a Space. `path` is validated (absolute folder) and
 * normalized backend-side; rejects a folder already claimed by another Space.
 */
export async function addSpaceBaseDir(spaceId: string, path: string): Promise<SpaceBaseDir> {
  return invoke('add_space_base_dir', { spaceId, path });
}

/** Remove a base directory by its row id. */
export async function removeSpaceBaseDir(id: string): Promise<void> {
  return invoke('remove_space_base_dir', { id });
}
