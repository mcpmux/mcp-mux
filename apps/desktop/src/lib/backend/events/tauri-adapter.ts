import { listen, type Event, type UnlistenFn } from '@tauri-apps/api/event';

import { isTauri } from '../data/transport';

/**
 * Subscribe to a Tauri IPC event channel (desktop only; no-op on web).
 */
export async function listenWhenTauri<T>(
  event: string,
  handler: (event: Event<T>) => void
): Promise<UnlistenFn | undefined> {
  if (!isTauri()) {
    return undefined;
  }
  return listen(event, handler);
}
