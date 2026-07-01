import { useEffect } from 'react';

import { isTauri } from '../data/transport';

import {
  acquireAdminSseConsumer,
  releaseAdminSseConsumer,
  subscribeAdminSseRaw,
} from './admin-sse-hub';
import { listenWhenTauri } from './tauri-adapter';

/** Options for {@link useBackendEventSubscription}. */
export interface BackendEventSubscriptionOptions {
  /** When false, skip SSE on web (desktop-only channels). Default true. */
  sse?: boolean;
}

/**
 * React hook that subscribes to a backend event channel via Tauri IPC or admin SSE.
 */
export function useBackendEventSubscription<T>(
  channel: string,
  callback: (payload: T) => void,
  options: BackendEventSubscriptionOptions = {}
): void {
  const { sse = true } = options;

  useEffect(() => {
    if (isTauri()) {
      let disposed = false;
      let unlistenFn: (() => void) | undefined;

      void listenWhenTauri<T>(channel, (event) => {
        callback(event.payload);
      }).then((fn) => {
        if (disposed) {
          fn?.();
        } else {
          unlistenFn = fn;
        }
      });

      return () => {
        disposed = true;
        unlistenFn?.();
      };
    }

    if (!sse) {
      return;
    }

    acquireAdminSseConsumer();

    const unsubscribe = subscribeAdminSseRaw(channel, (payload) => {
      try {
        callback(payload as T);
      } catch {
        // ignore malformed frames
      }
    });

    return () => {
      unsubscribe();
      releaseAdminSseConsumer();
    };
  }, [channel, callback, sse]);
}
