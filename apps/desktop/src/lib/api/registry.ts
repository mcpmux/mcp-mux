/**
 * Registry API functions for Tauri IPC.
 */

import { invoke } from '@tauri-apps/api/core';
import type { RegistryCategory, ServerDefinition, InstalledServerState, UiConfig, HomeConfig } from '../../types/registry';

/** Discover all servers (definitions from all sources) */
export async function discoverServers(): Promise<ServerDefinition[]> {
  return invoke<ServerDefinition[]>('discover_servers');
}

/** Get UI configuration from registry bundle (filters, sort options, etc.) */
export async function getRegistryUiConfig(): Promise<UiConfig> {
  return invoke<UiConfig>('get_registry_ui_config');
}

/** Get home configuration from registry bundle (featured server IDs) */
export async function getRegistryHomeConfig(): Promise<HomeConfig | null> {
  return invoke<HomeConfig | null>('get_registry_home_config');
}

/** Check if registry is running in offline mode (using disk cache) */
export async function isRegistryOffline(): Promise<boolean> {
  return invoke<boolean>('is_registry_offline');
}

/** Force refresh server discovery from all sources (ignores cache) 
 * Returns number of newly auto-installed user-configured servers */
export async function refreshRegistry(): Promise<number> {
  return invoke<number>('refresh_registry');
}

/** Get a specific server definition */
export async function getServerDefinition(serverId: string): Promise<ServerDefinition | null> {
  return invoke<ServerDefinition | null>('get_server_definition', { serverId });
}

/** List all registry categories */
export async function listCategories(): Promise<RegistryCategory[]> {
  return invoke<RegistryCategory[]>('list_registry_categories');
}

/** Install a server (adds to DB) */
export async function installServer(id: string, spaceId: string): Promise<void> {
  return invoke<void>('install_server', { id, spaceId });
}

/** Uninstall a server (removes from DB) */
export async function uninstallServer(id: string, spaceId: string): Promise<void> {
  return invoke<void>('uninstall_server', { id, spaceId });
}

/** List installed servers (returns state from DB) */
export async function listInstalledServers(spaceId?: string): Promise<InstalledServerState[]> {
  return invoke<InstalledServerState[]>('list_installed_servers', { spaceId });
}

/** Get count of installed servers */
export async function getInstalledServersCount(spaceId?: string): Promise<number> {
  const servers = await listInstalledServers(spaceId);
  return servers.length;
}

/** Enable or disable a server */
export async function setServerEnabled(
  id: string,
  enabled: boolean,
  spaceId: string
): Promise<void> {
  return invoke<void>('set_server_enabled', { id, enabled, spaceId });
}

/** Set OAuth connected status */
export async function setServerOAuthConnected(
  id: string,
  connected: boolean,
  spaceId: string
): Promise<void> {
  return invoke<void>('set_server_oauth_connected', { id, connected, spaceId });
}

/** Save input values for a server */
export async function saveServerInputs(
  id: string,
  inputValues: Record<string, string>,
  spaceId: string,
  envOverrides?: Record<string, string>,
  argsAppend?: string[],
  extraHeaders?: Record<string, string>
): Promise<void> {
  return invoke<void>('save_server_inputs', { id, inputValues, spaceId, envOverrides, argsAppend, extraHeaders });
}
