/**
 * SSE workspace event channels for web admin mode.
 */

import { useCallback, useEffect, useRef } from 'react';

import { isTauri } from '../data/transport';

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
    const source = new EventSource('/api/v1/events');

    for (const channel of ALL_WORKSPACE_CHANNELS) {
      source.addEventListener(channel, (event: MessageEvent<string>) => {
        try {
          const payload = JSON.parse(event.data) as WorkspacePayloadTypeMap[typeof channel];
          handlersRef.current.get(channel)?.forEach((handler) => handler(payload));
        } catch {
          // ignore malformed frames
        }
      });
    }

    return () => source.close();
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
