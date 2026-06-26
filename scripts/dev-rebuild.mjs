#!/usr/bin/env node
/**
 * Force a debug rebuild of the whole Rust workspace (gateway, core, storage,
 * mcp, and the `mcpmux` app crate) without launching the app.
 *
 * `tauri dev` already auto-rebuilds + restarts on Rust changes, so this is only
 * for recovery: after a stale binary or failed relaunch, run `pnpm dev:stop &&
 * pnpm dev:rebuild && pnpm dev:admin` to guarantee a fresh binary before start.
 *
 * Usage (repo root): pnpm dev:rebuild
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function main() {
  console.log('[dev-rebuild] cargo build --workspace (debug) …');
  const start = Date.now();

  const result = spawnSync('cargo', ['build', '--workspace'], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });

  const elapsed = ((Date.now() - start) / 1000).toFixed(1);
  if (result.status === 0) {
    console.log(`[dev-rebuild] Done in ${elapsed}s — binary is fresh.`);
  } else {
    console.error(`[dev-rebuild] cargo build failed after ${elapsed}s.`);
  }
  process.exit(result.status ?? 1);
}

main();
