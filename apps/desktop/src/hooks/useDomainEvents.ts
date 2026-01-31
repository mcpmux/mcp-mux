/**
 * useDomainEvents - Hook for subscribing to domain events from backend
 *
 * This hook provides a reactive interface to all domain events emitted
 * by the backend. Events are grouped by channel for easy subscription.
 *
 * ## Available Channels
 *
 * - `space-changed` - Space create/update/delete/activate
 * - `server-changed` - Server install/uninstall/enable/disable
 * - `server-status-changed` - Connection status updates
 * - `server-auth-progress` - OAuth countdown timer
 * - `server-features-refreshed` - Features discovered/updated
 * - `feature-set-changed` - Feature set create/update/delete
 * - `client-changed` - Client registration/update/delete
 * - `grants-changed` - Grant/revoke permissions
 * - `gateway-changed` - Gateway start/stop
 * - `mcp-notification` - MCP capability notifications
 *
 * ## Usage
 *
 * ```tsx
 * function MyComponent() {
 *   const { subscribe, subscribeAll, events } = useDomainEvents();
 *
 *   // Subscribe to specific channel
 *   useEffect(() => {
 *     return subscribe('server-status-changed', (payload) => {
 *       console.log('Server status:', payload.server_id, payload.status);
 *     });
 *   }, [subscribe]);
 *
 *   // Or subscribe to all events
 *   useEffect(() => {
 *     return subscribeAll((channel, payload) => {
 *       console.log(`[${channel}]`, payload);
 *     });
 *   }, [subscribeAll]);
 * }
 * ```
 */

import { useEffect, useCallback, useRef, useState } from 'react';
import { listen, UnlistenFn, Event } from '@tauri-apps/api/event';

// ============================================================================
// TYPES
// ============================================================================

/** Domain event channels */
export type DomainEventChannel =
  | 'space-changed'
  | 'server-changed'
  | 'server-status-changed'
  | 'server-auth-progress'
  | 'server-features-refreshed'
  | 'feature-set-changed'
  | 'client-changed'
  | 'grants-changed'
  | 'gateway-changed'
  | 'mcp-notification';

/** Base event payload */
export interface DomainEventPayload {
  action?: string;
  [key: string]: unknown;
}

/** Space event payloads */
export interface SpaceChangedPayload extends DomainEventPayload {
  action: 'created' | 'updated' | 'deleted' | 'activated';
  space_id: string;
  name?: string;
  icon?: string;
  from_space_id?: string;
  to_space_id?: string;
  to_space_name?: string;
}

/** Server lifecycle event payloads */
export interface ServerChangedPayload extends DomainEventPayload {
  action: 'installed' | 'uninstalled' | 'config_updated' | 'enabled' | 'disabled';
  space_id: string;
  server_id: string;
  server_name?: string;
}

/** Server status event payload */
export interface ServerStatusChangedPayload extends DomainEventPayload {
  space_id: string;
  server_id: string;
  status: 'connected' | 'disconnected' | 'connecting' | 'error' | 'oauth_required' | 'refreshing' | 'authenticating';
  has_connected_before: boolean;
  message?: string;
  features?: {
    tools_count: number;
    prompts_count: number;
    resources_count: number;
  };
}

/** Server auth progress payload */
export interface ServerAuthProgressPayload extends DomainEventPayload {
  space_id: string;
  server_id: string;
  remaining_seconds: number;
  flow_id: number;
}

/** Server features refreshed payload */
export interface ServerFeaturesRefreshedPayload extends DomainEventPayload {
  space_id: string;
  server_id: string;
  tools_count: number;
  prompts_count: number;
  resources_count: number;
  added: string[];
  removed: string[];
}

/** Feature set event payloads */
export interface FeatureSetChangedPayload extends DomainEventPayload {
  action: 'created' | 'updated' | 'deleted' | 'members_changed';
  space_id: string;
  feature_set_id: string;
  name?: string;
  feature_set_type?: string;
  added_count?: number;
  removed_count?: number;
}

/** Client event payloads */
export interface ClientChangedPayload extends DomainEventPayload {
  action: 'registered' | 'updated' | 'deleted' | 'token_issued';
  client_id: string;
  client_name?: string;
  registration_type?: string;
}

/** Grant event payloads */
export interface GrantsChangedPayload extends DomainEventPayload {
  action: 'granted' | 'revoked' | 'batch_updated';
  client_id: string;
  space_id: string;
  feature_set_id?: string;
  feature_set_ids?: string[];
}

/** Gateway event payloads */
export interface GatewayChangedPayload extends DomainEventPayload {
  action: 'started' | 'stopped';
  url?: string;
  port?: number;
}

/** MCP notification payload */
export interface MCPNotificationPayload extends DomainEventPayload {
  type: 'tools_changed' | 'prompts_changed' | 'resources_changed';
  space_id: string;
  server_id: string;
}

