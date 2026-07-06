import type { Capabilities } from './capabilities';
import { tauriTransport } from './tauri';
import { createHttpTransport } from './http';

export type { Capabilities } from './capabilities';

/**
 * Transport-independent surface the whole UI talks to. The desktop (Tauri)
 * build and the web-admin build share ONE codebase — only how `call` and
 * `subscribe` reach the backend differs. See `mcpmux.space/spikes/cloud-support/
 * 05-cloud-architecture.md §7`.
 */
export interface Transport {
  /** Invoke a backend command (mirrors the Tauri command contract 1:1). */
  call<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  /** Subscribe to a backend event; returns an unsubscribe function. */
  subscribe(event: string, handler: (payload: unknown) => void): () => void;
  /** What this platform can do (OS dialogs, fs writes, tray, …). */
  readonly capabilities: Capabilities;
}

/**
 * True when running inside the Tauri shell. Tauri injects `__TAURI_INTERNALS__`
 * on `window`; its absence means a plain browser (web admin). Tests set the
 * flag in `setup.ts` so the mocked IPC path is exercised.
 */
function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Storage key for the web-admin session token (set by the login gate). */
export const WEB_ADMIN_TOKEN_KEY = 'mcpmux_admin_token';

/** The active transport for this runtime. In the browser (web admin) the HTTP
 *  transport authenticates with the admin token the login gate stored. */
export const transport: Transport = isTauri()
  ? tauriTransport
  : createHttpTransport({
      getToken: () =>
        typeof localStorage !== 'undefined' ? localStorage.getItem(WEB_ADMIN_TOKEN_KEY) : null,
    });

/** True when running as the browser web admin (not the Tauri shell). */
export const isWebAdmin = !isTauri();

/** Convenience re-exports so call sites can `import { call, subscribe }`. */
export const call: Transport['call'] = (cmd, args) => transport.call(cmd, args);
export const subscribe: Transport['subscribe'] = (event, handler) =>
  transport.subscribe(event, handler);
export const capabilities: Capabilities = transport.capabilities;
