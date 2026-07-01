/**
 * useOAuthClientEvents — OAuth dynamic client registration changes.
 *
 * Subscribes to `oauth-client-changed`, emitted directly from oauth.rs when
 * OAuth clients are created, updated, or deleted (not via EventBus bridge).
 */

import { useCallback, useEffect, useRef } from 'react';
import { listen, UnlistenFn, Event } from '@tauri-apps/api/event';

import { isTauri } from '../data/transport';

import { useOAuthClientEventsWeb } from './useOAuthClientEventsWeb';

/** Payload for `oauth-client-changed`. */
export interface OAuthClientChangedPayload {
  action?: 'created' | 'updated' | 'deleted';
  client_id?: string;
  [key: string]: unknown;
}

/**
 * Hook for subscribing to OAuth client change events (Tauri).
 */
function useOAuthClientEventsTauri() {
  const activeListeners = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    return () => {
      activeListeners.current.forEach((unlisten) => unlisten());
      activeListeners.current = [];
    };
  }, []);

  /**
   * Subscribe to `oauth-client-changed`.
   * Returns an unsubscribe function.
   */
  const subscribe = useCallback(
    (callback: (payload: OAuthClientChangedPayload) => void): (() => void) => {
      if (!isTauri()) {
        return () => {};
      }
      let unlistenFn: UnlistenFn | null = null;

      listen('oauth-client-changed', (event: Event<OAuthClientChangedPayload>) => {
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
 * Hook for OAuth client events — Tauri on desktop, SSE on web admin.
 */
export function useOAuthClientEvents() {
  const tauri = useOAuthClientEventsTauri();
  const web = useOAuthClientEventsWeb();
  return isTauri() ? tauri : web;
}

/**
 * Convenience hook — invokes callback when an OAuth client changes.
 */
export function useOAuthClientEventListener(callback: () => void): void {
  const { subscribe } = useOAuthClientEvents();

  useEffect(() => {
    return subscribe(() => {
      callback();
    });
  }, [subscribe, callback]);
}

export default useOAuthClientEvents;
