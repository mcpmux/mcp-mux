#!/usr/bin/env node
/**
 * Playwright webServer fixture: McpMux dev with web admin on :45819.
 *
 * Tunnel parity: loads repo `.env`, passes `MCPMUX_CF_ACCESS_*` into `pnpm dev`, and
 * probes admin with the same CF headers Playwright uses (service token or JWT).
 *
 * Env: MCPMUX_DEV_ADMIN=1, MCPMUX_ADMIN_TEST=1 (SSE/oauth publish helpers for admin specs).
 * Linux CI: dbus + gnome-keyring unlock (same pattern as e2e-desktop).
 */

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  adminCfProbeHeaders,
  hasAdminCfProbeAuth,
  loadRepoDotEnv,
} from './cf-access-env.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ADMIN_PORT = Number.parseInt(process.env.MCPMUX_ADMIN_PORT ?? '45819', 10);
const HEALTH_URL = `http://127.0.0.1:${ADMIN_PORT}/api/v1/health`;
const READY_URL = `http://127.0.0.1:${ADMIN_PORT}/`;
const WAIT_MS = Number.parseInt(process.env.MCPMUX_ADMIN_E2E_WAIT_MS ?? '300000', 10);
const POLL_MS = 500;
const DIST_INDEX = path.join(REPO_ROOT, 'apps', 'desktop', 'dist', 'index.html');

loadRepoDotEnv(REPO_ROOT);

/**
 * @param {number} ms
 * @returns {Promise<void>}
 */
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * True when the admin HTTP server is listening without CF credentials (trust off).
 * @param {number} status
 * @returns {boolean}
 */
function isAdminHttpUpWithoutCf(status) {
  return status === 200 || status === 503;
}

/**
 * Probe admin; with CF auth env expects 200, without CF accepts trust-off signals.
 * @returns {Promise<boolean>}
 */
async function adminReady() {
  const headers = adminCfProbeHeaders();
  const useCfAuth = Object.keys(headers).length > 0;

  for (const url of [HEALTH_URL, READY_URL]) {
    try {
      const response = await fetch(url, {
        method: 'GET',
        headers,
        redirect: 'follow',
      });
      if (useCfAuth) {
        if (response.status === 200) {
          return true;
        }
      } else if (isAdminHttpUpWithoutCf(response.status)) {
        return true;
      }
    } catch {
      // try next probe
    }
  }
  return false;
}

/**
 * Ensure production admin SPA exists for :45819 static serving.
 */
function ensureAdminDistBuilt() {
  if (existsSync(DIST_INDEX)) {
    return;
  }
  console.log('[admin-e2e-fixture] Building web admin SPA (pnpm build:web:admin)…');
  const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
  const result = spawnSync(pnpm, ['build:web:admin'], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

/**
 * @param {import('node:child_process').ChildProcess | null} child
 */
function attachShutdown(child) {
  const stop = () => {
    if (child?.pid && !child.killed) {
      if (process.platform === 'win32') {
        spawnSync('taskkill', ['/pid', String(child.pid), '/t', '/f'], { stdio: 'ignore' });
      } else {
        child.kill('SIGTERM');
      }
    }
    process.exit(0);
  };
  process.on('SIGTERM', stop);
  process.on('SIGINT', stop);
}

/**
 * Start Tauri dev with admin enabled (and test helpers). Inherits `.env` CF vars.
 * @returns {import('node:child_process').ChildProcess}
 */
function startDevBackend() {
  const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
  const env = {
    ...process.env,
    MCPMUX_DEV_PREP_DONE: '1',
    MCPMUX_DEV_ADMIN: '1',
    MCPMUX_ADMIN_TEST: '1',
  };

  if (process.env.CI === 'true' && process.platform === 'linux') {
    // tauri dev opens a webkit2gtk window — needs a virtual display under headless CI
    // (same xvfb-run pattern as e2e-desktop.yml). Without it `pnpm dev` exits immediately.
    const inner = `echo "test" | gnome-keyring-daemon --unlock --components=secrets 2>/dev/null; eval "$(gnome-keyring-daemon --start --components=secrets 2>/dev/null)"; exec xvfb-run --auto-servernum ${pnpm} dev`;
    return spawn('dbus-run-session', ['--', 'bash', '-lc', inner], {
      cwd: REPO_ROOT,
      env,
      stdio: 'inherit',
    });
  }

  return spawn(pnpm, ['dev'], {
    cwd: REPO_ROOT,
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
}

/**
 * Block until admin responds or timeout.
 * @returns {Promise<boolean>}
 */
async function waitForAdmin() {
  const deadline = Date.now() + WAIT_MS;
  while (Date.now() < deadline) {
    if (await adminReady()) {
      return true;
    }
    await sleep(POLL_MS);
  }
  return adminReady();
}

/**
 * Log actionable hints when CF trust is on but probes never return 200.
 */
function logCfTrustHints() {
  if (hasAdminCfProbeAuth()) {
    console.error(
      '[admin-e2e-fixture] CF credentials are set but admin did not return 200.',
    );
    console.error(
      '  Restart McpMux after saving .env so the process has MCPMUX_CF_ACCESS_* set.',
    );
    return;
  }
  console.error(
    '[admin-e2e-fixture] Admin returned 401 (CF Access trust likely on).',
  );
  console.error(
    '  Add MCPMUX_CF_ACCESS_CLIENT_ID and MCPMUX_CF_ACCESS_CLIENT_SECRET to repo .env',
  );
  console.error('  (or MCPMUX_ADMIN_CF_JWT), then restart McpMux and re-run tests.');
}

async function main() {
  if (!existsSync(path.join(REPO_ROOT, 'package.json'))) {
    console.error('[admin-e2e-fixture] Could not locate repo root.');
    process.exit(1);
  }

  ensureAdminDistBuilt();

  if (await adminReady()) {
    console.log(`[admin-e2e-fixture] Reusing admin API at :${ADMIN_PORT}`);
    attachShutdown(null);
    await new Promise(() => {});
    return;
  }

  console.log(`[admin-e2e-fixture] Starting McpMux dev (admin :${ADMIN_PORT})…`);
  const child = startDevBackend();
  attachShutdown(child);

  child.on('exit', (code, signal) => {
    if (signal) {
      process.exit(0);
    }
    console.error(`[admin-e2e-fixture] pnpm dev exited (code=${code ?? 'null'})`);
    process.exit(code ?? 1);
  });

  const ready = await waitForAdmin();
  if (!ready) {
    console.error(`[admin-e2e-fixture] Admin API did not become ready on :${ADMIN_PORT}`);
    logCfTrustHints();
    process.exit(1);
  }

  console.log(`[admin-e2e-fixture] Ready on :${ADMIN_PORT}`);
  await new Promise(() => {});
}

main();
