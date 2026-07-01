/**
 * SSE meta-tool invocation events for web admin mode.
 */

import { useCallback, useEffect, useRef } from 'react';

import type { MetaToolAuditEvent } from '@/lib/api/metaTools';

import { isTauri } from '../data/transport';

import {
  acquireAdminSseConsumer,
  releaseAdminSseConsumer,
  subscribeAdminSseRaw,
} from './admin-sse-hub';

/**
 * Subscribe to `meta-tool-invoked` over the shared admin SSE hub in web admin mode.
 */
export function useMetaToolEventsWeb() {
  const handlersRef = useRef<Set<(event: MetaToolAuditEvent) => void>>(new Set());

  useEffect(() => {
    if (isTauri()) {
      return;
    }

    acquireAdminSseConsumer();

    const unsubscribe = subscribeAdminSseRaw('meta-tool-invoked', (payload) => {
      try {
        const data = payload as MetaToolAuditEvent;
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
   * Subscribe to meta-tool SSE events.
   */
  const subscribe = useCallback((callback: (event: MetaToolAuditEvent) => void): (() => void) => {
    handlersRef.current.add(callback);
    return () => {
      handlersRef.current.delete(callback);
    };
  }, []);

  return { subscribe };
}
