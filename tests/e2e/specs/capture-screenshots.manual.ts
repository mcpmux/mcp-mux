/**
 * Screenshot Capture for Docs & Marketing
 *
 * Seeds realistic data, navigates to each page, and saves screenshots
 * to docs/screenshots/ for the README and discover UI.
 *
 * Strategy:
 *   - Real Tauri API calls for spaces, servers, feature sets, gateway, OAuth clients
 *   - Servers are enabled via enable_server_v2 (sets enabled=true in DB)
 *   - Server connection status overridden to "connected" via Tauri event emission
 *     (window.__TAURI_INTERNALS__.invoke is non-writable/non-configurable in Tauri v2,
 *      so we cannot mock invoke; instead we emit events that useServerManager listens to)
 *   - OAuth clients registered via HTTP POST to gateway's DCR endpoint, then approved
 *
 * This file uses .manual.ts (not .wdio.ts) so it is excluded from `pnpm test:e2e`.
 * It must be invoked explicitly:
 *
 *   pnpm exec wdio run tests/e2e/wdio.conf.ts --spec tests/e2e/specs/capture-screenshots.manual.ts
 *
 * Prerequisites:
 *   - Desktop app built: `pnpm build` (needs target/release/mcpmux.exe)
 *   - Mock bundle API fixtures updated: tests/e2e/mocks/mock-bundle-api/fixtures.ts
 *
 * To change what appears in screenshots, edit:
 *   tests/e2e/mocks/screenshot-preseed.ts   (spaces, servers to install, feature sets)
 *   This file's OAUTH_CLIENTS constant        (DCR client registrations)
 *
 * Output:
 *   - docs/screenshots/*.png              (for README)
 *   - ../mcpmux.discover.ui/public/screenshots/*.png (for discover site)
 */

import path from 'path';
import fs from 'fs';
import { byTestId, safeClick } from '../helpers/selectors';
import {
  createSpace,
  setActiveSpace,
  createFeatureSet,
  installServer,
  getActiveSpace,
  refreshRegistry,
  enableServerV2,
  emitEvent,
  approveOAuthClient,
} from '../helpers/tauri-api';
import { PRESEED } from '../mocks/screenshot-preseed';

// Output paths
const DOCS_DIR = path.resolve('./docs/screenshots');
const DISCOVER_DIR = path.resolve('../mcpmux.discover.ui/public/screenshots');

function ensureDir(dir: string) {
  fs.mkdirSync(dir, { recursive: true });
}

async function saveScreenshot(name: string) {
  ensureDir(DOCS_DIR);
  const docsPath = path.join(DOCS_DIR, `${name}.png`);
  await browser.saveScreenshot(docsPath);
  console.log(`[screenshot] Saved: ${docsPath}`);

  // Copy to discover UI public folder
  try {
    ensureDir(DISCOVER_DIR);
    const discoverPath = path.join(DISCOVER_DIR, `${name}.png`);
    fs.copyFileSync(docsPath, discoverPath);
    console.log(`[screenshot] Copied to: ${discoverPath}`);
  } catch (err) {
    console.warn(`[screenshot] Could not copy to discover UI: ${err}`);
  }
}

// ── OAuth Client Definitions for DCR Registration ────────────────────

const OAUTH_CLIENTS = [
  {
    client_name: 'Cursor',
    redirect_uris: ['http://127.0.0.1:6274/callback'],
    logo_uri: 'https://github.com/getcursor.png?size=128',
    software_id: 'com.cursor.app',
    software_version: '0.48.2',
  },
  {
    client_name: 'VS Code',
    redirect_uris: ['http://127.0.0.1:6275/callback'],
    logo_uri: 'https://github.com/microsoft.png?size=128',
    software_id: 'com.microsoft.vscode',
    software_version: '1.96.4',
  },
  {
    client_name: 'Claude Desktop',
    redirect_uris: ['http://127.0.0.1:6276/callback'],
    logo_uri: 'https://github.com/anthropics.png?size=128',
    software_id: 'com.anthropic.claude-desktop',
    software_version: '1.2.0',
  },
  {
    client_name: 'Windsurf',
    redirect_uris: ['http://127.0.0.1:6277/callback'],
    logo_uri: 'https://github.com/codeium.png?size=128',
    software_id: 'com.codeium.windsurf',
    software_version: '1.6.0',
  },
];

