import { invoke } from '@tauri-apps/api/core';

import { fetchApi } from './fetch-api';

/**
 * Returns true when running inside the Tauri desktop shell.
 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Dispatch a backend command through Tauri IPC or the admin REST API.
 */
export async function apiCall<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (isTauri()) {
    return invoke<T>(command, args);
  }
  return fetchApi<T>(command, args);
}
