/**
 * useServerManager - React hook for event-driven server management
 *
 * Provides:
 * - Automatic subscription to server events
 * - Local state management for server statuses
 * - Helper functions for common operations
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  ServerStatusResponse,
  ServerStatusEvent,
  AuthProgressEvent,
  FeaturesUpdatedEvent,
  getServerStatuses,
  enableServer,
  disableServer,
  startAuth,
  cancelAuth,
  retryConnection,
  onServerStatus,
  onAuthProgress,
  onFeaturesUpdated,
  getConnectButtonLabel,
  getServerAction,
} from "../lib/api/serverManager";

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
    | "enable"
    | "disable"
    | "connect"
    | "cancel"
    | "retry"
    | "connected"
    | "connecting";
  /** Refresh statuses from backend */
  refresh: () => Promise<void>;
}

export function useServerManager({
  spaceId,
  onFeaturesChange,
}: UseServerManagerOptions): UseServerManagerResult {
  const [statuses, setStatuses] = useState<
    Record<string, ServerStatusResponse>
  >({});
  const [authProgress, setAuthProgress] = useState<Record<string, number>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const prevSpaceId = useRef<string | null>(null);

  // Stable ref for onFeaturesChange to avoid re-subscribing on every render
  const onFeaturesChangeRef = useRef(onFeaturesChange);
  onFeaturesChangeRef.current = onFeaturesChange;

  // Fetch initial statuses
  const refresh = useCallback(async () => {
    if (!spaceId) return;

    try {
      setLoading(true);
      setError(null);
      const result = await getServerStatuses(spaceId);
      setStatuses(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [spaceId]);

  useEffect(() => {
    if (!spaceId || prevSpaceId.current !== spaceId) {
      setStatuses({});
      setAuthProgress({});
      setError(null);
      setLoading(true); // Set loading true while fetching
      prevSpaceId.current = spaceId || null;
      
      // Immediately hydrate from backend on space switch
      if (spaceId) {
        refresh();
      }
    }
  }, [spaceId, refresh]);

  // Initial fetch (only on mount, not on space change since handled above)
  useEffect(() => {
    if (!prevSpaceId.current) {
      refresh();
    }
  }, [refresh]);

  // Subscribe to events
  useEffect(() => {
    if (!spaceId) return;

    const unsubscribers: Array<() => void> = [];

    // Event listeners are async (Tauri listen() returns a Promise).
    // Events emitted between the initial getServerStatuses fetch and listener
    // activation are lost. Track when all listeners are ready, then re-fetch
    // statuses to catch any events missed during the gap.
    const listenerPromises: Array<Promise<() => void>> = [];

    // Status changes
    const statusPromise = onServerStatus((event: ServerStatusEvent) => {
      if (event.space_id !== spaceId) return;

      setStatuses((prev) => {
        const existing = prev[event.server_id];
        // Ignore events from older flows (race condition prevention)
        if (existing && existing.flow_id > event.flow_id) {
          return prev;
        }

        return {
          ...prev,
          [event.server_id]: {
            server_id: event.server_id,
            status: event.status,
            flow_id: event.flow_id,
            has_connected_before: event.has_connected_before,
            message: event.message || null,
          },
        };
      });

      // Clear auth progress when leaving Authenticating state
      if (event.status !== "authenticating") {
        setAuthProgress((prev) => {
          const next = { ...prev };
          delete next[event.server_id];
          return next;
        });
      }
    });
    statusPromise.then((unlisten) => unsubscribers.push(unlisten));
    listenerPromises.push(statusPromise);

    // Auth progress
    const authPromise = onAuthProgress((event: AuthProgressEvent) => {
      if (event.space_id !== spaceId) return;

      setAuthProgress((prev) => ({
        ...prev,
        [event.server_id]: event.remaining_seconds,
      }));
    });
    authPromise.then((unlisten) => unsubscribers.push(unlisten));
    listenerPromises.push(authPromise);

    // Features updated (always subscribe, use ref to call latest callback)
    const featuresPromise = onFeaturesUpdated(
      (event: FeaturesUpdatedEvent) => {
        if (event.space_id !== spaceId) return;
        onFeaturesChangeRef.current?.(event);
      }
    );
    featuresPromise.then((unlisten) => unsubscribers.push(unlisten));
    listenerPromises.push(featuresPromise);

    // Once all listeners are active, re-fetch statuses to close the gap
    // between the initial fetch and listener activation (startup race fix)
    Promise.all(listenerPromises).then(() => {
      refresh();
    });

    return () => {
      unsubscribers.forEach((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [spaceId]);

  // Actions
  const enable = useCallback(
    (serverId: string) => enableServer(spaceId, serverId),
    [spaceId]
  );

  const disable = useCallback(
    (serverId: string) => disableServer(spaceId, serverId),
    [spaceId]
  );

  const connect = useCallback(
    (serverId: string) => startAuth(spaceId, serverId),
    [spaceId]
  );

  const cancel = useCallback(
    (serverId: string) => cancelAuth(spaceId, serverId),
    [spaceId]
  );

  const retry = useCallback(
    (serverId: string) => retryConnection(spaceId, serverId),
    [spaceId]
  );

  // Helpers
  const getButtonLabel = useCallback(
    (serverId: string) => {
      const status = statuses[serverId];
      if (!status) return "Enable";
      return getConnectButtonLabel(status.status, status.has_connected_before);
    },
    [statuses]
  );

  const getAction = useCallback(
    (serverId: string) => {
      const status = statuses[serverId];
      if (!status) return "enable";
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
