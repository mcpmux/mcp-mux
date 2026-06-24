/** @deprecated Prefer `@/lib/backend` — shim during facade migration. */
import {
  getAdminWebSettings as shellGetAdminWebSettings,
  openLogsFolder as shellOpenLogsFolder,
  updateAdminWebSettings as shellUpdateAdminWebSettings,
} from '@/lib/backend/shell';

import { apiCall } from './transport';

/** Startup and system tray settings. */
export interface StartupSettings {
  autoLaunch: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
}

/** Per-server package update policy. */
export type UpdatePolicy = 'auto' | 'notify' | 'pinned';

/** App-wide default update policy for new server installs. */
export interface ServerUpdateSettings {
  defaultUpdatePolicy: UpdatePolicy;
  /** ISO timestamp of the last bulk version probe, when available. */
  lastCheckedAt?: string | null;
}

/** Persisted gateway port override, default, and currently active port. */
export interface GatewayPortSettings {
  configuredPort: number | null;
  defaultPort: number;
  activePort: number | null;
  publicUrl: string | null;
}

/** Web admin HTTP server settings (loopback remote UI). */
export interface AdminWebSettings {
  enabled: boolean;
  port: number;
  trustCfAccess: boolean;
  cfTeamDomain: string;
}

/**
 * Load startup and system tray preferences.
 */
export async function getStartupSettings(): Promise<StartupSettings> {
  return apiCall('get_startup_settings');
}

/**
 * Persist startup and system tray preferences.
 */
export async function updateStartupSettings(settings: StartupSettings): Promise<void> {
  return apiCall('update_startup_settings', { settings });
}

/**
 * Load the default update policy for newly installed servers.
 */
export async function getServerUpdateSettings(): Promise<ServerUpdateSettings> {
  return apiCall('get_server_update_settings');
}

/**
 * Persist the default update policy for newly installed servers.
 */
export async function updateServerUpdateSettings(settings: ServerUpdateSettings): Promise<void> {
  return apiCall('update_server_update_settings', { settings });
}

/** Probe all notify/auto package-managed servers for updates. */
export async function checkAllServerUpdates(): Promise<{
  checked: number;
  updatesAvailable: number;
  checkedAt: string;
}> {
  return apiCall('check_all_server_updates');
}

/** Probe a single installed server for package updates. */
export async function checkServerVersion(
  spaceId: string,
  serverId: string
): Promise<{
  spaceId: string;
  serverId: string;
  currentVersion: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  checkedAt: string;
}> {
  return apiCall('check_server_version', { spaceId, serverId });
}

/**
 * Load gateway port settings (configured override, default, active).
 */
export async function getGatewayPortSettings(): Promise<GatewayPortSettings> {
  return apiCall('get_gateway_port_settings');
}

/**
 * Persist a custom gateway port. Takes effect on the next gateway start.
 */
export async function setGatewayPort(port: number): Promise<void> {
  return apiCall('set_gateway_port', { port });
}

/**
 * Clear the persisted gateway port override.
 */
export async function resetGatewayPort(): Promise<void> {
  return apiCall('reset_gateway_port');
}

/**
 * Persist the public HTTPS URL advertised in OAuth metadata for tunnel clients.
 */
export async function setGatewayPublicUrl(publicUrl: string): Promise<void> {
  return apiCall('set_gateway_public_url', { publicUrl });
}

/**
 * Resolve the on-disk application logs directory path.
 */
export async function getLogsPath(): Promise<string> {
  return apiCall('get_logs_path');
}

/**
 * Open the application logs folder in the system file manager.
 */
export async function openLogsFolder(): Promise<void> {
  return shellOpenLogsFolder();
}

/**
 * Load web admin mode settings (desktop only — controls :45819 server).
 */
export async function getAdminWebSettings(): Promise<AdminWebSettings> {
  return shellGetAdminWebSettings();
}

/**
 * Persist web admin settings and restart the admin HTTP server.
 */
export async function updateAdminWebSettings(settings: AdminWebSettings): Promise<void> {
  return shellUpdateAdminWebSettings(settings);
}
