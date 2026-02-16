/**
 * MCP Client Helper for E2E Tests
 *
 * Provides functions to perform the full OAuth 2.1 + PKCE flow
 * against the McpMux gateway programmatically and obtain an access token.
 *
 * This enables tests to connect an MCP client to the gateway
 * without going through the browser-based consent flow.
 *
 * IMPORTANT: The `/oauth/consent/approve` HTTP endpoint is only available
 * when `MCPMUX_E2E_TEST=1` environment variable is set. In production,
 * consent approval is restricted to Tauri IPC (desktop app UI) only.
 */

import crypto from 'node:crypto';

const DEFAULT_GATEWAY_PORT = 45818;

function gatewayUrl(path: string, port?: number): string {
  return `http://localhost:${port ?? DEFAULT_GATEWAY_PORT}${path}`;
}

/** Generate PKCE code verifier + challenge pair (S256) */
function generatePkce(): { codeVerifier: string; codeChallenge: string } {
  const codeVerifier = crypto.randomBytes(32).toString('base64url');
  const hash = crypto.createHash('sha256').update(codeVerifier).digest();
  const codeChallenge = hash.toString('base64url');
  return { codeVerifier, codeChallenge };
}

/**
 * Register a new OAuth client via Dynamic Client Registration (DCR).
 * Returns the client_id assigned by the gateway.
 */
export async function registerOAuthClient(
  clientName: string,
  redirectUri: string = 'http://localhost:0/callback',
  port?: number
): Promise<string> {
  const res = await fetch(gatewayUrl('/oauth/register', port), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      client_name: clientName,
      redirect_uris: [redirectUri],
      grant_types: ['authorization_code'],
      response_types: ['code'],
      token_endpoint_auth_method: 'none',
    }),
  });

  if (!res.ok) {
    throw new Error(`DCR failed: ${res.status} ${await res.text()}`);
  }

  const data = (await res.json()) as { client_id: string };
  return data.client_id;
}

/**
 * Perform the full OAuth 2.1 + PKCE flow to obtain a JWT access token.
 *
 * Prerequisites:
 * - Client must be registered (via registerOAuthClient or DCR)
 * - Client must be approved (via Tauri API approveOAuthClient)
 *
 * Steps:
 * 1. GET /oauth/authorize with PKCE challenge → extracts request_id from deep link
 * 2. POST /oauth/consent/approve with request_id → extracts auth code from redirect
 * 3. POST /oauth/token with auth code + code verifier → returns JWT
 *
 * @returns JWT access token string
 */
export async function obtainAccessToken(
  clientId: string,
  redirectUri: string = 'http://localhost:0/callback',
  port?: number
): Promise<string> {
  const { codeVerifier, codeChallenge } = generatePkce();
  const state = crypto.randomUUID();

  // Step 1: Authorization request
  const authorizeUrl = new URL(gatewayUrl('/oauth/authorize', port));
  authorizeUrl.searchParams.set('client_id', clientId);
  authorizeUrl.searchParams.set('response_type', 'code');
  authorizeUrl.searchParams.set('redirect_uri', redirectUri);
  authorizeUrl.searchParams.set('code_challenge', codeChallenge);
  authorizeUrl.searchParams.set('code_challenge_method', 'S256');
  authorizeUrl.searchParams.set('state', state);

  // The authorize endpoint returns an HTML page with a deep link.
  // We need to extract the request_id from the HTML content.
  const authorizeRes = await fetch(authorizeUrl.toString(), { redirect: 'manual' });
  const html = await authorizeRes.text();

  // Extract request_id from the deep link in the HTML
  // Format: mcpmux://authorize?request_id=<id>
  const requestIdMatch = html.match(/request_id=([^"&\s]+)/);
  if (!requestIdMatch) {
    throw new Error(
      `Could not extract request_id from authorize response. HTML: ${html.substring(0, 500)}`
    );
  }
  const requestId = decodeURIComponent(requestIdMatch[1]);

  // Step 2: Consent approval
  const consentRes = await fetch(gatewayUrl('/oauth/consent/approve', port), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      request_id: requestId,
      approved: true,
    }),
  });

  if (!consentRes.ok) {
    throw new Error(`Consent approval failed: ${consentRes.status} ${await consentRes.text()}`);
  }

  const consentData = (await consentRes.json()) as {
    success: boolean;
    redirect_url: string;
    error?: string;
  };

  if (!consentData.success) {
    throw new Error(`Consent not successful: ${consentData.error}`);
  }

  // Extract auth code from redirect URL
  const redirectUrl = new URL(consentData.redirect_url);
  const code = redirectUrl.searchParams.get('code');
  if (!code) {
    throw new Error(`No auth code in redirect: ${consentData.redirect_url}`);
  }

  // Step 3: Token exchange
  const tokenRes = await fetch(gatewayUrl('/oauth/token', port), {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'authorization_code',
      code,
      client_id: clientId,
      redirect_uri: redirectUri,
      code_verifier: codeVerifier,
    }).toString(),
  });

  if (!tokenRes.ok) {
    throw new Error(`Token exchange failed: ${tokenRes.status} ${await tokenRes.text()}`);
  }

  const tokenData = (await tokenRes.json()) as {
    access_token: string;
    token_type: string;
    expires_in?: number;
  };

  return tokenData.access_token;
}

/**
 * Wait for the gateway to be ready by polling /health.
 */
export async function waitForGateway(port?: number, timeoutMs: number = 10000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(gatewayUrl('/health', port));
      if (res.ok) return;
    } catch {
      // Not ready yet
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`Gateway not ready after ${timeoutMs}ms`);
}
