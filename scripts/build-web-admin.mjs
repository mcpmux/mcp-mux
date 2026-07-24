#!/usr/bin/env node
/**
 * Production web-admin SPA build with VITE_ADMIN_WEB set for the full pipeline.
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DESKTOP_DIR = path.join(REPO_ROOT, 'apps', 'desktop');
const env = { ...process.env, VITE_ADMIN_WEB: 'true' };
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';

/**
 * Run a pnpm command in the desktop package and exit on failure.
 * @param {string[]} args
 */
function runDesktop(args) {
  const result = spawnSync(pnpm, args, {
    cwd: DESKTOP_DIR,
    stdio: 'inherit',
    env,
    shell: process.platform === 'win32',
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

runDesktop(['exec', 'tsc']);
runDesktop(['exec', 'vite', 'build']);
