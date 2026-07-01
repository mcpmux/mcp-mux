export type { ApiRoute } from './fetch-api.types';
export { routeFor, registeredCommands } from './fetch-api.routes';

import { routeFor } from './fetch-api.routes';

let cachedCsrfToken: string | null = null;
let csrfFetchInFlight: Promise<string> | null = null;
let adminReadyPromise: Promise<void> | null = null;

const RETRYABLE_STATUS_CODES = new Set([500, 502, 503, 504]);
const MAX_GET_RETRIES = 4;
const MAX_MUTATION_RETRIES = 4;
const GET_RETRY_DELAYS_MS = [400, 800, 1600, 3200];
const CSRF_RETRY_DELAY_MS = 200;

/**
 * Pause briefly before retrying a transient admin API failure.
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/**
 * Returns true when the admin server rejected a stale CSRF token.
 */
function isCsrfRejection(status: number, message: string): boolean {
  return status === 403 && message.toLowerCase().includes('csrf');
}

/**
 * Clear the cached CSRF token so the next mutating request fetches a fresh one.
 */
function invalidateCsrfToken(): void {
  cachedCsrfToken = null;
  csrfFetchInFlight = null;
}

/**
 * Returns true when a GET should be retried (admin restarting, proxy blip, etc.).
 */
function isRetryableGetFailure(status: number | null): boolean {
  return status === null || RETRYABLE_STATUS_CODES.has(status);
}

/**
 * Poll admin `/health` until the backend accepts requests (web startup / hot-reload).
 * Single-flight per page load so React Strict Mode does not double-invalidate CSRF.
 */
export async function waitForAdminReady(timeoutMs = 15000): Promise<void> {
  if (!adminReadyPromise) {
    adminReadyPromise = waitForAdminReadyOnce(timeoutMs);
  }
  return adminReadyPromise;
}

/**
 * One-shot admin readiness probe plus CSRF prefetch for the current admin process.
 */
async function waitForAdminReadyOnce(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let attempt = 0;

  while (Date.now() < deadline) {
    try {
      const response = await fetch('/api/v1/health', {
        method: 'GET',
        headers: { Accept: 'application/json' },
        credentials: 'same-origin',
      });
      if (response.ok) {
        invalidateCsrfToken();
        if (attempt > 0) {
          console.info(`[fetchApi] Admin API ready after ${attempt + 1} health probe(s)`);
        }
        // CSRF is fetched lazily on the first mutating request — do not block GET startup sync.
        return;
      }
    } catch {
      // Vite proxy or admin not listening yet
    }

    const delayMs = GET_RETRY_DELAYS_MS[Math.min(attempt, GET_RETRY_DELAYS_MS.length - 1)] ?? 3200;
    attempt += 1;
    await sleep(delayMs);
  }

  console.warn(`[fetchApi] Admin health probe timed out after ${timeoutMs}ms — proceeding anyway`);
  invalidateCsrfToken();
}

/**
 * Fetch and cache the CSRF token for mutating admin requests.
 */
async function ensureCsrfToken(): Promise<string> {
  if (cachedCsrfToken) {
    return cachedCsrfToken;
  }
  if (csrfFetchInFlight) {
    return csrfFetchInFlight;
  }

  csrfFetchInFlight = (async () => {
    const response = await fetch('/api/v1/csrf-token', {
      method: 'GET',
      headers: { Accept: 'application/json' },
      credentials: 'same-origin',
    });
    if (!response.ok) {
      throw new Error('Failed to fetch CSRF token');
    }
    const body = (await response.json()) as { token?: string };
    if (!body.token) {
      throw new Error('CSRF token missing from response');
    }
    cachedCsrfToken = body.token;
    return cachedCsrfToken;
  })();

  try {
    return await csrfFetchInFlight;
  } finally {
    csrfFetchInFlight = null;
  }
}

/**
 * Execute an admin REST request for the given command mapping.
 */
export async function fetchApi<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  const { method, path, body } = routeFor(command, args ?? {});
  const maxAttempts = method === 'GET' ? MAX_GET_RETRIES : MAX_MUTATION_RETRIES;

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const headers: Record<string, string> = { Accept: 'application/json' };
    const init: RequestInit = {
      method,
      headers,
      credentials: 'same-origin',
    };

    if (method !== 'GET') {
      headers['Content-Type'] = 'application/json';
      headers['X-CSRF-Token'] = await ensureCsrfToken();
      if (body !== undefined) {
        init.body = JSON.stringify(body);
      } else if (method === 'POST' || method === 'PUT' || method === 'DELETE') {
        init.body = '{}';
      }
    }

    let response: Response;
    try {
      response = await fetch(path, init);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (method === 'GET' && attempt < maxAttempts - 1 && isRetryableGetFailure(null)) {
        const delayMs = GET_RETRY_DELAYS_MS[attempt] ?? 3200;
        console.warn(
          `[fetchApi] ${method} ${path} network error (attempt ${attempt + 1}/${maxAttempts}): ${message} — retrying in ${delayMs}ms`
        );
        await sleep(delayMs);
        continue;
      }
      throw new Error(`${message} (${method} ${path})`);
    }

    if (!response.ok) {
      const responseBody = await response.text();
      let message = responseBody || response.statusText;
      try {
        const parsed = JSON.parse(responseBody) as { error?: string };
        if (parsed.error) {
          message = parsed.error;
        }
      } catch {
        // keep raw body
      }

      if (
        method === 'GET' &&
        attempt < maxAttempts - 1 &&
        isRetryableGetFailure(response.status)
      ) {
        const delayMs = GET_RETRY_DELAYS_MS[attempt] ?? 3200;
        console.warn(
          `[fetchApi] ${method} ${path} failed with ${response.status} (attempt ${attempt + 1}/${maxAttempts}): ${message} — retrying in ${delayMs}ms`
        );
        await sleep(delayMs);
        continue;
      }

      if (method !== 'GET' && attempt < maxAttempts - 1 && isCsrfRejection(response.status, message)) {
        console.warn(
          `[fetchApi] ${method} ${path} CSRF rejected (attempt ${attempt + 1}/${maxAttempts}) — refreshing token`
        );
        invalidateCsrfToken();
        await sleep(CSRF_RETRY_DELAY_MS);
        continue;
      }

      console.error(
        `[fetchApi] ${method} ${path} failed with ${response.status}: ${message}`
      );
      throw new Error(`${message} (${method} ${path})`);
    }

    return response.json() as Promise<T>;
  }

  throw new Error(`Request failed after retries (${method} ${path})`);
}
