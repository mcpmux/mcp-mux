import { invoke } from '@tauri-apps/api/core';

/**
 * Server log entry from the backend.
 */
export interface ServerLogEntry {
  timestamp: string;
  level: string;
  source: string;
  message: string;
  metadata?: Record<string, unknown>;
}

/**
 * Get recent logs for a server in a Space. Logs are stored per Space, so the
 * Space the server is viewed in is part of the lookup.
 */
export async function getServerLogs(
  serverId: string,
  spaceId: string,
  limit?: number,
  levelFilter?: string
): Promise<ServerLogEntry[]> {
  return invoke('get_server_logs', {
    serverId,
    spaceId,
    limit,
    levelFilter,
  });
}

/**
 * Clear logs for a server in a Space.
 */
export async function clearServerLogs(serverId: string, spaceId: string): Promise<void> {
  return invoke('clear_server_logs', { serverId, spaceId });
}

/**
 * Get the log file path for a server in a Space (for external viewers).
 */
export async function getServerLogFile(serverId: string, spaceId: string): Promise<string> {
  return invoke('get_server_log_file', { serverId, spaceId });
}

/**
 * Get log retention period in days (0 = keep forever).
 */
export async function getLogRetentionDays(): Promise<number> {
  return invoke('get_log_retention_days');
}

/**
 * Set log retention period in days (0 = keep forever).
 * Triggers an immediate cleanup with the new setting.
 */
export async function setLogRetentionDays(days: number): Promise<void> {
  return invoke('set_log_retention_days', { days });
}

