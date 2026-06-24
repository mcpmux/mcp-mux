/**
 * SSE meta-tool invocation events for web admin mode.
 */

import { useCallback, useEffect, useRef } from 'react';

import type { MetaToolAuditEvent } from '@/lib/api/metaTools';

import { isTauri } from '../data/transport';

/**
 * Subscribe to `meta-tool-invoked` over SSE in web admin mode.
 */
export function useMetaToolEventsWeb() {
  const handlersRef = useRef<Set<(event: MetaToolAuditEvent) => void>>(new Set());

  useEffect(() => {
    if (isTauri()) {
      return;
    }
    const source = new EventSource('/api/v1/events');

    source.addEventListener('meta-tool-invoked', (event: MessageEvent<string>) => {
      try {
        const payload = JSON.parse(event.data) as MetaToolAuditEvent;
        handlersRef.current.forEach((handler) => handler(payload));
      } catch {
        // ignore malformed frames
      }
    });

    return () => source.close();
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
