/**
 * Screenshot Capture for Docs & Marketing
 *
 * Seeds realistic data, navigates to each page, and saves focused screenshots
 * of the main content area (excluding sidebar/titlebar) for maximum readability.
 *
 * Strategy:
 *   - Real Tauri API calls for spaces, servers, feature sets, gateway, OAuth clients
 *   - Servers are enabled via enable_server_v2 (sets enabled=true in DB)
 *   - Server connection status overridden to "connected" via Tauri event emission
 *     (window.__TAURI_INTERNALS__.invoke is non-writable/non-configurable in Tauri v2,
 *      so we cannot mock invoke; instead we emit events that useServerManager listens to)
 *   - OAuth clients registered via HTTP POST to gateway's DCR endpoint, then approved
 *   - Screenshots capture the <main> content area only (not full window) for readability
 *   - Additional detail screenshots capture modals/panels for feature-specific views
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
  seedServerFeatures,
  listFeatureSetsBySpace,
  listServerFeatures,
  listClients,
  addFeatureToSet,
  grantFeatureSetToClient,
  grantOAuthClientFeatureSet,
} from '../helpers/tauri-api';
import { PRESEED } from '../mocks/screenshot-preseed';

// Output paths
const DOCS_DIR = path.resolve('./docs/screenshots');
const DISCOVER_DIR = path.resolve('../mcpmux.discover.ui/public/screenshots');

function ensureDir(dir: string) {
  fs.mkdirSync(dir, { recursive: true });
}

/**
 * Save a screenshot of the main content area only (excludes sidebar and titlebar).
 * Falls back to full browser screenshot if <main> element is not found.
 */
async function saveScreenshot(name: string) {
  ensureDir(DOCS_DIR);
  const docsPath = path.join(DOCS_DIR, `${name}.png`);

  // Try to capture just the main content area for focused, readable screenshots
  try {
    const main = await $('main');
    const isDisplayed = await main.isDisplayed();
    if (isDisplayed) {
      await main.saveScreenshot(docsPath);
      console.log(`[screenshot] Saved (content-focused): ${docsPath}`);
    } else {
      await browser.saveScreenshot(docsPath);
      console.log(`[screenshot] Saved (full window fallback): ${docsPath}`);
    }
  } catch {
    await browser.saveScreenshot(docsPath);
    console.log(`[screenshot] Saved (full window fallback): ${docsPath}`);
  }

  // Copy to discover UI public folder
  copyToDiscover(docsPath, name);
}

/**
 * Save a full-window screenshot (for contexts where sidebar context is valuable).
 */
async function saveFullScreenshot(name: string) {
  ensureDir(DOCS_DIR);
  const docsPath = path.join(DOCS_DIR, `${name}.png`);
  await browser.saveScreenshot(docsPath);
  console.log(`[screenshot] Saved (full window): ${docsPath}`);
  copyToDiscover(docsPath, name);
}

/**
 * Save a screenshot of a specific element by selector.
 */
async function saveElementScreenshot(name: string, selector: string) {
  ensureDir(DOCS_DIR);
  const docsPath = path.join(DOCS_DIR, `${name}.png`);

  try {
    const element = await $(selector);
    await element.waitForDisplayed({ timeout: 5000 });
    await element.saveScreenshot(docsPath);
    console.log(`[screenshot] Saved (element: ${selector}): ${docsPath}`);
    copyToDiscover(docsPath, name);
  } catch (err) {
    console.warn(`[screenshot] Failed to capture element ${selector}: ${err}`);
  }
}

