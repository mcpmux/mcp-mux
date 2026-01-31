import { invoke } from '@tauri-apps/api/core';

/**
 * A Space represents an isolated environment with its own credentials and server configs.
 */
export interface Space {
  id: string; // UUID string
  name: string;
  icon: string | null;
  description: string | null;
  is_default: boolean;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

/**
 * List all spaces.
 */
export async function listSpaces(): Promise<Space[]> {
  return invoke('list_spaces');
}

/**
 * Get a space by ID.
 */
export async function getSpace(id: string): Promise<Space | null> {
  return invoke('get_space', { id });
}

/**
 * Create a new space.
 */
export async function createSpace(name: string, icon?: string): Promise<Space> {
  return invoke('create_space', { name, icon });
}

/**
 * Delete a space.
 */
export async function deleteSpace(id: string): Promise<void> {
  return invoke('delete_space', { id });
}

/**
 * Get the active (default) space.
 */
export async function getActiveSpace(): Promise<Space | null> {
  return invoke('get_active_space');
}

/**
 * Set the active space.
 */
export async function setActiveSpace(id: string): Promise<void> {
  return invoke('set_active_space', { id });
}

/**
 * Read space configuration JSON file.
 */
export async function readSpaceConfig(spaceId: string): Promise<string> {
  return invoke('read_space_config', { spaceId });
}

/**
 * Save space configuration JSON file.
 */
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

/**
 * Open space configuration file in external editor.
 */
export async function openSpaceConfigFile(spaceId: string): Promise<void> {
  return invoke('open_space_config_file', { spaceId });
}
