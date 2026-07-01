/**
 * useWorkspaceEvents — workspace binding and session roots channels.
 *
 * Subscribes to Tauri events emitted by the gateway bridge:
 * - `session-roots-changed` — EventBus bridge (reported roots updated)
 * - `workspace-binding-changed` — EventBus bridge (binding + appearance writes)
 * - `workspace-needs-binding` — EventBus bridge (unbound root prompt)
 */

import { useCallback, useEffect, useRef } from 'react';
import { listen, UnlistenFn, Event } from '@tauri-apps/api/event';

import { isTauri } from '../data/transport';

import { useWorkspaceEventsWeb } from './useWorkspaceEventsWeb';

/** Workspace-related Tauri event channels. */
export type WorkspaceEventChannel =
  | 'session-roots-changed'
  | 'workspace-binding-changed'
  | 'workspace-needs-binding';

/** Payload for `workspace-binding-changed` (binding or appearance update). */
export interface WorkspaceBindingChangedPayload {
  space_id?: string;
  workspace_root: string;
}

/** Payload for `workspace-needs-binding`. */
export interface WorkspaceNeedsBindingPayload {
  client_id: string;
  session_id: string;
  space_id: string;
  workspace_root: string;
  /** When true, the Space picker is locked to `space_id`. */
  space_locked?: boolean;
}

/** Payload map for type-safe subscriptions. */
export interface WorkspacePayloadTypeMap {
  'session-roots-changed': Record<string, never>;
  'workspace-binding-changed': WorkspaceBindingChangedPayload;
  'workspace-needs-binding': WorkspaceNeedsBindingPayload;
}

/** Callback for a specific workspace channel. */
export type WorkspaceChannelCallback<T extends WorkspaceEventChannel> = (
  payload: WorkspacePayloadTypeMap[T]
) => void;

/** Callback receiving channel name and payload. */
export type WorkspaceEventsCallback = <T extends WorkspaceEventChannel>(
  channel: T,
  payload: WorkspacePayloadTypeMap[T]
) => void;

const ALL_WORKSPACE_CHANNELS: WorkspaceEventChannel[] = [
  'session-roots-changed',
  'workspace-binding-changed',
  'workspace-needs-binding',
];

/**
 * Hook for subscribing to workspace-related Tauri event channels.
 */
function useWorkspaceEventsTauri() {
  const activeListeners = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    return () => {
      activeListeners.current.forEach((unlisten) => unlisten());
      activeListeners.current = [];
    };
  }, []);

  /**
   * Subscribe to a single workspace event channel.
   * Returns an unsubscribe function.
   */
  const subscribe = useCallback(
    <T extends WorkspaceEventChannel>(
      channel: T,
      callback: WorkspaceChannelCallback<T>
    ): (() => void) => {
      if (!isTauri()) {
        return () => {};
      }
      let unlistenFn: UnlistenFn | null = null;

      listen(channel, (event: Event<WorkspacePayloadTypeMap[T]>) => {
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

  /**
   * Subscribe to multiple workspace channels with one callback.
   * Returns an unsubscribe function.
   */
  const subscribeMany = useCallback(
    (channels: WorkspaceEventChannel[], callback: WorkspaceEventsCallback): (() => void) => {
      const unlisteners: (() => void)[] = [];

      for (const channel of channels) {
        const unsub = subscribe(channel, (payload) => {
          callback(channel, payload);
        });
        unlisteners.push(unsub);
      }

      return () => {
        unlisteners.forEach((unsub) => unsub());
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

/**
 * Hook for workspace events — Tauri on desktop, SSE on web admin.
 */
export function useWorkspaceEvents() {
  const tauri = useWorkspaceEventsTauri();
  const web = useWorkspaceEventsWeb();
  return isTauri() ? tauri : web;
}

/**
 * Convenience hook — invokes callback when any workspace channel fires.
 */
export function useWorkspaceEventListener(
  callback: WorkspaceEventsCallback,
  channels: WorkspaceEventChannel[] = ALL_WORKSPACE_CHANNELS
): void {
  const { subscribeMany } = useWorkspaceEvents();

  useEffect(() => {
    return subscribeMany(channels, callback);
  }, [subscribeMany, callback, channels]);
}

export default useWorkspaceEvents;
