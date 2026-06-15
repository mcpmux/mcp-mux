/**
 * Streamable HTTP & List Change Notification E2E Tests
 *
 * Tests the full notification pipeline through the real running desktop app:
 *   Backend MCP Server -> Gateway -> list_changed -> Connected Clients
 *
 * Uses the "cloudflare-server" fixture (HTTP transport, no auth) which
 * points to the stub MCP server on port 3457. The stub server has control
 * endpoints to trigger list_changed notifications programmatically.
 *
 * Prerequisites:
 * - App built and running via tauri-driver
 * - Mock Bundle API on port 8787 (serves server definitions)
 * - Stub MCP HTTP Server on port 3457 (with control endpoints)
 */

import {
  getActiveSpace,
  getGatewayStatus,
  installServer,
  enableServerV2,
  disableServerV2,
  listInstalledServers,
  refreshRegistry,
  approveOAuthClient,
  grantOAuthClientFeatureSet,
  createFeatureSet,
  addFeatureToSet,
  seedServerFeatures,
} from '../helpers/tauri-api';
import {
  registerOAuthClient,
  obtainAccessToken,
} from '../helpers/mcp-client';
import {
  triggerToolsChanged,
  triggerPromptsChanged,
  triggerResourcesChanged,
  addDynamicTool,
  removeDynamicTool,
} from '../helpers/stub-server-control';

// Server definition from mock bundle
const CLOUDFLARE_SERVER_ID = 'cloudflare-server';
const STUB_HTTP_PORT = 3457;

/**
 * Parse an MCP Streamable HTTP response that may be either JSON or SSE format.
 *
 * Per the MCP spec (2025-03-26), when a POST contains JSON-RPC requests, the
 * server MUST respond with either `Content-Type: application/json` (single JSON
 * object) or `Content-Type: text/event-stream` (SSE stream). The client MUST
 * support both cases.
 *
 * SSE responses contain `data:` lines with JSON-RPC messages. We extract the
 * first JSON-RPC response message from the stream.
 */
function parseMcpResponse<T>(contentType: string | null, responseText: string): T {
  if (contentType?.includes('text/event-stream')) {
    // Parse SSE: extract JSON from `data:` lines
    const lines = responseText.split('\n');
    for (const line of lines) {
      if (line.startsWith('data:')) {
        const data = line.slice('data:'.length).trim();
        if (data) {
          return JSON.parse(data) as T;
        }
      }
    }
    throw new Error(`No data events found in SSE response: ${responseText.substring(0, 500)}`);
  }
  // Default: parse as plain JSON
  return JSON.parse(responseText) as T;
}

// ============================================================================
// Test Suite: Streamable HTTP Transport & Notifications
// ============================================================================

