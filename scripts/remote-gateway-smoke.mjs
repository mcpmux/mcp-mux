#!/usr/bin/env node
/**
 * Remote gateway smoke — health + OAuth metadata over a Cloudflare Tunnel hostname.
 *
 * Prereqs:
 *   cp .env.example .env   # fill MCPMUX_CF_ACCESS_* and MCPMUX_REMOTE_GATEWAY_URL
 *   cloudflared tunnel running → localhost:45818
 *
 * Usage:
 *   pnpm remote:smoke
 *   node scripts/remote-gateway-smoke.mjs
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { cfAccessCurlFlagsFromEnv, loadRepoDotEnv } from './cf-access-env.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/**
 * Run curl and return stdout.
 * @param {string[]} args
 */
function curl(args) {
  const result = spawnSync('curl', args, { encoding: 'utf8' });
  if (result.status !== 0) {
    console.error(result.stderr || result.stdout);
    process.exit(result.status ?? 1);
  }
  return result.stdout.trim();
}

/**
 * @param {string} url
 */
function checkHealth(url) {
  const code = curl([
    ...cfAccessCurlFlagsFromEnv(),
    '-s',
    '-o',
    '/dev/null',
    '-w',
    '%{http_code}',
    `${url}/health`,
  ]);
  console.log(`GET ${url}/health => HTTP ${code}`);
  if (code !== '200') {
    process.exit(1);
  }
}

/**
 * @param {string} url
 */
function checkOAuthMetadata(url) {
  const body = curl([
    ...cfAccessCurlFlagsFromEnv(),
    '-s',
    `${url}/.well-known/oauth-protected-resource`,
  ]);
  const parsed = JSON.parse(body);
  console.log('Protected resource metadata:', parsed.resource);
}

function main() {
  loadRepoDotEnv(REPO_ROOT);

  const gatewayUrl = process.env.MCPMUX_REMOTE_GATEWAY_URL?.replace(/\/$/, '');
  if (!gatewayUrl) {
    console.error('Set MCPMUX_REMOTE_GATEWAY_URL in .env (see .env.example)');
    process.exit(1);
  }

  const cfHeaders = cfAccessCurlFlagsFromEnv();
  if (cfHeaders.length === 0) {
    console.error('Set MCPMUX_CF_ACCESS_CLIENT_ID and MCPMUX_CF_ACCESS_CLIENT_SECRET in .env');
    process.exit(1);
  }

  checkHealth(gatewayUrl);
  checkOAuthMetadata(gatewayUrl);

  const adminUrl = process.env.MCPMUX_REMOTE_ADMIN_URL?.replace(/\/$/, '');
  if (adminUrl) {
    const code = curl([
      ...cfAccessCurlFlagsFromEnv(),
      '-s',
      '-o',
      '/dev/null',
      '-w',
      '%{http_code}',
      `${adminUrl}/api/v1/health`,
    ]);
    console.log(`GET ${adminUrl}/api/v1/health => HTTP ${code}`);
    if (code === '302') {
      console.error(
        'Admin returned 302 — add the service token to the mux Access application policy in Cloudflare Zero Trust.',
      );
      process.exit(1);
    }
    if (code !== '200') {
      process.exit(1);
    }
  }
}

main();
