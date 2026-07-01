#!/usr/bin/env node
/**
 * Web admin UI dev entry — prep ports, ensure Tauri backend is up, run Vite with
 * VITE_ADMIN_WEB so the browser uses the admin HTTP transport (proxied /api → :45819).
 *
 * Usage (repo root): pnpm dev:web:admin
 * Optional: MCPMUX_DEV_ADMIN=1 is set when spawning the backend (see pnpm dev:admin).
 */

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { adminCfProbeHeaders, loadRepoDotEnv } from './cf-access-env.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ADMIN_PORT = Number.parseInt(process.env.MCPMUX_ADMIN_PORT ?? '45819', 10);
const HEALTH_URL = `http://127.0.0.1:${ADMIN_PORT}/api/v1/health`;
const BACKEND_WAIT_MS = 120_000;
const POLL_MS = 500;

/**
 * @param {number} ms
 * @returns {Promise<void>}
 */
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Fail fast when the admin API is not reachable (after any backend auto-start).
 */
function runPrep() {
  const node = process.execPath;
  const result = spawnSync(node, [path.join(REPO_ROOT, 'scripts/dev-env.mjs'), 'prep'], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

/**
 * @returns {Promise<boolean>}
 */
async function adminHealthOk() {
  try {
    const response = await fetch(HEALTH_URL, {
      method: 'GET',
      headers: { Accept: 'application/json', ...adminCfProbeHeaders() },
    });
    return response.ok;
  } catch {
    return false;
  }
}

/**
 * Start `pnpm dev` in the background when the admin API is not up yet.
 */
function startBackendDetached() {
  const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
  const env = {
    ...process.env,
    MCPMUX_DEV_PREP_DONE: '1',
    MCPMUX_DEV_ADMIN: '1',
    VITE_ADMIN_WEB: 'true',
  };
  const child = spawn(pnpm, ['dev'], {
    cwd: REPO_ROOT,
    detached: true,
    stdio: 'ignore',
    env,
    shell: process.platform === 'win32',
  });
  child.unref();
  console.log('[dev-web-admin] Started `pnpm dev` in the background (Tauri + gateway + admin when enabled).');
}

/**
 * Block until admin /health responds or timeout.
 * @returns {Promise<boolean>}
 */
async function waitForAdmin() {
  const deadline = Date.now() + BACKEND_WAIT_MS;
  while (Date.now() < deadline) {
    if (await adminHealthOk()) {
      return true;
    }
    await sleep(POLL_MS);
  }
  return adminHealthOk();
}

/**
 * Run Vite with admin web build flags (HMR on :1420, /api proxy → admin port).
 */
function runVite() {
  const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
  const result = spawnSync(
    pnpm,
    ['--filter', '@mcpmux/desktop', 'dev:web:admin'],
    {
      cwd: REPO_ROOT,
      stdio: 'inherit',
      env: process.env,
      shell: process.platform === 'win32',
    },
  );
  process.exit(result.status ?? 0);
}

async function main() {
  if (!existsSync(path.join(REPO_ROOT, 'package.json'))) {
    console.error('[dev-web-admin] Could not locate repo root.');
    process.exit(1);
  }

  loadRepoDotEnv(REPO_ROOT);

  if (!(await adminHealthOk())) {
    startBackendDetached();
    console.log(`[dev-web-admin] Waiting for admin API at ${HEALTH_URL} …`);
    const ready = await waitForAdmin();
    if (!ready) {
      console.error('[dev-web-admin] Admin API did not become ready in time.');
      console.error(
        '  Enable **Web admin** in McpMux Settings → Gateway, or run `pnpm dev:admin` (auto-enables admin in dev).',
      );
      console.error(`  Then open http://127.0.0.1:1420 for HMR, or http://127.0.0.1:${ADMIN_PORT} after pnpm build:web:admin.`);
      process.exit(1);
    }
  }

  runPrep();

  console.log('[dev-web-admin] Admin API ready. Starting Vite (http://127.0.0.1:1420, /api → admin).');
  runVite();
}

main();
