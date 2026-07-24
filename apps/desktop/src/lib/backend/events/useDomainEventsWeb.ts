/**
 * SSE-based domain event listener for web admin mode.
 *
 * Mirrors the `useDomainEvents` API (`subscribe`, `subscribeAll`, `subscribeMany`)
 * using a single shared `GET /api/v1/events` connection (`admin-sse-hub.ts`).
 */

import { useCallback, useEffect, useState } from 'react';

import {
  acquireAdminSseConsumer,
  onAdminSseLastEvent,
  releaseAdminSseConsumer,
  subscribeAdminSseAll,
  subscribeAdminSseChannel,
} from './admin-sse-hub';

import type {
  AllEventsCallback,
  ChannelCallback,
  DomainEventChannel,
  DomainEventPayload,
  PayloadTypeMap,
} from './useDomainEvents';
import { ADMIN_SSE_CHANNELS } from './admin-sse-hub';

/**
 * Subscribe to admin SSE domain events in web mode (shared EventSource).
 */
export function useDomainEventsWeb() {
  const [lastEvent, setLastEvent] = useState<{
    channel: DomainEventChannel;
    payload: DomainEventPayload;
  } | null>(null);

  useEffect(() => {
    acquireAdminSseConsumer();
    const offLast = onAdminSseLastEvent(setLastEvent);
    return () => {
      offLast();
      releaseAdminSseConsumer();
    };
  }, []);

  /**
   * Subscribe to a specific SSE event channel.
   */
  const subscribe = useCallback(
    <T extends DomainEventChannel>(channel: T, callback: ChannelCallback<T>): (() => void) => {
      const wrapped = callback as (payload: DomainEventPayload) => void;
      return subscribeAdminSseChannel(channel, wrapped);
    },
    []
  );

  /**
   * Subscribe to all domain SSE channels.
   */
  const subscribeAll = useCallback((callback: AllEventsCallback): () => void => {
    return subscribeAdminSseAll(callback);
  }, []);

  /**
   * Subscribe to multiple domain SSE channels with one callback.
   */
  const subscribeMany = useCallback(
    (channels: DomainEventChannel[], callback: AllEventsCallback): (() => void) => {
      const unsubs = channels.map((channel) =>
        subscribe(channel, (payload) => {
          callback(channel, payload as PayloadTypeMap[typeof channel]);
        })
      );
      return () => {
        unsubs.forEach((unsub) => unsub());
      };
    },
    [subscribe]
  );

  return {
    subscribe,
    subscribeAll,
    subscribeMany,
    lastEvent,
    channels: ADMIN_SSE_CHANNELS,
  };
}
