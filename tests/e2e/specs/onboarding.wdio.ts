/**
 * E2E: "install, connect, use" onboarding.
 *
 * A new user installs a server and connects an app with no FeatureSet setup
 * and no access key, and the app gets every tool. Past the size warning (80
 * tools) the app keeps serving everything but points the user at `@mux` or a
 * FeatureSet. Web pages in a browser can't reach the gateway.
 *
 * Specs share one data directory and run in file order, so this spec sets the
 * state it needs (the default Space's Starter back in auto mode) instead of
 * assuming earlier specs left it untouched. The defaults themselves are
 * checked where earlier specs can't have changed them: the inbound-auth
 * setting (no spec toggles it) and a brand-new Space's Starter.
 */

import {
  createSpace,
  deleteSpace,
  enableServerV2,
  getActiveSpace,
  getGatewayStatus,
  installServer,
  invoke,
  listFeatureSetsBySpace,
  refreshRegistry,
} from '../helpers/tauri-api';
import { addDynamicTool, removeDynamicTool } from '../helpers/stub-server-control';
import { byTestId, safeClick, TIMEOUT } from '../helpers/selectors';

const CLOUDFLARE_SERVER_ID = 'cloudflare-server';
const EXTRA_TOOL_COUNT = 85;
const extraToolName = (i: number) => `onboarding_extra_${i}`;

interface StarterToolSummary {
  feature_set_id: string;
  auto_include: boolean;
  tool_count: number;
  server_count: number;
  threshold: number;
  over_threshold: boolean;
}

/** Extract the first JSON-RPC message from a JSON or SSE response body. */
function parseMcp<T>(contentType: string | null, text: string): T {
  if (contentType?.includes('text/event-stream')) {
    for (const line of text.split('\n')) {
      if (line.startsWith('data:') && line.slice(5).trim()) {
        return JSON.parse(line.slice(5).trim()) as T;
      }
    }
    throw new Error(`No data event in SSE body: ${text.slice(0, 300)}`);
  }
  return JSON.parse(text) as T;
}

/** POST a JSON-RPC message to the gateway with no Authorization header. */
async function postMcp(
  port: number,
  body: unknown,
  extraHeaders: Record<string, string> = {}
): Promise<Response> {
  return fetch(`http://localhost:${port}/mcp`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'application/json, text/event-stream',
      ...extraHeaders,
    },
    body: JSON.stringify(body),
  });
}

const INITIALIZE = {
  jsonrpc: '2.0',
  id: 1,
  method: 'initialize',
  params: {
    protocolVersion: '2025-11-25',
    capabilities: {},
    clientInfo: { name: 'e2e-onboarding', version: '1.0.0' },
  },
};

/** Connect like a brand-new app (no token) and return its tool names. */
async function listToolsWithoutToken(port: number): Promise<string[]> {
  const init = await postMcp(port, INITIALIZE);
  expect(init.status).toBe(200);
  const sessionId = init.headers.get('mcp-session-id');
  expect(sessionId).toBeTruthy();
  await init.text();

  const session = { 'Mcp-Session-Id': sessionId! };
  await (
    await postMcp(port, { jsonrpc: '2.0', method: 'notifications/initialized' }, session)
  ).text();

  const res = await postMcp(port, { jsonrpc: '2.0', id: 2, method: 'tools/list' }, session);
  expect(res.status).toBe(200);
  const msg = parseMcp<{ result: { tools: { name: string }[] } }>(
    res.headers.get('content-type'),
    await res.text()
  );
  return msg.result.tools.map((t) => t.name);
}

async function starterSummary(spaceId: string): Promise<StarterToolSummary> {
  return invoke<StarterToolSummary>('get_starter_tool_summary', { spaceId });
}

