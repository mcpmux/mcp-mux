/**
 * Event subscription facade — mirrors the shape of `@tauri-apps/api/event`'s
 * `listen`/`emit`/`once` so call sites swap the import and nothing else, but
 * routes through the transport: real Tauri IPC in the desktop shell, the
 * management-API SSE stream in the browser web admin. This is what lets the
 * SAME UI receive live change events in both runtimes (and, crucially, means
 * the browser never calls Tauri IPC — which would throw `transformCallback`).
 */
import { transport } from '@/lib/transport';

export type UnlistenFn = () => void;

/** A Tauri-style event object; call sites read `.payload`. */
export interface Event<T> {
  event: string;
  payload: T;
}
export type EventCallback<T> = (event: Event<T>) => void;

/**
 * Subscribe to `event`. Returns a Promise<UnlistenFn> to match the Tauri
 * signature (call sites do `listen(...).then((un) => un())`).
 */
export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  const unsub = transport.subscribe(event, (payload) =>
    handler({ event, payload: payload as T })
  );
  return Promise.resolve(unsub);
}

/** Subscribe once, then auto-unsubscribe. */
export function once<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  let unsub: UnlistenFn = () => {};
  unsub = transport.subscribe(event, (payload) => {
    handler({ event, payload: payload as T });
    unsub();
  });
  return Promise.resolve(unsub);
}

/**
 * Emit an app event. Emitting FROM the UI is a desktop-only affordance (the
 * gateway is the event source); in the browser web admin this is a no-op.
 */
export async function emit(_event: string, _payload?: unknown): Promise<void> {
  /* no-op: the UI does not originate domain events in web mode */
}