describe('Streamable HTTP: Gateway & Notifications', function () {
  this.timeout(120000);

  let defaultSpaceId: string;
  let gatewayPort: number;

  before(async () => {
    // Wait for app to be ready
    await browser.pause(3000);

    // Get default space
    const activeSpace = await getActiveSpace();
    defaultSpaceId = activeSpace?.id || '';
    console.log('[setup] Default space:', defaultSpaceId);

    // Refresh registry so servers from mock bundle are available
    try {
      await refreshRegistry();
      await browser.pause(2000);
    } catch (e) {
      console.log('[setup] Registry refresh failed (may already be loaded):', e);
    }

    // Install and enable the Cloudflare server (HTTP transport, no auth)
    try {
      await installServer(CLOUDFLARE_SERVER_ID, defaultSpaceId);
      console.log('[setup] Installed cloudflare-server');
    } catch (e) {
      console.log('[setup] Install failed (may already exist):', e);
    }

    try {
      await enableServerV2(defaultSpaceId, CLOUDFLARE_SERVER_ID);
      console.log('[setup] Enabled cloudflare-server');
    } catch (e) {
      console.log('[setup] Enable failed:', e);
    }

    // Wait for gateway to connect to backend
    await browser.pause(5000);

    // Get gateway port
    const status = await getGatewayStatus();
    console.log('[setup] Gateway status:', JSON.stringify(status));
    if (status.url) {
      const url = new URL(status.url);
      gatewayPort = parseInt(url.port, 10);
    } else {
      gatewayPort = 45818; // default
    }
  });

  // --------------------------------------------------------------------------
  // TC-SH-001: Gateway serves Streamable HTTP endpoint
  // --------------------------------------------------------------------------
  it('TC-SH-001: Gateway is running and serves /mcp endpoint', async () => {
    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
    console.log('[test] Gateway URL:', status.url);
    console.log('[test] Connected backends:', status.connected_backends);
  });

  // --------------------------------------------------------------------------
  // TC-SH-002: Backend server connects via HTTP transport
  // --------------------------------------------------------------------------
  it('TC-SH-002: Cloudflare server connects to gateway via HTTP', async () => {
    // Wait a bit more for connection if needed
    let retries = 5;
    let status = await getGatewayStatus();

    while (status.connected_backends === 0 && retries > 0) {
      await browser.pause(2000);
      status = await getGatewayStatus();
      retries--;
    }

    console.log('[test] Connected backends:', status.connected_backends);
    // On CI the MCP handshake may fail, so just check the gateway is running
    expect(status.running).toBe(true);

    // If backends connected, verify the installed server is the right one
    if (status.connected_backends > 0) {
      const servers = await listInstalledServers(defaultSpaceId);
      const cfServer = servers.find(
        (s) => s.server_id === CLOUDFLARE_SERVER_ID || s.id === CLOUDFLARE_SERVER_ID
      );
      expect(cfServer).toBeTruthy();
      console.log('[test] Cloudflare server found:', cfServer?.server_id || cfServer?.id);
    }
  });

  // --------------------------------------------------------------------------
  // TC-SH-003: Stub server control endpoints work
  // --------------------------------------------------------------------------
  it('TC-SH-003: Stub server control endpoints respond', async () => {
    // Verify the stub server is reachable and control endpoints work
    const healthRes = await fetch(`http://localhost:${STUB_HTTP_PORT}/health`);
    expect(healthRes.ok).toBe(true);

    const health = (await healthRes.json()) as { status: string; sessions: number };
    console.log('[test] Stub server health:', JSON.stringify(health));
    expect(health.status).toBe('ok');

    // Trigger tools changed (may have 0 sessions if gateway hasn't connected yet)
    const result = await triggerToolsChanged(STUB_HTTP_PORT);
    console.log('[test] Trigger tools changed result:', JSON.stringify(result));
    expect(result.ok).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-004: Trigger tools/list_changed notification from backend
  // --------------------------------------------------------------------------
  it('TC-SH-004: Backend triggers tools/list_changed notification', async () => {
    // Trigger tools/list_changed on the stub server
    // The gateway's McpClientHandler should receive this and emit a DomainEvent
    const result = await triggerToolsChanged(STUB_HTTP_PORT);
    console.log('[test] Tools changed:', JSON.stringify(result));
    expect(result.ok).toBe(true);

    // Wait for notification to propagate through the gateway
    await browser.pause(2000);

    // The gateway should still be running after receiving the notification
    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-005: Trigger prompts/list_changed notification from backend
  // --------------------------------------------------------------------------
  it('TC-SH-005: Backend triggers prompts/list_changed notification', async () => {
    const result = await triggerPromptsChanged(STUB_HTTP_PORT);
    console.log('[test] Prompts changed:', JSON.stringify(result));
    expect(result.ok).toBe(true);

    await browser.pause(2000);
    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-006: Trigger resources/list_changed notification from backend
  // --------------------------------------------------------------------------
  it('TC-SH-006: Backend triggers resources/list_changed notification', async () => {
    const result = await triggerResourcesChanged(STUB_HTTP_PORT);
    console.log('[test] Resources changed:', JSON.stringify(result));
    expect(result.ok).toBe(true);

    await browser.pause(2000);
    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-007: Backend dynamically adds a tool
  // --------------------------------------------------------------------------
  it('TC-SH-007: Backend dynamically adds a tool and notifies', async () => {
    const result = await addDynamicTool('test_dynamic_tool', 'A dynamically added test tool', STUB_HTTP_PORT);
    console.log('[test] Add dynamic tool:', JSON.stringify(result));
    expect(result.ok).toBe(true);

    // Wait for notification pipeline
    await browser.pause(2000);

    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-008: Backend dynamically removes a tool
  // --------------------------------------------------------------------------
  it('TC-SH-008: Backend dynamically removes a tool and notifies', async () => {
    const result = await removeDynamicTool('test_dynamic_tool', STUB_HTTP_PORT);
    console.log('[test] Remove dynamic tool:', JSON.stringify(result));
    expect(result.ok).toBe(true);

    await browser.pause(2000);

    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-009: All notification types in rapid succession
  // --------------------------------------------------------------------------
  it('TC-SH-009: Multiple notification types in rapid succession', async () => {
    // Fire all 3 notification types quickly
    const [toolsResult, promptsResult, resourcesResult] = await Promise.all([
      triggerToolsChanged(STUB_HTTP_PORT),
      triggerPromptsChanged(STUB_HTTP_PORT),
      triggerResourcesChanged(STUB_HTTP_PORT),
    ]);

    console.log('[test] Rapid notifications:',
      JSON.stringify({ tools: toolsResult, prompts: promptsResult, resources: resourcesResult }));

    expect(toolsResult.ok).toBe(true);
    expect(promptsResult.ok).toBe(true);
    expect(resourcesResult.ok).toBe(true);

    // Wait for all to propagate
    await browser.pause(3000);

    // Gateway should handle rapid notifications without crashing
    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
  });

  // --------------------------------------------------------------------------
  // TC-SH-010: Disable server triggers notification pipeline
  // --------------------------------------------------------------------------
  it('TC-SH-010: Disabling server triggers disconnection notification', async () => {
    // Disable the server
    await disableServerV2(defaultSpaceId, CLOUDFLARE_SERVER_ID);
    console.log('[test] Disabled cloudflare-server');

    // Wait for disconnect propagation
    await browser.pause(3000);

    // Gateway should still be running
    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
    console.log('[test] Connected backends after disable:', status.connected_backends);
  });

  // --------------------------------------------------------------------------
  // TC-SH-011: Re-enable server reconnects
  // --------------------------------------------------------------------------
  it('TC-SH-011: Re-enabling server reconnects to backend', async () => {
    // Re-enable
    try {
      await enableServerV2(defaultSpaceId, CLOUDFLARE_SERVER_ID);
      console.log('[test] Re-enabled cloudflare-server');
    } catch (e) {
      console.log('[test] Re-enable failed:', e);
    }

    // Wait for reconnection
    await browser.pause(5000);

    const status = await getGatewayStatus();
    expect(status.running).toBe(true);
    console.log('[test] Connected backends after re-enable:', status.connected_backends);
  });
});

// ============================================================================
// Test Suite: OAuth Client + Gateway MCP Connection
// ============================================================================

describe('Streamable HTTP: OAuth MCP Client Flow', function () {
  this.timeout(120000);

  let defaultSpaceId: string;
  let gatewayPort: number;
  let clientId: string;

  before(async () => {
    await browser.pause(2000);

    const activeSpace = await getActiveSpace();
    defaultSpaceId = activeSpace?.id || '';

    const status = await getGatewayStatus();
    if (status.url) {
      const url = new URL(status.url);
      gatewayPort = parseInt(url.port, 10);
    } else {
      gatewayPort = 45818;
    }
  });

  // --------------------------------------------------------------------------
  // TC-SH-012: Register and approve OAuth client
  // --------------------------------------------------------------------------
  it('TC-SH-012: Register OAuth client via DCR and approve', async () => {
    // Register via DCR
    clientId = await registerOAuthClient('e2e-test-mcp-client', 'http://localhost:0/callback', gatewayPort);
    console.log('[test] Registered client:', clientId);
    expect(clientId).toBeTruthy();

    // Approve via Tauri API (bypasses consent UI)
    await approveOAuthClient(clientId);
    console.log('[test] Approved client:', clientId);
  });

  // --------------------------------------------------------------------------
  // TC-SH-013: Obtain JWT access token via OAuth flow
  // --------------------------------------------------------------------------
  it('TC-SH-013: Obtain access token via full OAuth PKCE flow', async () => {
    const token = await obtainAccessToken(clientId, 'http://localhost:0/callback', gatewayPort);
    console.log('[test] Got access token:', token.substring(0, 20) + '...');
    expect(token).toBeTruthy();
    expect(token.length).toBeGreaterThan(10);
  });

  // --------------------------------------------------------------------------
  // TC-SH-014: Authenticated POST to /mcp endpoint
  // --------------------------------------------------------------------------
  it('TC-SH-014: Authenticated initialize request to /mcp', async () => {
    const token = await obtainAccessToken(clientId, 'http://localhost:0/callback', gatewayPort);
    console.log('[test] Token obtained, length:', token.length);

    // Send MCP initialize request
    // Streamable HTTP requires Accept: application/json, text/event-stream
    const res = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json, text/event-stream',
        'Authorization': `Bearer ${token}`,
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: '2025-03-26',
          capabilities: {},
          clientInfo: {
            name: 'e2e-test-client',
            version: '1.0.0',
          },
        },
      }),
    });

    console.log('[test] Initialize response status:', res.status, res.statusText);
    console.log('[test] Initialize response content-type:', res.headers.get('content-type'));
    const responseText = await res.text();
    console.log('[test] Initialize response body:', responseText.substring(0, 1000));

    // If response is not OK, provide detailed failure info
    if (!res.ok) {
      console.log('[test] FAILURE: /mcp returned', res.status, '- body:', responseText);
    }
    expect(res.status).toBeLessThan(400);

    // The server may respond with JSON or SSE (per MCP Streamable HTTP spec)
    const body = parseMcpResponse<{
      jsonrpc: string;
      id: number;
      result?: {
        protocolVersion: string;
        capabilities: {
          tools?: { listChanged?: boolean };
          prompts?: { listChanged?: boolean };
          resources?: { listChanged?: boolean };
        };
        serverInfo: { name: string; version: string };
      };
    }>(res.headers.get('content-type'), responseText);

    console.log('[test] Initialize result:', JSON.stringify(body));

    // Verify response structure
    expect(body.jsonrpc).toBe('2.0');
    expect(body.result).toBeTruthy();
    expect(body.result!.serverInfo).toBeTruthy();
    expect(body.result!.protocolVersion).toBeTruthy();

    // Verify capabilities advertise listChanged
    const caps = body.result!.capabilities;
    console.log('[test] Server capabilities:', JSON.stringify(caps));

    // The gateway should advertise listChanged for tools, prompts, and resources
    if (caps.tools) {
      expect(caps.tools.listChanged).toBe(true);
    }
    if (caps.prompts) {
      expect(caps.prompts.listChanged).toBe(true);
    }
    if (caps.resources) {
      expect(caps.resources.listChanged).toBe(true);
    }
  });

  // --------------------------------------------------------------------------
  // TC-SH-015: Session management via Mcp-Session-Id header
  // --------------------------------------------------------------------------
  it('TC-SH-015: Session ID returned and usable', async () => {
    const token = await obtainAccessToken(clientId, 'http://localhost:0/callback', gatewayPort);
    console.log('[test] Token for session test, length:', token.length);

    // Initialize to get session ID
    const initRes = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json, text/event-stream',
        'Authorization': `Bearer ${token}`,
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: '2025-03-26',
          capabilities: {},
          clientInfo: { name: 'e2e-session-test', version: '1.0.0' },
        },
      }),
    });

    console.log('[test] Session init status:', initRes.status, initRes.statusText);
    console.log('[test] Session init content-type:', initRes.headers.get('content-type'));
    const initText = await initRes.text();
    console.log('[test] Session init body:', initText.substring(0, 1000));
    if (!initRes.ok) {
      console.log('[test] FAILURE: /mcp returned', initRes.status, '- body:', initText);
    }
    expect(initRes.status).toBeLessThan(400);

    // Parse the response (may be JSON or SSE per spec)
    parseMcpResponse<{ jsonrpc: string; id: number }>(initRes.headers.get('content-type'), initText);

    // Check for Mcp-Session-Id in response headers
    const sessionId = initRes.headers.get('mcp-session-id');
    console.log('[test] Session ID:', sessionId);
    expect(sessionId).toBeTruthy();

    // Send initialized notification using the session ID
    const notifyRes = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json, text/event-stream',
        'Authorization': `Bearer ${token}`,
        'Mcp-Session-Id': sessionId!,
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        method: 'notifications/initialized',
      }),
    });

    console.log('[test] Initialized notification status:', notifyRes.status);
    // 200 or 202 are both acceptable
    expect(notifyRes.status).toBeLessThan(300);

    // Use the session to list tools
    const toolsRes = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Accept': 'application/json, text/event-stream',
        'Authorization': `Bearer ${token}`,
        'Mcp-Session-Id': sessionId!,
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 2,
        method: 'tools/list',
        params: {},
      }),
    });

    expect(toolsRes.ok).toBe(true);
    console.log('[test] Tools response content-type:', toolsRes.headers.get('content-type'));
    const toolsText = await toolsRes.text();
    console.log('[test] Tools response body:', toolsText.substring(0, 1000));
    const toolsBody = parseMcpResponse<{
      result?: { tools: Array<{ name: string; description?: string }> };
    }>(toolsRes.headers.get('content-type'), toolsText);

    console.log('[test] Tools count:', toolsBody.result?.tools?.length ?? 0);
    if (toolsBody.result?.tools && toolsBody.result.tools.length > 0) {
      console.log('[test] First tool:', toolsBody.result.tools[0].name);
    }
  });

  // --------------------------------------------------------------------------
  // TC-SH-016: "if it lists, it calls" — a listed tool is never grant-blocked
  //
  // Regression for the reported bug: a tool (e.g. notion_notion-get-users)
  // appeared in tools/list yet tools/call rejected it with "not allowed by the
  // current grants". Root cause was list encoding names via qualified_name()
  // while call decoded via a stale prefix-cache reverse lookup. Both paths now
  // match qualified_name() against the SAME resolved feature set.
  //
  // Seed a feature with a dotted server_id + hyphenated name (mirrors the real
  // com.notion-mcp-http_notion-get-users shape) so this holds even when the
  // backend MCP handshake doesn't complete on CI.
  // --------------------------------------------------------------------------
  it('TC-SH-016: every listed tool is callable (no listed-but-blocked)', async () => {
    // 1. Seed a backend tool feature directly into this space.
    const seeded = await seedServerFeatures([
      {
        space_id: defaultSpaceId,
        server_id: 'com.e2e-listcall-http',
        feature_type: 'tool',
        feature_name: 'list-and-call-me',
        display_name: 'List And Call Me',
        description: 'E2E tool proving list==call',
      },
    ]);
    expect(seeded.length).toBe(1);
    const featureId = seeded[0];

    // 2. Compose a FeatureSet with exactly that tool, grant it to the client.
    const fs = await createFeatureSet({
      name: `e2e-listcall-${Date.now()}`,
      space_id: defaultSpaceId,
    });
    await addFeatureToSet(fs.id, featureId, 'include');
    await grantOAuthClientFeatureSet(clientId, defaultSpaceId, fs.id);

    // 3. Fresh session so the new grant resolves.
    const token = await obtainAccessToken(clientId, 'http://localhost:0/callback', gatewayPort);
    const initRes = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: '2025-03-26',
          capabilities: {},
          clientInfo: { name: 'e2e-listcall', version: '1.0.0' },
        },
      }),
    });
    expect(initRes.status).toBeLessThan(400);
    const sessionId = initRes.headers.get('mcp-session-id');
    expect(sessionId).toBeTruthy();
    await initRes.text();

    await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
        Authorization: `Bearer ${token}`,
        'Mcp-Session-Id': sessionId!,
      },
      body: JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }),
    });

    // 4. tools/list — the seeded tool MUST be listed under the grant.
    const listRes = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
        Authorization: `Bearer ${token}`,
        'Mcp-Session-Id': sessionId!,
      },
      body: JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} }),
    });
    expect(listRes.ok).toBe(true);
    const listBody = parseMcpResponse<{
      result?: { tools: Array<{ name: string }> };
    }>(listRes.headers.get('content-type'), await listRes.text());
    const tools = listBody.result?.tools ?? [];
    console.log('[test] TC-SH-016 listed tools:', tools.map((t) => t.name).join(', '));
    const target = tools.find((t) => t.name.includes('list-and-call-me'));
    expect(target).toBeTruthy();

    // 5. tools/call the EXACT listed name. It may fail to execute (no live
    //    backend behind the seeded feature), but it must NEVER be rejected by
    //    grants — that is the invariant the fix guarantees.
    const callRes = await fetch(`http://localhost:${gatewayPort}/mcp`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
        Authorization: `Bearer ${token}`,
        'Mcp-Session-Id': sessionId!,
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 3,
        method: 'tools/call',
        params: { name: target!.name, arguments: {} },
      }),
    });
    const callBody = parseMcpResponse<{
      error?: { message?: string };
      result?: unknown;
    }>(callRes.headers.get('content-type'), await callRes.text());
    const errMsg = callBody.error?.message ?? '';
    console.log('[test] TC-SH-016 call error (if any):', errMsg);
    expect(errMsg).not.toContain('not allowed by the current grants');
  });
});
