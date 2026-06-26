#!/usr/bin/env node
/**
 * Tauri dev with web admin enabled for the session (MCPMUX_DEV_ADMIN=1,
 * VITE_ADMIN_WEB=1). Vite proxies /api → :45819 so the browser tab at :1420
 * uses the same REST + SSE transport as production web admin.
 * Opens the HMR URL in the default browser after the admin health check passes.
 *
 * Usage (repo root): pnpm dev:admin
 */

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { loadRepoDotEnv } from './cf-access-env.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ADMIN_PORT = Number.parseInt(process.env.MCPMUX_ADMIN_PORT ?? '45819', 10);
const GATEWAY_PORT = Number.parseInt(process.env.MCPMUX_GATEWAY_PORT ?? '45818', 10);
const HEALTH_URL = `http://127.0.0.1:${ADMIN_PORT}/api/v1/health`;
const GATEWAY_HEALTH_URL = `http://127.0.0.1:${GATEWAY_PORT}/health`;
const VITE_URL = 'http://127.0.0.1:1420';
const OPEN_WAIT_MS = 90_000;
const POLL_MS = 500;
/** Number of consecutive healthy responses required before gateway is considered stable. */
const GATEWAY_STABLE_TICKS = 3;
/** Minimum ms between stability ticks — ensures we aren't measuring the same in-flight response twice. */
const GATEWAY_STABLE_INTERVAL_MS = 1_000;

/**
 * @param {number} ms
 * @returns {Promise<void>}
 */
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * @returns {Promise<boolean>}
 */
async function adminHealthOk() {
  try {
    const response = await fetch(HEALTH_URL, {
      method: 'GET',
      headers: { Accept: 'application/json' },
    });
    return response.ok;
  } catch {
    return false;
  }
}

/**
 * Poll :45818/health until the gateway has responded consistently for
 * GATEWAY_STABLE_TICKS consecutive checks, indicating it is not mid-restart.
 * Logs progress so the dev sees what's happening.
 *
 * @param {number} deadlineMs - absolute timestamp after which we give up
 * @returns {Promise<boolean>} true if stable, false if timed out
 */
async function waitForStableGateway(deadlineMs) {
  let ticks = 0;
  while (Date.now() < deadlineMs) {
    try {
      const res = await fetch(GATEWAY_HEALTH_URL, {
        method: 'GET',
        headers: { Accept: 'application/json' },
        signal: AbortSignal.timeout(2_000),
      });
      if (res.ok) {
        const body = await res.json().catch(() => ({}));
        ticks++;
        const connected = body.servers_connected ?? '?';
        console.log(
          `[dev-admin] Gateway health tick ${ticks}/${GATEWAY_STABLE_TICKS} — servers_connected=${connected}`,
        );
        if (ticks >= GATEWAY_STABLE_TICKS) return true;
        await sleep(GATEWAY_STABLE_INTERVAL_MS);
        continue;
      }
    } catch {
      // not up yet
    }
    ticks = 0;
    await sleep(POLL_MS);
  }
  return false;
}

/**
 * Open a URL in the system browser (macOS/Linux/Windows best-effort).
 * @param {string} url
 */
function openBrowser(url) {
  if (process.platform === 'darwin') {
    spawnSync('open', [url], { stdio: 'ignore' });
    return;
  }
  if (process.platform === 'win32') {
    spawnSync('cmd', ['/c', 'start', '', url], { stdio: 'ignore', shell: true });
    return;
  }
  spawnSync('xdg-open', [url], { stdio: 'ignore' });
}

async function waitThenOpenBrowser() {
  const deadline = Date.now() + OPEN_WAIT_MS;

  // Step 1: wait for admin API
  while (Date.now() < deadline) {
    if (await adminHealthOk()) break;
    await sleep(POLL_MS);
  }
  if (Date.now() >= deadline) {
    console.warn(`[dev-admin] Timed out waiting for ${HEALTH_URL}; open ${VITE_URL} manually when ready.`);
    return;
  }

  // Step 2: wait for gateway to be stable (not mid-restart)
  console.log(`[dev-admin] Admin API ready — waiting for stable gateway on :${GATEWAY_PORT}…`);
  const gatewayStable = await waitForStableGateway(deadline);
  if (!gatewayStable) {
    console.warn(
      `[dev-admin] Gateway did not stabilise before timeout; open ${VITE_URL} manually. Reload MCP in Cursor once :${GATEWAY_PORT} is up.`,
    );
    return;
  }

  console.log(`[dev-admin] Gateway stable — opening ${VITE_URL} (HMR + /api proxy).`);
  console.log(`[dev-admin] Production-parity UI: http://127.0.0.1:${ADMIN_PORT}/ after pnpm build:web:admin`);
  console.log(`[dev-admin] Reminder: reload MCP in Cursor (Settings → MCP) if tools are stale.`);
  openBrowser(VITE_URL);
}

async function main() {
  if (!existsSync(path.join(REPO_ROOT, 'package.json'))) {
    console.error('[dev-admin] Could not locate repo root.');
    process.exit(1);
  }

  loadRepoDotEnv(REPO_ROOT);

  void waitThenOpenBrowser();

  const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
  const result = spawnSync(pnpm, ['dev'], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
    env: {
      ...process.env,
      MCPMUX_DEV_ADMIN: '1',
      MCPMUX_DEV_PREP_DONE: '1',
      VITE_ADMIN_WEB: 'true',
    },
    shell: process.platform === 'win32',
  });
  process.exit(result.status ?? 0);
}

main();
