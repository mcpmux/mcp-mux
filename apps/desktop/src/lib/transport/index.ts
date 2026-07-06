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

/** The active transport for this runtime. */
export const transport: Transport = isTauri() ? tauriTransport : createHttpTransport();

/** Convenience re-exports so call sites can `import { call, subscribe }`. */
export const call: Transport['call'] = (cmd, args) => transport.call(cmd, args);
export const subscribe: Transport['subscribe'] = (event, handler) =>
  transport.subscribe(event, handler);
export const capabilities: Capabilities = transport.capabilities;
