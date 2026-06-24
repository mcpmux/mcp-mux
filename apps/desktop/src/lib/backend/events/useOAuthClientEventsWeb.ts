/**
 * SSE OAuth client change events for web admin mode.
 */

import { useCallback, useEffect, useRef } from 'react';

import { isTauri } from '../data/transport';

import type { OAuthClientChangedPayload } from './useOAuthClientEvents';

/**
 * Subscribe to `oauth-client-changed` over SSE in web admin mode.
 */
export function useOAuthClientEventsWeb() {
  const handlersRef = useRef<Set<(payload: OAuthClientChangedPayload) => void>>(new Set());

  useEffect(() => {
    if (isTauri()) {
      return;
    }
    const source = new EventSource('/api/v1/events');

    source.addEventListener('oauth-client-changed', (event: MessageEvent<string>) => {
      try {
        const payload = JSON.parse(event.data) as OAuthClientChangedPayload;
        handlersRef.current.forEach((handler) => handler(payload));
      } catch {
        // ignore malformed frames
      }
    });

    return () => source.close();
  }, []);

  /**
   * Subscribe to OAuth client SSE events.
   */
  const subscribe = useCallback(
    (callback: (payload: OAuthClientChangedPayload) => void): (() => void) => {
      handlersRef.current.add(callback);
      return () => {
        handlersRef.current.delete(callback);
      };
    },
    []
  );

  return { subscribe };
}
