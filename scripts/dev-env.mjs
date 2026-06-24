#!/usr/bin/env node
/**
 * Dev environment checks for web admin HMR (admin API liveness on loopback).
 *
 * Usage (repo root):
 *   node scripts/dev-env.mjs prep   — fail fast if admin API is not reachable
 */

import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ADMIN_PORT = Number.parseInt(process.env.MCPMUX_ADMIN_PORT ?? '45819', 10);
const HEALTH_URL = `http://127.0.0.1:${ADMIN_PORT}/api/v1/health`;

/**
 * @returns {Promise<boolean>}
 */
async function adminHealthOk() {
  try {
    const response = await fetch(HEALTH_URL, {
      method: 'GET',
      headers: { Accept: 'application/json' },
      signal: AbortSignal.timeout(3_000),
    });
    return response.ok;
  } catch {
    return false;
  }
}

/**
 * Verify the admin HTTP API is listening before Vite proxies /api.
 * @returns {Promise<number>} exit code
 */
async function runPrep() {
  if (!existsSync(path.join(REPO_ROOT, 'package.json'))) {
    console.error('[dev-env] Could not locate repo root.');
    return 1;
  }

  if (await adminHealthOk()) {
    console.log(`[dev-env] Admin API ready at ${HEALTH_URL}`);
    return 0;
  }

  console.error('[dev-env] Admin API is not running.');
  console.error(`  Expected health check at ${HEALTH_URL}`);
  console.error('  Start the gateway first: `pnpm dev`, `pnpm dev:admin`, or enable Web admin in Settings → Gateway.');
  console.error('  Or run `pnpm dev:web:admin` from the repo root to auto-start the backend.');
  return 1;
}

/**
 * @param {string[]} argv
 * @returns {Promise<number>}
 */
async function main(argv) {
  const subcommand = argv[0] ?? 'prep';

  if (subcommand === 'prep') {
    return runPrep();
  }

  console.error(`[dev-env] Unknown subcommand: ${subcommand}`);
  console.error('  Usage: node scripts/dev-env.mjs prep');
  return 1;
}

const exitCode = await main(process.argv.slice(2));
process.exit(exitCode);