describe('Onboarding: install, connect, use', function () {
  this.timeout(180000);

  let spaceId: string;
  let gatewayPort: number;

  before(async () => {
    await browser.pause(3000);
    const space = await getActiveSpace();
    spaceId = space?.id || '';

    try {
      await refreshRegistry();
      await browser.pause(2000);
    } catch (e) {
      console.log('[onboarding] registry refresh failed (may already be loaded):', e);
    }
    try {
      await installServer(CLOUDFLARE_SERVER_ID, spaceId);
    } catch (e) {
      console.log('[onboarding] install failed (may already exist):', e);
    }
    try {
      await enableServerV2(spaceId, CLOUDFLARE_SERVER_ID);
    } catch (e) {
      console.log('[onboarding] enable failed (may already be enabled):', e);
    }
    await browser.pause(5000);

    const status = await getGatewayStatus();
    gatewayPort = status.url ? parseInt(new URL(status.url).port, 10) : 45818;

    // Earlier specs edit the default Space's Starter; put it back in auto
    // mode (what a new install starts with) for the end-to-end checks below.
    const summary = await starterSummary(spaceId);
    await invoke('set_feature_set_auto_include', {
      featureSetId: summary.feature_set_id,
      enabled: true,
    });
  });

  it('TC-ONB-001: defaults need no access key and give Starters every tool', async () => {
    expect(await invoke<boolean>('get_gateway_auth_disabled')).toBe(true);
    expect(await invoke<boolean>('get_starter_auto_include_default')).toBe(true);

    // A brand-new Space's Starter starts in auto mode.
    const fresh = await createSpace('Onboarding E2E');
    try {
      const sets = await listFeatureSetsBySpace(fresh.id);
      const starter = sets.find((s) => s.feature_set_type === 'starter') as
        | { auto_include?: boolean }
        | undefined;
      expect(starter?.auto_include).toBe(true);
    } finally {
      await deleteSpace(fresh.id);
    }
  });

  it('TC-ONB-002: an app with no token gets every tool, no FeatureSet setup', async () => {
    const tools = await listToolsWithoutToken(gatewayPort);
    console.log('[onboarding] tools without token:', tools.length);
    // The stub server's built-in tools arrive with no membership edits.
    expect(tools.some((n) => n.endsWith('_echo'))).toBe(true);
    expect(tools.some((n) => n.endsWith('_add'))).toBe(true);
  });

  it('TC-ONB-003: web pages in a browser are blocked; local pages are not', async () => {
    const fromWebsite = await postMcp(gatewayPort, INITIALIZE, { Origin: 'https://evil.example' });
    expect(fromWebsite.status).toBe(403);

    const fromLocalPage = await postMcp(gatewayPort, INITIALIZE, {
      Origin: 'http://localhost:6274',
    });
    expect(fromLocalPage.status).toBe(200);
  });

  it('TC-ONB-004: Home, footer, and Mapping explain what apps get and showcase @mux', async () => {
    await safeClick(await byTestId('nav-dashboard'));
    const card = await byTestId('starter-tools-card-title');
    await card.waitForDisplayed({ timeout: TIMEOUT.medium });
    expect(await card.getText()).toMatch(/Your apps get \d+ tools? from \d+ servers?/);

    expect(await (await byTestId('statusbar-starter-tools')).isDisplayed()).toBe(true);
    expect(await (await byTestId('statusbar-mux')).isDisplayed()).toBe(true);

    await safeClick(await byTestId('nav-workspaces'));
    const hint = await byTestId('mapping-mux-hint');
    await hint.waitForDisplayed({ timeout: TIMEOUT.medium });
    expect(await hint.getText()).toContain('@mux');

    await browser.saveScreenshot('./tests/e2e/screenshots/onboarding-01-mapping.png');
  });

  it('TC-ONB-005: past 80 tools everything still works, with a warning', async () => {
    for (let i = 0; i < EXTRA_TOOL_COUNT; i++) {
      await addDynamicTool(extraToolName(i), `Extra tool ${i}`);
    }
    try {
      await browser.waitUntil(async () => (await starterSummary(spaceId)).over_threshold, {
        timeout: TIMEOUT.long,
        timeoutMsg: 'Starter never went over the tool-count threshold',
      });

      // Not a cap: the app still gets every tool.
      const summary = await starterSummary(spaceId);
      const tools = await listToolsWithoutToken(gatewayPort);
      expect(tools.length).toBeGreaterThanOrEqual(summary.tool_count);

      await safeClick(await byTestId('nav-dashboard'));
      const warning = await byTestId('starter-tools-warning');
      await warning.waitForDisplayed({ timeout: TIMEOUT.medium });
      expect(await warning.getText()).toContain('@mux');
      await browser.saveScreenshot('./tests/e2e/screenshots/onboarding-02-warning.png');

      await safeClick(await byTestId('nav-featuresets'));
      const fsWarning = await byTestId('featuresets-starter-warning');
      await fsWarning.waitForDisplayed({ timeout: TIMEOUT.medium });
    } finally {
      for (let i = 0; i < EXTRA_TOOL_COUNT; i++) {
        await removeDynamicTool(extraToolName(i)).catch(() => {});
      }
    }
  });

  it('TC-ONB-006: pro users can switch the Starter default off and back on', async () => {
    await safeClick(await byTestId('nav-settings'));
    const toggle = await byTestId('starter-auto-include-switch');
    await toggle.waitForDisplayed({ timeout: TIMEOUT.medium });
    await toggle.scrollIntoView();
    expect(await toggle.getAttribute('aria-checked')).toBe('true');

    // Off: the Starter becomes a manual list (keeping its tools).
    await toggle.click();
    await browser.waitUntil(async () => !(await starterSummary(spaceId)).auto_include, {
      timeout: TIMEOUT.medium,
      timeoutMsg: 'Starter stayed in auto mode after turning the setting off',
    });
    expect(await invoke<boolean>('get_starter_auto_include_default')).toBe(false);
    expect((await starterSummary(spaceId)).tool_count).toBeGreaterThan(0);

    // On again: asks first, then every Starter includes everything.
    await toggle.click();
    await safeClick(await byTestId('confirm-dialog-confirm'));
    await browser.waitUntil(async () => (await starterSummary(spaceId)).auto_include, {
      timeout: TIMEOUT.medium,
      timeoutMsg: 'Starter did not return to auto mode',
    });
    expect(await invoke<boolean>('get_starter_auto_include_default')).toBe(true);
  });
});