/**
 * Ensure the gateway is running and return its base URL.
 * First checks if already running (may auto-start when servers are enabled),
 * then starts it if needed.
 */
async function ensureGatewayRunning(): Promise<string> {
  // Check if gateway is already running
  const status = await browser.executeAsync(
    (done: (result: { running: boolean; url: string | null }) => void) => {
      (window as any).__TAURI_TEST_API__
        .invoke('get_gateway_status', {})
        .then((s: { running: boolean; url: string | null }) => done(s))
        .catch(() => done({ running: false, url: null }));
    }
  ) as { running: boolean; url: string | null };

  if (status.running && status.url) {
    console.log(`[setup] Gateway already running at: ${status.url}`);
    return status.url;
  }

  // Start the gateway
  const url = await browser.executeAsync(
    (done: (result: string) => void) => {
      (window as any).__TAURI_TEST_API__
        .invoke('start_gateway', {})
        .then((result: string) => done(result))
        .catch((e: unknown) => done('ERROR:' + String(e)));
    }
  );

  if (typeof url === 'string' && !url.startsWith('ERROR:')) {
    console.log(`[setup] Gateway started at: ${url}`);
    return url;
  }

  // Fallback: check status again (might have started despite error)
  console.warn(`[setup] start_gateway returned: ${url}, checking status...`);
  const retry = await browser.executeAsync(
    (done: (result: { running: boolean; url: string | null }) => void) => {
      (window as any).__TAURI_TEST_API__
        .invoke('get_gateway_status', {})
        .then((s: { running: boolean; url: string | null }) => done(s))
        .catch(() => done({ running: false, url: null }));
    }
  ) as { running: boolean; url: string | null };

  if (retry.running && retry.url) {
    console.log(`[setup] Gateway running (fallback) at: ${retry.url}`);
    return retry.url;
  }

  throw new Error('Gateway failed to start');
}

/**
 * Register OAuth clients via DCR POST and approve them.
 * Returns the registered client IDs.
 */
