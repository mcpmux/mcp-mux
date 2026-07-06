// This file is the ONE allowed place to import raw Tauri IPC (see the
// no-restricted-imports override in eslint.config.js); everything else in the
// UI goes through the transport facade.
import { invoke } from '@tauri-apps/api/core';
import { listen, type EventCallback } from '@tauri-apps/api/event';
import type { Transport } from './index';
import { DESKTOP_CAPABILITIES } from './capabilities';

/**
 * Desktop transport: forwards to Tauri IPC. `call` maps 1:1 to `invoke`, and
 * `subscribe` maps to `listen`, returning an unsubscribe function that matches
 * the previous `listen(...).then((un) => un())` cleanup shape.
 */
export const tauriTransport: Transport = {
  call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    // Forward with the same arity the call site used: `invoke(cmd)` when no
    // args, `invoke(cmd, args)` otherwise. Keeps the contract byte-identical
    // to direct `invoke` use (tests assert exact call arguments).
    return args === undefined ? invoke<T>(cmd) : invoke<T>(cmd, args);
  },

  subscribe(event: string, handler: (payload: unknown) => void): () => void {
    const cb: EventCallback<unknown> = (e) => handler(e.payload);
    const unlistenP = listen(event, cb);
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void unlistenP.then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  },

  capabilities: DESKTOP_CAPABILITIES,
};
