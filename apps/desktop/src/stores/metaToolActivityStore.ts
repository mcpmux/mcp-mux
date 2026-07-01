/**
 * Global, navigation-persistent log of `mcpmux_*` meta-tool invocations.
 *
 * The audit panel (`MetaToolAuditLog`) previously kept rows in component-local
 * state with a `meta-tool-invoked` listener mounted only while the panel was
 * visible. That meant the list vanished on tab change and showed empty if you
 * navigated in *after* a call had already fired. This store lifts both the rows
 * and the listener to app scope: the listener starts once at launch and rows
 * persist for the whole UI session.
 */

import { create } from 'zustand';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { MetaToolAuditEvent } from '@/lib/api/metaTools';
import {
  acquireAdminSseConsumer,
  releaseAdminSseConsumer,
  subscribeAdminSseRaw,
} from '@/lib/backend/events/admin-sse-hub';
import { isTauri } from '@/lib/backend/data/transport';

/** Ring-buffer size — most recent N invocations kept in memory. */
export const MAX_META_TOOL_ROWS = 50;

interface MetaToolActivityState {
  rows: MetaToolAuditEvent[];
  push: (event: MetaToolAuditEvent) => void;
  clear: () => void;
}

export const useMetaToolActivityStore = create<MetaToolActivityState>((set) => ({
  rows: [],
  push: (event) =>
    set((state) => {
      // Most-recent-first; trim to the ring-buffer size.
      const next = [event, ...state.rows];
      return { rows: next.length > MAX_META_TOOL_ROWS ? next.slice(0, MAX_META_TOOL_ROWS) : next };
    }),
  clear: () => set({ rows: [] }),
}));

// Module-level singleton listener so it survives component unmounts (tab
// changes) and is wired exactly once regardless of how many callers init it.
let listening = false;
let unlistenPromise: Promise<UnlistenFn> | null = null;
let sseUnsubscribe: (() => void) | null = null;

/**
 * Start the app-wide `meta-tool-invoked` listener (idempotent). Call once near
 * the app root so activity accumulates for the whole session, independent of
 * which tab is currently mounted.
 */
export function startMetaToolActivityListener(): void {
  if (listening) return;
  listening = true;

  if (isTauri()) {
    unlistenPromise = listen<MetaToolAuditEvent>('meta-tool-invoked', (event) => {
      useMetaToolActivityStore.getState().push(event.payload);
    });
    return;
  }

  acquireAdminSseConsumer();

  sseUnsubscribe = subscribeAdminSseRaw('meta-tool-invoked', (payload) => {
    try {
      const data = payload as MetaToolAuditEvent;
      useMetaToolActivityStore.getState().push(data);
    } catch {
      // ignore malformed frames
    }
  });

  unlistenPromise = Promise.resolve(() => {
    sseUnsubscribe?.();
    sseUnsubscribe = null;
    releaseAdminSseConsumer();
  });
}

/** Tear down the listener (mainly for tests / hot-reload hygiene). */
export function stopMetaToolActivityListener(): void {
  listening = false;
  void unlistenPromise?.then((fn) => fn()).catch(() => {});
  unlistenPromise = null;
  sseUnsubscribe = null;
}
