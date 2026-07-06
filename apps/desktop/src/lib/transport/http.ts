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
      // A single shared EventSource per subscribe keeps this simple; the
      // server fans out event names. Reconnect is handled by EventSource.
      const url = `${baseUrl}/admin/api/events`;
      const es = new EventSource(url, { withCredentials: true });
      const listener = (e: MessageEvent) => {
        try {
          handler(e.data ? JSON.parse(e.data) : undefined);
        } catch {
          handler(e.data);
        }
      };
      es.addEventListener(event, listener as EventListener);
      return () => {
        es.removeEventListener(event, listener as EventListener);
        es.close();
      };
    },

    capabilities: WEB_CAPABILITIES,
  };
}
