/**
 * useMetaToolEvents — meta-tool invocation audit stream.
 *
 * Subscribes to `meta-tool-invoked`, emitted by the EventBus → gateway bridge
 * when any `mcpmux_*` tool runs (read or write, all decision outcomes).
 */

import { useCallback, useEffect, useRef } from 'react';
import { listen, UnlistenFn, Event } from '@tauri-apps/api/event';

import type { MetaToolAuditEvent } from '@/lib/api/metaTools';

import { isTauri } from '../data/transport';

import { useMetaToolEventsWeb } from './useMetaToolEventsWeb';

/**
 * Hook for subscribing to meta-tool invocation events (Tauri).
 */
function useMetaToolEventsTauri() {
  const activeListeners = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    return () => {
      activeListeners.current.forEach((unlisten) => unlisten());
      activeListeners.current = [];
    };
  }, []);

  /**
   * Subscribe to `meta-tool-invoked`.
   * Returns an unsubscribe function.
   */
  const subscribe = useCallback(
    (callback: (event: MetaToolAuditEvent) => void): (() => void) => {
      if (!isTauri()) {
        return () => {};
      }
      let unlistenFn: UnlistenFn | null = null;

      listen('meta-tool-invoked', (event: Event<MetaToolAuditEvent>) => {
        callback(event.payload);
      }).then((unlisten) => {
        unlistenFn = unlisten;
        activeListeners.current.push(unlisten);
      });

      return () => {
        if (unlistenFn) {
          unlistenFn();
          activeListeners.current = activeListeners.current.filter((fn) => fn !== unlistenFn);
        }
      };
    },
    []
  );

  return { subscribe };
}

/**
 * Hook for meta-tool events — Tauri on desktop, SSE on web admin.
 */
export function useMetaToolEvents() {
  const tauri = useMetaToolEventsTauri();
  const web = useMetaToolEventsWeb();
  return isTauri() ? tauri : web;
}

/**
 * Convenience hook — invokes callback on every meta-tool invocation.
 */
export function useMetaToolEventListener(callback: (event: MetaToolAuditEvent) => void): void {
  const { subscribe } = useMetaToolEvents();

  useEffect(() => {
    return subscribe(callback);
  }, [subscribe, callback]);
}

export default useMetaToolEvents;