function copyToDiscover(srcPath: string, name: string) {
  try {
    ensureDir(DISCOVER_DIR);
    const discoverPath = path.join(DISCOVER_DIR, `${name}.png`);
    fs.copyFileSync(srcPath, discoverPath);
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
    logo_uri: 'https://avatars.githubusercontent.com/u/157927506?v=4',
    software_id: 'com.cursor.app',
    software_version: '0.48.2',
  },
  {
    client_name: 'VS Code',
    redirect_uris: ['http://127.0.0.1:6275/callback'],
    logo_uri: 'https://avatars.githubusercontent.com/u/9950313?v=4',
    software_id: 'com.microsoft.vscode',
    software_version: '1.96.4',
  },
  {
    client_name: 'Windsurf',
    redirect_uris: ['http://127.0.0.1:6277/callback'],
    logo_uri: 'https://avatars.githubusercontent.com/u/137354558?v=4',
    software_id: 'com.codeium.windsurf',
    software_version: '1.6.0',
  },
  {
    client_name: 'Claude Code',
    redirect_uris: ['http://127.0.0.1:6278/callback'],
    logo_uri: 'https://avatars.githubusercontent.com/u/76263028?v=4',
    software_id: 'com.anthropic.claude-code',
    software_version: '1.0.0',
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
  let oauthClientIds: string[] = [];

  before(async () => {
    // Use a large window so content area screenshots are high-res and readable.
    // Content area = window minus sidebar (240px) and titlebar (36px).
    // At 1600x1000, the content area is ~1360x964 — plenty of detail.
    await browser.setWindowSize(1600, 1000);

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

    // Seed server features (tools, prompts, resources) into the DB
    // so the server expanded view and feature set panel show real data.
    try {
      const featureDefs = PRESEED.serverFeatures(defaultSpaceId);
      const featureIds = await seedServerFeatures(featureDefs);
      console.log(`[setup] Seeded ${featureIds.length} server features`);

      // Assign realistic subsets of features to custom feature sets.
      // Deliberately partial selection so screenshots show mixed enable/disable states.
      const allFeatureSets = await listFeatureSetsBySpace(defaultSpaceId);
      const allFeatures = await listServerFeatures(defaultSpaceId, true);

      // ── React Development: GitHub + Filesystem read tools ──
      const reactDevFS = allFeatureSets.find((fs) => fs.name === 'React Development');
      if (reactDevFS) {
        const reactInclude = new Set([
          'get_file_contents', 'search_repositories', 'search_code', 'list_commits',
          'read_file', 'list_directory', 'search_files', 'write_file',
          'web_search',
        ]);
        const reactServerIds = ['github-server', 'filesystem-server', 'brave-search'];
        const reactFeatures = allFeatures.filter(
          (f) => reactServerIds.includes(f.server_id) && reactInclude.has(f.feature_name)
        );
        for (const feature of reactFeatures) {
          try { await addFeatureToSet(reactDevFS.id, feature.id, 'include'); } catch { /* ignore */ }
        }
        console.log(`[setup] Assigned ${reactFeatures.length} features to React Development`);
      }

      // ── Cloudflare Workers: Cloudflare + Docker tools ──
      const cloudflareFS = allFeatureSets.find((fs) => fs.name === 'Cloudflare Workers');
      if (cloudflareFS) {
        const cfInclude = new Set([
          'list_workers', 'get_worker_code', 'list_kv_namespaces',
          'list_containers', 'run_container', 'container_logs',
        ]);
        const cfServerIds = ['cloudflare-workers-server', 'docker-server'];
        const cfFeatures = allFeatures.filter(
          (f) => cfServerIds.includes(f.server_id) && cfInclude.has(f.feature_name)
        );
        for (const feature of cfFeatures) {
          try { await addFeatureToSet(cloudflareFS.id, feature.id, 'include'); } catch { /* ignore */ }
        }
        console.log(`[setup] Assigned ${cfFeatures.length} features to Cloudflare Workers`);
      }

      // ── Research & Analysis: Brave Search + Notion + PostgreSQL query ──
      const researchFS = allFeatureSets.find((fs) => fs.name === 'Research & Analysis');
      if (researchFS) {
        const researchInclude = new Set([
          'web_search', 'local_search',
          'search_pages', 'create_page',
          'query', 'list_tables', 'describe_table',
        ]);
        const researchServerIds = ['brave-search', 'notion-server', 'postgres-server'];
        const researchFeatures = allFeatures.filter(
          (f) => researchServerIds.includes(f.server_id) && researchInclude.has(f.feature_name)
        );
        for (const feature of researchFeatures) {
          try { await addFeatureToSet(researchFS.id, feature.id, 'include'); } catch { /* ignore */ }
        }
        console.log(`[setup] Assigned ${researchFeatures.length} features to Research & Analysis`);
      }

      // ── Full Stack Dev: GitHub + Filesystem + PostgreSQL + Docker ──
      const fullStackFS = allFeatureSets.find((fs) => fs.name === 'Full Stack Dev');
      if (fullStackFS) {
        const fullStackServerIds = ['github-server', 'filesystem-server', 'postgres-server', 'docker-server'];
        const fullStackFeatures = allFeatures.filter(
          (f) => fullStackServerIds.includes(f.server_id)
        );
        for (const feature of fullStackFeatures) {
          try { await addFeatureToSet(fullStackFS.id, feature.id, 'include'); } catch { /* ignore */ }
        }
        console.log(`[setup] Assigned ${fullStackFeatures.length} features to Full Stack Dev`);
      }

      // ── Read Only: only read/list/search/query tools (no writes, deletes, mutations) ──
      const readOnlyFS = allFeatureSets.find((fs) => fs.name === 'Read Only');
      if (readOnlyFS) {
        const readOnlyInclude = new Set([
          'get_file_contents', 'search_repositories', 'search_code', 'list_commits',
          'read_file', 'list_directory', 'search_files',
          'query', 'list_tables', 'describe_table',
          'list_channels', 'search_messages',
          'web_search', 'local_search',
          'list_containers', 'container_logs', 'list_images',
          'search_pages',
          'list_s3_buckets', 'get_s3_object', 'describe_instances',
          'list_workers', 'get_worker_code', 'list_kv_namespaces',
          'list_resource_groups', 'list_vms', 'query_cosmos_db',
        ]);
        const readOnlyFeatures = allFeatures.filter(
          (f) => readOnlyInclude.has(f.feature_name)
        );
        for (const feature of readOnlyFeatures) {
          try { await addFeatureToSet(readOnlyFS.id, feature.id, 'include'); } catch { /* ignore */ }
        }
        console.log(`[setup] Assigned ${readOnlyFeatures.length} features to Read Only`);
      }
    } catch (e) {
      console.warn('[setup] Feature seeding failed:', e);
    }

    // Ensure gateway is running (may have auto-started when servers were enabled)
    try {
      gatewayUrl = await ensureGatewayRunning();
      await browser.pause(1000); // Let gateway fully initialize

      // Register and approve OAuth clients via DCR
      oauthClientIds = await registerOAuthClients(gatewayUrl);
    } catch (e) {
      console.warn('[setup] Gateway/OAuth setup failed:', e);
      gatewayUrl = '';
    }

    // Grant feature sets to OAuth clients so Permissions checkboxes and Effective Features have data.
    // IMPORTANT: Use grantOAuthClientFeatureSet (inbound_clients table), NOT grantFeatureSetToClient (clients table).
    // Only grant specific custom sets — NOT "All Features" — to show realistic granular permissions.
    try {
      const allFeatureSets = await listFeatureSetsBySpace(defaultSpaceId);
      const reactDevFS = allFeatureSets.find((fs) => fs.name === 'React Development');
      const fullStackFS = allFeatureSets.find((fs) => fs.name === 'Full Stack Dev');
      const readOnlyFS = allFeatureSets.find((fs) => fs.name === 'Read Only');

      if (oauthClientIds.length > 0) {
        const firstClientId = oauthClientIds[0]; // Cursor
        // Grant "React Development" (custom, 9 features)
        if (reactDevFS) {
          await grantOAuthClientFeatureSet(firstClientId, defaultSpaceId, reactDevFS.id);
          console.log(`[setup] Granted React Development to first OAuth client`);
        }
        // Grant "Full Stack Dev" (custom, 55 features)
        if (fullStackFS) {
          await grantOAuthClientFeatureSet(firstClientId, defaultSpaceId, fullStackFS.id);
          console.log(`[setup] Granted Full Stack Dev to first OAuth client`);
        }
        // Grant "Read Only" to second client (VS Code) for variety
        if (oauthClientIds.length > 1 && readOnlyFS) {
          await grantOAuthClientFeatureSet(oauthClientIds[1], defaultSpaceId, readOnlyFS.id);
          console.log(`[setup] Granted Read Only to second OAuth client`);
        }
      }
    } catch (e) {
      console.warn('[setup] OAuth client feature set grant failed:', e);
    }

    // Set active space back to default
    await setActiveSpace(defaultSpaceId);

    // Reload the page so the frontend store picks up all seeded data
    // (spaces, feature sets, etc. created via Tauri invoke aren't in the Zustand store yet)
    await browser.refresh();
    await browser.pause(3000);
  });

  // ── Page Screenshots (content area only) ───────────────────────────

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

  it('captures Connected Clients with approval modal', async () => {
    const nav = await byTestId('nav-clients');
    await safeClick(nav);
    await browser.pause(2000);

    // Trigger an OAuth consent modal over the clients page.
    // Flow: register a new client → hit /oauth/authorize → extract request_id → emit event
    if (gatewayUrl) {
      try {
        // Register a new client (not yet approved) for the consent flow
        const consentResp = await fetch(`${gatewayUrl}/oauth/register`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            client_name: 'VS Code',
            redirect_uris: ['http://127.0.0.1:6280/callback'],
            logo_uri: 'https://avatars.githubusercontent.com/u/9950313?v=4',
            software_id: 'com.microsoft.vscode',
            software_version: '1.96.4',
          }),
        });

        if (consentResp.ok) {
          const consentClient = (await consentResp.json()) as { client_id: string };

          // Generate PKCE challenge
          const codeVerifier = 'screenshot-test-verifier-01234567890123456789012345678901234567890123';
          const encoder = new TextEncoder();
          const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(codeVerifier));
          const codeChallenge = btoa(String.fromCharCode(...new Uint8Array(hashBuffer)))
            .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');

          // Hit authorize endpoint — creates pending consent in gateway memory
          const authorizeUrl = `${gatewayUrl}/oauth/authorize?` + new URLSearchParams({
            response_type: 'code',
            client_id: consentClient.client_id,
            redirect_uri: 'http://127.0.0.1:6280/callback',
            scope: 'mcp',
            code_challenge: codeChallenge,
            code_challenge_method: 'S256',
          }).toString();

          const authResp = await fetch(authorizeUrl, { redirect: 'manual' });
          // The response is HTML containing a deep link with request_id
          const html = await authResp.text();
          const match = html.match(/request_id=([a-f0-9-]+)/);

          if (match) {
            const requestId = match[1];
            console.log(`[setup] Created pending consent: ${requestId}`);

            // Emit the event that triggers OAuthConsentModal (simulates deep link)
            await emitEvent('oauth-consent-request', { requestId });
            await browser.pause(3000); // Wait for modal to validate and render
            console.log('[setup] Consent modal should be visible');
          } else {
            console.warn('[screenshot] Could not extract request_id from authorize response');
          }
        }
      } catch (e) {
        console.warn('[screenshot] Failed to trigger consent modal:', e);
      }
    }

    // Capture full window to show consent modal overlay + clients behind
    await saveFullScreenshot('clients');

    // Dismiss the consent modal — click the "Dismiss" link or remove via JS
    try {
      // Try clicking the "Dismiss (client will wait)" button
      const dismissBtns = await $$('button');
      for (const btn of dismissBtns) {
        const text = await btn.getText();
        if (text.includes('Dismiss')) {
          await browser.execute((el: HTMLElement) => el.click(), btn as any);
          await browser.pause(500);
          break;
        }
      }
      // Fallback: force-remove any z-50 overlay still present
      await browser.execute(() => {
        const overlay = document.querySelector('.fixed.inset-0.bg-black\\/50');
        if (overlay) overlay.remove();
      });
      await browser.pause(500);
    } catch { /* ignore */ }
  });

  // Dashboard captured after other pages so it remounts with fresh data
  // (client count, server stats reflect all setup done in before() hook)
  it('captures Dashboard', async () => {
    // Clear any lingering overlays from consent modal
    await browser.execute(() => {
      document.querySelectorAll('.fixed.inset-0').forEach((el) => el.remove());
    });
    await browser.pause(300);
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

  // ── Detail Screenshots (modals, panels, focused views) ─────────────

  // Helper: dismiss any open panel/modal and navigate to ensure clean state
  async function dismissAndNavigate(navTestId: string) {
    // Force-remove any z-50 overlay (consent modal, etc.) that may be blocking
    await browser.execute(() => {
      document.querySelectorAll('.fixed.inset-0').forEach((el) => el.remove());
    });
    await browser.pause(300);

    // Try Escape multiple times to close any open panels/modals/dropdowns
    for (let i = 0; i < 3; i++) {
      await browser.keys('Escape');
      await browser.pause(300);
    }

    // Click the main content area to defocus any panels
    try {
      const main = await $('main');
      if (await main.isDisplayed()) {
        await main.click();
        await browser.pause(300);
      }
    } catch { /* ignore */ }

    // Now navigate — use direct click without safeClick's strict waitForClickable
    // since we may still have overlay remnants
    const nav = await byTestId(navTestId);
    try {
      await nav.waitForClickable({ timeout: 5000 });
      await nav.click();
    } catch {
      // Fallback: force-click via JS if element exists but is "not clickable"
      await browser.execute((el: HTMLElement) => el.click(), nav as any);
    }
    await browser.pause(2000);
  }

  it('captures Feature Set detail panel', async () => {
    await dismissAndNavigate('nav-featuresets');

    // Click "React Development" feature set to show the detail panel with assigned features
    try {
      const featureSetCards = await $$('[data-testid="featuresets-page"] [data-testid^="featureset-card"]');
      let clicked = false;
      for (const card of featureSetCards) {
        const text = await card.getText();
        if (text.includes('React Development')) {
          await card.click();
          clicked = true;
          break;
        }
      }
      if (!clicked && featureSetCards.length > 0) {
        await featureSetCards[0].click();
      }
      // Wait for features to load — the "Included Features" section is expanded by default
      await browser.pause(3000);

      await saveFullScreenshot('featureset-detail');
    } catch (err) {
      console.warn(`[screenshot] Could not capture feature set detail: ${err}`);
    }
  });

  it('captures Client detail panel with granted feature sets', async () => {
    await dismissAndNavigate('nav-clients');

    // Click first client card to open the detail/authorization panel
    try {
      const clientCards = await $$('[data-testid="clients-page"] [data-testid^="client-card"]');
      if (clientCards.length > 0) {
        await clientCards[0].click();
        await browser.pause(2000);

        // Expand "Permissions" section if collapsed (it defaults to expanded)
        // and collapse "Quick Settings" to make room for permissions
        const buttons = await $$('button');
        for (const btn of buttons) {
          const text = await btn.getText();
          if (text.includes('Quick Settings')) {
            // Quick Settings is expanded by default — collapse it to focus on Permissions
            await btn.click();
            await browser.pause(500);
            break;
          }
        }

        await browser.pause(1000);
        await saveFullScreenshot('client-detail');
      } else {
        console.warn('[screenshot] No client cards found for detail screenshot');
      }
    } catch (err) {
      console.warn(`[screenshot] Could not capture client detail: ${err}`);
    }
  });

  it('captures Server Install modal', async () => {
    await dismissAndNavigate('nav-discover');
    await browser.pause(1000); // Extra wait for registry load

    // Click the first server card's install button
    try {
      const installBtns = await $$('[data-testid^="install-btn-"]');
      if (installBtns.length > 0) {
        await installBtns[0].click();
        await browser.pause(1500);

        // Capture the install modal
        await saveElementScreenshot('install-modal', '[data-testid="install-modal"]');

        // Close the modal
        const cancelBtn = await $('[data-testid="install-modal-cancel-btn"]');
        if (await cancelBtn.isDisplayed().catch(() => false)) {
          await cancelBtn.click();
        } else {
          await browser.keys('Escape');
        }
        await browser.pause(500);
      } else {
        console.warn('[screenshot] No install buttons found for modal screenshot');
      }
    } catch (err) {
      console.warn(`[screenshot] Could not capture install modal: ${err}`);
    }
  });

  it('captures Server expanded view with tools/resources', async () => {
    await dismissAndNavigate('nav-my-servers');

    // Ensure servers show as connected
    await emitConnectedStatus(PRESEED.serversToInstall, defaultSpaceId);
    await browser.pause(2000);

    // Emit features-refreshed event so the UI knows features are available
    // and shows the expand chevron with feature count badges
    for (const serverId of PRESEED.serversToInstall) {
      await emitEvent('server-features-refreshed', {
        space_id: defaultSpaceId,
        server_id: serverId,
        tools_count: 5,
        prompts_count: 1,
        resources_count: 1,
        added: [],
        removed: [],
      });
    }
    await browser.pause(1000);

    // Expand GitHub server to show its rich tool set (8 seeded tools + 1 resource)
    try {
      // First scroll to GitHub in the server list (it may be below the fold)
      const githubCard = await $('[data-testid="server-card-github-server"]');
      if (await githubCard.isExisting()) {
        await githubCard.scrollIntoView({ block: 'center' });
        await browser.pause(500);
      }

      // Now click its expand button
      let expandBtn = await $('[data-testid="expand-server-github-server"]');
      if (!(await expandBtn.isExisting())) {
        // Fallback: any expandable server
        expandBtn = await $('[data-testid^="expand-server-"]');
      }
      if (await expandBtn.isDisplayed()) {
        await expandBtn.click();
        await browser.pause(3000); // Wait for features to load from DB
        await saveFullScreenshot('server-expanded');
      } else {
        console.warn('[screenshot] No expand button found');
      }
    } catch (err) {
      console.warn(`[screenshot] Could not capture server expanded: ${err}`);
    }
  });

  it('captures Client permissions panel (granting feature sets)', async () => {
    await dismissAndNavigate('nav-clients');

    // Click first client to open detail panel
    try {
      const clientCards = await $$('[data-testid="clients-page"] [data-testid^="client-card"]');
      if (clientCards.length > 0) {
        await clientCards[0].click();
        await browser.pause(2000);

        // Collapse "Quick Settings" to make room for the features sections
        const buttons = await $$('button');
        for (const btn of buttons) {
          const text = await btn.getText();
          if (text.includes('Quick Settings')) {
            await btn.click();
            await browser.pause(500);
            break;
          }
        }

        // Expand "Effective Features" to show resolved tools/prompts/resources
        const buttons2 = await $$('button');
        for (const btn of buttons2) {
          const text = await btn.getText();
          if (text.includes('Effective Features')) {
            await btn.click();
            await browser.pause(2000); // Wait for features to load
            break;
          }
        }

        await saveFullScreenshot('client-permissions');
      }
    } catch (err) {
      console.warn(`[screenshot] Could not capture client permissions: ${err}`);
    }
  });

  it('captures Space switcher dropdown', async () => {
    await dismissAndNavigate('nav-dashboard');

    // The space switcher is in the sidebar — click it to open the dropdown
    try {
      const sidebar = await $('[data-testid="sidebar"]');
      const spaceSwitcherButtons = await sidebar.$$('button');

      for (const btn of spaceSwitcherButtons) {
        const text = await btn.getText();
        // The space switcher shows the current space name
        if (text.includes('Default') || text.includes('Work') || text.includes('Personal') || text.includes('My Space') || text.includes('Open Source')) {
          await btn.click();
          await browser.pause(1000);

          // Capture the full window to show sidebar + dropdown
          await saveFullScreenshot('space-switcher');

          // Close the dropdown
          await browser.keys('Escape');
          await browser.pause(500);
          break;
        }
      }
    } catch (err) {
      console.warn(`[screenshot] Could not capture space switcher: ${err}`);
    }
  });

  // ── Discover Web Screenshot ────────────────────────────────────────

  it('captures Discover Web UI', async function () {
    // Take a screenshot of the discover web UI (Next.js site)
    // This requires the discover dev server running on port 3000
    this.timeout(30000);
    const DISCOVER_WEB_URL = 'http://localhost:3000';

    // First check if the dev server is running
    try {
      const resp = await fetch(DISCOVER_WEB_URL, { signal: AbortSignal.timeout(3000) });
      if (!resp.ok) {
        console.warn('[screenshot] Discover dev server returned non-OK status, skipping');
        return;
      }
    } catch {
      console.warn('[screenshot] Discover dev server not running on port 3000, skipping');
      console.warn('[screenshot] To capture: cd ../mcpmux.discover.ui && pnpm dev');
      return;
    }

    try {
      const originalUrl = await browser.getUrl();

      await browser.url(DISCOVER_WEB_URL);
      await browser.pause(3000);

      await browser.setWindowSize(1920, 1080);
      await browser.pause(500);

      // Scroll down past the hero so the server grid is the main focus.
      // First scroll to the category pills, then nudge further so the
      // server cards fill the viewport.
      try {
        const categoryPill = await $('[data-testid="category-pill-all"]');
        if (await categoryPill.isExisting()) {
          await categoryPill.scrollIntoView({ block: 'start' });
          await browser.pause(300);
          // Scroll down more so the server cards fill the viewport
          await browser.execute(() => window.scrollBy(0, 500));
          await browser.pause(500);
        }
      } catch {
        // Fallback: scroll by a large fixed amount to get past the hero
        await browser.execute(() => window.scrollBy(0, 900));
        await browser.pause(500);
      }

      await saveFullScreenshot('discover-web');

      await browser.url(originalUrl);
      await browser.setWindowSize(1600, 1000);
      await browser.pause(2000);
    } catch (err) {
      console.warn(`[screenshot] Could not capture discover web UI: ${err}`);
    }
  });
});
