#!/usr/bin/env node
/**
 * Run a command with repo-root `.env` merged into the environment (when present).
 *
 * Usage: node scripts/run-with-repo-env.mjs tauri dev
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { loadRepoDotEnv } from './cf-access-env.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
loadRepoDotEnv(REPO_ROOT);

const [, , ...cmd] = process.argv;
if (cmd.length === 0) {
  console.error('Usage: node scripts/run-with-repo-env.mjs <command> [args...]');
  process.exit(1);
}

const result = spawnSync(cmd[0], cmd.slice(1), {
  stdio: 'inherit',
  env: process.env,
  shell: process.platform === 'win32',
});

process.exit(result.status ?? 1);
