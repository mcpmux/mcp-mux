/**
 * SSE workspace event channels for web admin mode.
 *
 * Uses the shared admin-sse-hub connection instead of opening a second
 * EventSource, avoiding HTTP/1.1 connection starvation and ensuring workspace
 * events are gated behind enableAdminSse() like all other domain channels.
 */

import { useCallback, useEffect, useRef } from 'react';

import { isTauri } from '../data/transport';
import {
  acquireAdminSseConsumer,
  releaseAdminSseConsumer,
  subscribeAdminSseRaw,
} from './admin-sse-hub';

import type {
  WorkspaceChannelCallback,
  WorkspaceEventChannel,
  WorkspaceEventsCallback,
  WorkspacePayloadTypeMap,
} from './useWorkspaceEvents';

const ALL_WORKSPACE_CHANNELS: WorkspaceEventChannel[] = [
  'session-roots-changed',
  'workspace-binding-changed',
  'workspace-needs-binding',
];

/**
 * Subscribe to workspace-related SSE channels in web admin mode.
 */
export function useWorkspaceEventsWeb() {
  const handlersRef = useRef<
    Map<WorkspaceEventChannel, Set<(payload: WorkspacePayloadTypeMap[WorkspaceEventChannel]) => void>>
  >(new Map());

  useEffect(() => {
    if (isTauri()) {
      return;
    }

    acquireAdminSseConsumer();

    const unsubs = ALL_WORKSPACE_CHANNELS.map((channel) =>
      subscribeAdminSseRaw(channel, (payload) => {
        handlersRef.current
          .get(channel)
          ?.forEach((handler) =>
            handler(payload as WorkspacePayloadTypeMap[typeof channel])
          );
      })
    );

    return () => {
      unsubs.forEach((unsub) => unsub());
      releaseAdminSseConsumer();
    };
  }, []);

  /**
   * Subscribe to a single workspace SSE channel.
   */
  const subscribe = useCallback(
    <T extends WorkspaceEventChannel>(
      channel: T,
      callback: WorkspaceChannelCallback<T>
    ): (() => void) => {
      if (!handlersRef.current.has(channel)) {
        handlersRef.current.set(channel, new Set());
      }
      const wrapped = callback as (
        payload: WorkspacePayloadTypeMap[WorkspaceEventChannel]
      ) => void;
      handlersRef.current.get(channel)!.add(wrapped);
      return () => {
        handlersRef.current.get(channel)?.delete(wrapped);
      };
    },
    []
  );

  /**
   * Subscribe to multiple workspace SSE channels.
   */
  const subscribeMany = useCallback(
    (channels: WorkspaceEventChannel[], callback: WorkspaceEventsCallback): (() => void) => {
      const unsubs = channels.map((channel) =>
        subscribe(channel, (payload) => {
          callback(channel, payload);
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
    subscribeMany,
    channels: ALL_WORKSPACE_CHANNELS,
  };
}
