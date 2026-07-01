/**
 * useServerManager - React hook for event-driven server management.
 *
 * Provides:
 * - Automatic subscription to server events via the useDomainEvents facade
 * - Local state management for server statuses
 * - Helper functions for common operations
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  type ConnectionStatus,
  type FeaturesUpdatedEvent,
  type ServerStatusResponse,
  cancelAuth,
  disableServer,
  enableServer,
  getConnectButtonLabel,
  getServerAction,
  getServerStatuses,
  retryConnection,
  startAuth,
} from '../lib/api/serverManager';
import { listServerFeaturesByServer } from '../lib/api/serverFeatures';
import {
  useDomainEvents,
  type ServerAuthProgressPayload,
  type ServerFeaturesRefreshedPayload,
  type ServerStatusChangedPayload,
} from './useDomainEvents';

interface UseServerManagerOptions {
  spaceId: string;
  /** Called when features are updated */
  onFeaturesChange?: (event: FeaturesUpdatedEvent) => void;
}

interface UseServerManagerResult {
  /** Map of server_id -> status info */
  statuses: Record<string, ServerStatusResponse>;
  /** Loading state for initial fetch */
  loading: boolean;
  /** Error message if initial fetch failed */
  error: string | null;
  /** Auth progress for servers in Authenticating state (server_id -> remaining seconds) */
  authProgress: Record<string, number>;
  /** Enable and connect a server */
  enable: (serverId: string) => Promise<void>;
  /** Disable a server */
  disable: (serverId: string) => Promise<void>;
  /** Start OAuth flow */
  connect: (serverId: string) => Promise<void>;
  /** Cancel OAuth flow */
  cancel: (serverId: string) => Promise<void>;
  /** Retry connection */
  retry: (serverId: string) => Promise<void>;
  /** Get button label for a server */
  getButtonLabel: (serverId: string) => string;
  /** Get action type for a server */
  getAction: (
    serverId: string
  ) =>
    | 'enable'
    | 'disable'
    | 'connect'
    | 'cancel'
    | 'retry'
    | 'connected'
    | 'connecting';
  /** Refresh statuses from backend */
  refresh: () => Promise<void>;
}

/**
 * Normalize backend status strings to the UI ConnectionStatus union.
 */
function normalizeConnectionStatus(status: string): ConnectionStatus {
  if (status === 'auth_required') {
    return 'oauth_required';
  }
  return status as ConnectionStatus;
}

/**
 * Map a REST/Tauri status payload into ServerStatusResponse.
 */
function toServerStatusResponse(
  serverId: string,
  payload: Pick<ServerStatusResponse, 'flow_id' | 'has_connected_before' | 'message'> & {
    status: string;
  }
): ServerStatusResponse {
  return {
    server_id: serverId,
    status: normalizeConnectionStatus(payload.status),
    flow_id: payload.flow_id,
    has_connected_before: payload.has_connected_before,
    message: payload.message ?? null,
  };
}

/**
 * Event-driven hook for managing MCP server connection state.
 * Uses the useDomainEvents facade so it works on both Tauri desktop and web admin.
 */
export function useServerManager({
  spaceId,
  onFeaturesChange,
}: UseServerManagerOptions): UseServerManagerResult {
  const [statuses, setStatuses] = useState<Record<string, ServerStatusResponse>>({});
  const [authProgress, setAuthProgress] = useState<Record<string, number>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const prevSpaceId = useRef<string | null>(null);

  const onFeaturesChangeRef = useRef(onFeaturesChange);
  onFeaturesChangeRef.current = onFeaturesChange;

  const { subscribe } = useDomainEvents();

  const refresh = useCallback(async () => {
    if (!spaceId) return;

    try {
      setLoading(true);
      setError(null);
      const result = await getServerStatuses(spaceId);
      const normalized = Object.fromEntries(
        Object.entries(result).map(([serverId, status]) => [
          serverId,
          toServerStatusResponse(serverId, status),
        ])
      );
      setStatuses(normalized);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
    } finally {
      setLoading(false);
    }
  }, [spaceId]);

  useEffect(() => {
    if (!spaceId || prevSpaceId.current !== spaceId) {
      setStatuses({});
      setAuthProgress({});
      setError(null);
      setLoading(true);
      prevSpaceId.current = spaceId || null;

      if (spaceId) {
        void refresh();
      }
    }
  }, [spaceId, refresh]);

  useEffect(() => {
    if (!prevSpaceId.current) {
      void refresh();
    }
  }, [refresh]);

  useEffect(() => {
    if (!spaceId) return;

    const unsubs = [
      subscribe('server-status-changed', (event: ServerStatusChangedPayload) => {
        if (event.space_id !== spaceId) return;

        setStatuses((prev) => {
          const existing = prev[event.server_id];
          return {
            ...prev,
            [event.server_id]: toServerStatusResponse(event.server_id, {
              status: event.status,
              flow_id: existing?.flow_id ?? 0,
              has_connected_before:
                event.has_connected_before ??
                (existing?.has_connected_before ?? false),
              message: event.message ?? null,
            }),
          };
        });

        if (event.status !== 'authenticating') {
          setAuthProgress((prev) => {
            const next = { ...prev };
            delete next[event.server_id];
            return next;
          });
        }
      }),
      subscribe('server-auth-progress', (event: ServerAuthProgressPayload) => {
        if (event.space_id !== spaceId) return;

        setAuthProgress((prev) => ({
          ...prev,
          [event.server_id]: event.remaining_seconds,
        }));
      }),
      subscribe('server-features-refreshed', (event: ServerFeaturesRefreshedPayload) => {
        if (event.space_id !== spaceId) return;

        void (async () => {
          const allFeatures = await listServerFeaturesByServer(event.space_id, event.server_id);
          const features = {
            tools: allFeatures.filter((f) => f.feature_type === 'tool'),
            prompts: allFeatures.filter((f) => f.feature_type === 'prompt'),
            resources: allFeatures.filter((f) => f.feature_type === 'resource'),
          };
          onFeaturesChangeRef.current?.({
            type: 'features_updated',
            space_id: event.space_id,
            server_id: event.server_id,
            features,
            added: event.added,
            removed: event.removed,
          });
        })();
      }),
    ];

    void refresh();

    return () => {
      unsubs.forEach((unsub) => unsub());
    };
  }, [spaceId, subscribe, refresh]);

  const enable = useCallback((serverId: string) => enableServer(spaceId, serverId), [spaceId]);
  const disable = useCallback((serverId: string) => disableServer(spaceId, serverId), [spaceId]);
  const connect = useCallback((serverId: string) => startAuth(spaceId, serverId), [spaceId]);
  const cancel = useCallback((serverId: string) => cancelAuth(spaceId, serverId), [spaceId]);
  const retry = useCallback((serverId: string) => retryConnection(spaceId, serverId), [spaceId]);

  const getButtonLabel = useCallback(
    (serverId: string) => {
      const status = statuses[serverId];
      if (!status) return 'Enable';
      return getConnectButtonLabel(status.status, status.has_connected_before);
    },
    [statuses]
  );

  const getAction = useCallback(
    (serverId: string) => {
      const status = statuses[serverId];
      if (!status) return 'enable' as const;
      return getServerAction(status.status);
    },
    [statuses]
  );

  return {
    statuses,
    loading,
    error,
    authProgress,
    enable,
    disable,
    connect,
    cancel,
    retry,
    getButtonLabel,
    getAction,
    refresh,
  };
}
