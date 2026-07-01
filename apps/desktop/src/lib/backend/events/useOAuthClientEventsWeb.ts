/**
 * SSE OAuth client change events for web admin mode.
 */

import { useCallback, useEffect, useRef } from 'react';

import { isTauri } from '../data/transport';

import {
  acquireAdminSseConsumer,
  releaseAdminSseConsumer,
  subscribeAdminSseRaw,
} from './admin-sse-hub';

import type { OAuthClientChangedPayload } from './useOAuthClientEvents';

/**
 * Subscribe to `oauth-client-changed` over the shared admin SSE hub in web admin mode.
 */
export function useOAuthClientEventsWeb() {
  const handlersRef = useRef<Set<(payload: OAuthClientChangedPayload) => void>>(new Set());

  useEffect(() => {
    if (isTauri()) {
      return;
    }

    acquireAdminSseConsumer();

    const unsubscribe = subscribeAdminSseRaw('oauth-client-changed', (payload) => {
      try {
        const data = payload as OAuthClientChangedPayload;
        handlersRef.current.forEach((handler) => handler(data));
      } catch {
        // ignore malformed frames
      }
    });

    return () => {
      unsubscribe();
      releaseAdminSseConsumer();
    };
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