async function registerOAuthClients(gatewayUrl: string): Promise<string[]> {
  const clientIds: string[] = [];
  for (const client of OAUTH_CLIENTS) {
    try {
      const response = await fetch(`${gatewayUrl}/oauth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(client),
      });
      if (!response.ok) {
        console.warn(`[setup] DCR failed for ${client.client_name}: ${response.status}`);
        continue;
      }
      const data = (await response.json()) as { client_id: string };
      clientIds.push(data.client_id);
      console.log(`[setup] Registered OAuth client: ${client.client_name} (${data.client_id})`);

      // Approve the client (bypasses consent flow for E2E)
      await approveOAuthClient(data.client_id);
      console.log(`[setup] Approved: ${client.client_name}`);
    } catch (e) {
      console.warn(`[setup] Failed to register ${client.client_name}:`, e);
    }
  }
  return clientIds;
}

/**
 * Emit server-status-changed events to override the connection status.
 * Uses a high flow_id (9999) to ensure events are accepted by useServerManager
 * even if the real backend already emitted events with lower flow_ids.
 */
async function emitConnectedStatus(serverIds: string[], spaceId: string): Promise<void> {
  for (const serverId of serverIds) {
    await emitEvent('server-status-changed', {
      space_id: spaceId,
      server_id: serverId,
      status: 'connected',
      flow_id: 9999,
      has_connected_before: true,
      message: null,
    });
  }
  console.log(`[mock] Emitted 'connected' status for ${serverIds.length} servers`);
}

describe('Screenshot Capture', function () {
  this.timeout(120000);

  let defaultSpaceId: string;
  let gatewayUrl: string;

  before(async () => {
    // Set a fixed window size for readable, consistent screenshots.
    // 1280x800 is optimal: text is legible, full UI visible, standard 16:10 ratio.
    await browser.setWindowSize(1280, 800);

    // ---- Seed data from preseed config ----

    // Get default space
    const activeSpace = await getActiveSpace();
    defaultSpaceId = activeSpace?.id || '';
    console.log('[setup] Default space:', defaultSpaceId);

    // Create additional spaces
    for (const spaceDef of PRESEED.spaces) {
      try {
        const space = await createSpace(spaceDef.name, spaceDef.icon);
        console.log(`[setup] Created space: ${spaceDef.name} (${space.id})`);
      } catch (e) {
        console.warn(`[setup] Failed to create space ${spaceDef.name}:`, e);
      }
    }

    // Force-refresh the registry so server definitions are available
    await refreshRegistry();
    await browser.pause(2000);

    // Install servers into default space
    for (const serverId of PRESEED.serversToInstall) {
      try {
        await installServer(serverId, defaultSpaceId);
        console.log(`[setup] Installed ${serverId}`);
      } catch (e) {
        console.warn(`[setup] Failed to install ${serverId}:`, e);
      }
    }

    // Enable all servers (sets enabled=true in DB, triggers background connection attempts)
    for (const serverId of PRESEED.serversToInstall) {
      try {
        await enableServerV2(defaultSpaceId, serverId);
        console.log(`[setup] Enabled ${serverId}`);
      } catch (e) {
        console.warn(`[setup] Failed to enable ${serverId}:`, e);
      }
    }

    // Create feature sets
    for (const fsDef of PRESEED.featureSets) {
      try {
        await createFeatureSet({
          name: fsDef.name,
          space_id: defaultSpaceId,
          description: fsDef.description,
          icon: fsDef.icon,
        });
        console.log(`[setup] Created feature set: ${fsDef.name}`);
      } catch (e) {
        console.warn(`[setup] Failed to create feature set ${fsDef.name}:`, e);
      }
    }

    // Ensure gateway is running (may have auto-started when servers were enabled)
    try {
      gatewayUrl = await ensureGatewayRunning();
      await browser.pause(1000); // Let gateway fully initialize

      // Register and approve OAuth clients via DCR
      await registerOAuthClients(gatewayUrl);
    } catch (e) {
      console.warn('[setup] Gateway/OAuth setup failed:', e);
      gatewayUrl = '';
    }

    // Set active space back to default
    await setActiveSpace(defaultSpaceId);

    // Reload the page so the frontend store picks up all seeded data
    // (spaces, feature sets, etc. created via Tauri invoke aren't in the Zustand store yet)
    await browser.refresh();
    await browser.pause(3000);
  });

  it('captures My Servers', async () => {
    const nav = await byTestId('nav-my-servers');
    await safeClick(nav);
    await browser.pause(2000);

    // Override server statuses to show "Connected" via Tauri events.
    // useServerManager listens for these events and updates React state directly.
    await emitConnectedStatus(PRESEED.serversToInstall, defaultSpaceId);
    await browser.pause(2000); // Wait for React to re-render

    await saveScreenshot('servers');
  });

  it('captures Discover Registry', async () => {
    const nav = await byTestId('nav-discover');
    await safeClick(nav);
    await browser.pause(3000); // Registry loads from mock bundle API
    await saveScreenshot('discover');
  });

  it('captures Spaces', async () => {
    const nav = await byTestId('nav-spaces');
    await safeClick(nav);
    await browser.pause(2000);
    await saveScreenshot('spaces');
  });

  it('captures FeatureSets', async () => {
    const nav = await byTestId('nav-featuresets');
    await safeClick(nav);
    await browser.pause(2000);
    await saveScreenshot('featuresets');
  });

  it('captures Connected Clients', async () => {
    const nav = await byTestId('nav-clients');
    await safeClick(nav);
    await browser.pause(2000);
    await saveScreenshot('clients');
  });

  // Dashboard captured after other pages so it remounts with fresh data
  // (client count, server stats reflect all setup done in before() hook)
  it('captures Dashboard', async () => {
    const nav = await byTestId('nav-dashboard');
    await safeClick(nav);
    await browser.pause(2000);
    await saveScreenshot('dashboard');
  });

  it('captures Settings', async () => {
    const nav = await byTestId('nav-settings');
    await safeClick(nav);
    await browser.pause(2000);
    await saveScreenshot('settings');
  });
});