/** Payload type map for type safety */
export interface PayloadTypeMap {
  'space-changed': SpaceChangedPayload;
  'server-changed': ServerChangedPayload;
  'server-status-changed': ServerStatusChangedPayload;
  'server-auth-progress': ServerAuthProgressPayload;
  'server-features-refreshed': ServerFeaturesRefreshedPayload;
  'feature-set-changed': FeatureSetChangedPayload;
  'client-changed': ClientChangedPayload;
  'grants-changed': GrantsChangedPayload;
  'gateway-changed': GatewayChangedPayload;
  'mcp-notification': MCPNotificationPayload;
}

/** Type-safe callback for specific channels */
export type ChannelCallback<T extends DomainEventChannel> = (
  payload: PayloadTypeMap[T]
) => void;

/** Callback for all events */
export type AllEventsCallback = (
  channel: DomainEventChannel,
  payload: DomainEventPayload
) => void;

// ============================================================================
// HOOK IMPLEMENTATION
// ============================================================================

/** All channels that can receive events */
const ALL_CHANNELS: DomainEventChannel[] = [
  'space-changed',
  'server-changed',
  'server-status-changed',
  'server-auth-progress',
  'server-features-refreshed',
  'feature-set-changed',
  'client-changed',
  'grants-changed',
  'gateway-changed',
  'mcp-notification',
];

/**
 * Hook for subscribing to domain events from the backend
 */
export function useDomainEvents() {
  // Track active listeners for cleanup
  const activeListeners = useRef<UnlistenFn[]>([]);
  const [lastEvent, setLastEvent] = useState<{
    channel: DomainEventChannel;
    payload: DomainEventPayload;
  } | null>(null);

  // Cleanup all listeners on unmount
  useEffect(() => {
    return () => {
      activeListeners.current.forEach((unlisten) => unlisten());
      activeListeners.current = [];
    };
  }, []);

  /**
   * Subscribe to a specific event channel
   * Returns unsubscribe function
   */
  const subscribe = useCallback(
    <T extends DomainEventChannel>(channel: T, callback: ChannelCallback<T>): (() => void) => {
      let unlistenFn: UnlistenFn | null = null;

      listen(channel, (event: Event<PayloadTypeMap[T]>) => {
        callback(event.payload);
        setLastEvent({ channel, payload: event.payload as DomainEventPayload });
      }).then((unlisten) => {
        unlistenFn = unlisten;
        activeListeners.current.push(unlisten);
      });

      // Return cleanup function
      return () => {
        if (unlistenFn) {
          unlistenFn();
          activeListeners.current = activeListeners.current.filter(
            (fn) => fn !== unlistenFn
          );
        }
      };
    },
    []
  );

  /**
   * Subscribe to all event channels
   * Returns unsubscribe function
   */
  const subscribeAll = useCallback((callback: AllEventsCallback): (() => void) => {
    const unlisteners: (() => void)[] = [];

    for (const channel of ALL_CHANNELS) {
      const unsub = subscribe(channel, (payload) => {
        callback(channel, payload);
      });
      unlisteners.push(unsub);
    }

    return () => {
      unlisteners.forEach((unsub) => unsub());
    };
  }, [subscribe]);

  /**
   * Subscribe to multiple channels with the same callback
   */
  const subscribeMany = useCallback(
    (channels: DomainEventChannel[], callback: AllEventsCallback): (() => void) => {
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
    /** Subscribe to a specific channel */
    subscribe,
    /** Subscribe to all channels */
    subscribeAll,
    /** Subscribe to multiple specific channels */
    subscribeMany,
    /** Last received event (for debugging) */
    lastEvent,
    /** Available channels */
    channels: ALL_CHANNELS,
  };
}

// ============================================================================
// CONVENIENCE HOOKS
// ============================================================================

/**
 * Hook that subscribes to space changes
 */
export function useSpaceEvents(callback: ChannelCallback<'space-changed'>) {
  const { subscribe } = useDomainEvents();

  useEffect(() => {
    return subscribe('space-changed', callback);
  }, [subscribe, callback]);
}

/**
 * Hook that subscribes to server status changes
 */
export function useServerStatusEvents(callback: ChannelCallback<'server-status-changed'>) {
  const { subscribe } = useDomainEvents();

  useEffect(() => {
    return subscribe('server-status-changed', callback);
  }, [subscribe, callback]);
}

/**
 * Hook that subscribes to server auth progress
 */
export function useServerAuthProgress(callback: ChannelCallback<'server-auth-progress'>) {
  const { subscribe } = useDomainEvents();

  useEffect(() => {
    return subscribe('server-auth-progress', callback);
  }, [subscribe, callback]);
}

/**
 * Hook that subscribes to client/grant changes
 */
export function useClientEvents(
  callback: AllEventsCallback
) {
  const { subscribeMany } = useDomainEvents();

  useEffect(() => {
    return subscribeMany(['client-changed', 'grants-changed'], callback);
  }, [subscribeMany, callback]);
}

/**
 * Hook that subscribes to gateway state changes
 */
export function useGatewayEvents(callback: ChannelCallback<'gateway-changed'>) {
  const { subscribe } = useDomainEvents();

  useEffect(() => {
    return subscribe('gateway-changed', callback);
  }, [subscribe, callback]);
}

export default useDomainEvents;

