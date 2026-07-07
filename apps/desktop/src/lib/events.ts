/**
 * Event subscription facade — mirrors the shape of `@tauri-apps/api/event`'s
 * `listen`/`emit`/`once` so call sites swap the import and nothing else, but
 * routes through the transport: real Tauri IPC in the desktop shell, the
 * management-API SSE stream in the browser web admin. This is what lets the
 * SAME UI receive live change events in both runtimes (and, crucially, means
 * the browser never calls Tauri IPC — which would throw `transformCallback`).
 */
import { transport, isWebAdmin } from '@/lib/transport';

export type UnlistenFn = () => void;

/** A Tauri-style event object; call sites read `.payload`. */
export interface Event<T> {
  event: string;
  payload: T;
}
export type EventCallback<T> = (event: Event<T>) => void;

/**
 * Web-mode local registry: lets `emit` deliver to listeners registered via
 * this shim (the only "bus" a plain browser has — the gateway can't loop UI
 * events back). Unused in the Tauri shell, where the real event bus loops
 * emitted events back through `listen` and local dispatch would double-fire.
 */
const localListeners = new Map<string, Set<EventCallback<unknown>>>();

function addLocalListener(event: string, handler: EventCallback<unknown>): UnlistenFn {
  let set = localListeners.get(event);
  if (!set) {
    set = new Set();
    localListeners.set(event, set);
  }
  set.add(handler);
  return () => {
    set.delete(handler);
    if (set.size === 0) localListeners.delete(event);
  };
}

/**
 * Subscribe to `event`. Returns a Promise<UnlistenFn> to match the Tauri
 * signature (call sites do `listen(...).then((un) => un())`).
 */
export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  const unsubTransport = transport.subscribe(event, (payload) =>
    handler({ event, payload: payload as T })
  );
  const unsubLocal = isWebAdmin
    ? addLocalListener(event, handler as EventCallback<unknown>)
    : null;
  return Promise.resolve(() => {
    unsubTransport();
    unsubLocal?.();
  });
}

/** Subscribe once, then auto-unsubscribe. */
export function once<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  let done = false;
  let unsub: UnlistenFn = () => {};
  const wrapped: EventCallback<T> = (e) => {
    if (done) return;
    done = true;
    handler(e);
    unsub();
  };
  return listen<T>(event, wrapped).then((fn) => {
    unsub = fn;
    if (done) unsub();
    return unsub;
  });
}

/**
 * Emit an app event. In the Tauri shell this delegates to the real Tauri
 * event bus (E2E helpers inject events like `server-install-request` through
 * `window.__TAURI_TEST_API__.emit`); in the browser web admin it dispatches
 * to listeners registered via this shim's `listen`/`once`.
 */
export async function emit(event: string, payload?: unknown): Promise<void> {
  if (!isWebAdmin) {
    const { emit: tauriEmit } = await import('@tauri-apps/api/event');
    await tauriEmit(event, payload);
    return;
  }
  const set = localListeners.get(event);
  if (!set) return;
  for (const handler of [...set]) {
    handler({ event, payload });
  }
}
