import type { Transport } from './index';
import { WEB_CAPABILITIES } from './capabilities';

/**
 * Web transport: talks to the headless gateway's management API over HTTP —
 * a command-mirror JSON-RPC (`POST /admin/api/rpc/<cmd>`, same command names +
 * payloads as `invoke`) plus an SSE event stream. Used when the app runs in a
 * plain browser (the `mcpmux serve` web admin) rather than the Tauri shell.
 *
 * The base URL and session token come from the page it's served on; in the
 * embedded web-admin they default to same-origin. This transport is wired
 * behind the facade now and exercised end-to-end once the write endpoints +
 * SSE land (cloud-support M1-07/M1-09).
 */

interface HttpTransportOptions {
  /** API origin; defaults to same-origin (`''`). */
  baseUrl?: string;
  /** Bearer token for `/admin/api/*` (from login/session). */
  getToken?: () => string | null;
}

export function createHttpTransport(opts: HttpTransportOptions = {}): Transport {
  const baseUrl = (opts.baseUrl ?? '').replace(/\/$/, '');
  const authHeaders = (): Record<string, string> => {
    const t = opts.getToken?.();
    return t ? { Authorization: `Bearer ${t}` } : {};
  };

  // ONE shared EventSource per transport instance, multiplexing every
  // subscription. Browsers cap concurrent connections per origin (~6 on
  // HTTP/1.1); the web admin registers 11+ listeners at boot, so one SSE
  // stream per subscribe() would starve all other fetches and freeze the UI.
  let eventSource: EventSource | null = null;
  // event name -> handlers subscribed to it
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  // event name -> the single DOM listener that fans out to `handlers`
  const domListeners = new Map<string, (e: MessageEvent) => void>();

  const ensureEventSource = (): EventSource => {
    if (eventSource) return eventSource;
    // EventSource can't set headers, so the token rides as a query param.
    const t = opts.getToken?.();
    const url = `${baseUrl}/admin/api/events${t ? `?token=${encodeURIComponent(t)}` : ''}`;
    eventSource = new EventSource(url, { withCredentials: true });
    return eventSource;
  };

  return {
    async call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
      const res = await fetch(`${baseUrl}/admin/api/rpc/${encodeURIComponent(cmd)}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...authHeaders() },
        body: JSON.stringify(args ?? {}),
      });
      if (res.status === 404) {
        throw new Error(`Command "${cmd}" is not available over the web API yet.`);
      }
      if (!res.ok) {
        const text = await res.text().catch(() => '');
        throw new Error(text || `Request failed (${res.status})`);
      }
      // 204 / empty body → undefined result.
      const text = await res.text();
      return (text ? JSON.parse(text) : undefined) as T;
    },

    subscribe(event: string, handler: (payload: unknown) => void): () => void {
      // All subscriptions share one EventSource (see above); the server fans
      // out event names. Reconnect is handled by EventSource itself.
      const es = ensureEventSource();
      let set = handlers.get(event);
      if (!set) {
        set = new Set();
        handlers.set(event, set);
        // One DOM listener per unique event name; it parses the payload once
        // and dispatches to every handler registered for that name.
        const domListener = (e: MessageEvent) => {
          let payload: unknown;
          try {
            payload = e.data ? JSON.parse(e.data) : undefined;
          } catch {
            payload = e.data;
          }
          for (const h of [...(handlers.get(event) ?? [])]) h(payload);
        };
        domListeners.set(event, domListener);
        es.addEventListener(event, domListener as EventListener);
      }
      set.add(handler);

      let active = true;
      return () => {
        if (!active) return;
        active = false;
        const current = handlers.get(event);
        current?.delete(handler);
        if (current && current.size === 0) {
          handlers.delete(event);
          const domListener = domListeners.get(event);
          if (domListener && eventSource) {
            eventSource.removeEventListener(event, domListener as EventListener);
          }
          domListeners.delete(event);
        }
        // Last handler across all names gone → close the shared stream.
        if (handlers.size === 0 && eventSource) {
          eventSource.close();
          eventSource = null;
        }
      };
    },

    capabilities: WEB_CAPABILITIES,
  };
}
