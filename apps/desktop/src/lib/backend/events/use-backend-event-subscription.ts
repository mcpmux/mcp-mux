import { useEffect } from 'react';

import { isTauri } from '../data/transport';

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

    const source = new EventSource('/api/v1/events');
    const onMessage = (event: MessageEvent<string>) => {
      try {
        callback(JSON.parse(event.data) as T);
      } catch {
        // ignore malformed frames
      }
    };
    source.addEventListener(channel, onMessage);

    return () => {
      source.removeEventListener(channel, onMessage);
      source.close();
    };
  }, [channel, callback, sse]);
}
