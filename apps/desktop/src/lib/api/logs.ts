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
 * Get recent logs for a server.
 */
export async function getServerLogs(
  serverId: string,
  limit?: number,
  levelFilter?: string
): Promise<ServerLogEntry[]> {
  return invoke('get_server_logs', {
    serverId,
    limit,
    levelFilter,
  });
}

/**
 * Clear logs for a server.
 */
export async function clearServerLogs(serverId: string): Promise<void> {
  return invoke('clear_server_logs', { serverId });
}

/**
 * Get the log file path for a server (for external viewers).
 */
export async function getServerLogFile(serverId: string): Promise<string> {
  return invoke('get_server_log_file', { serverId });
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

