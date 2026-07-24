#!/usr/bin/env node
/**
 * Git + build metadata for web-admin SPA builds (mirrors gateway `build.rs` fields).
 */

import { execSync } from 'node:child_process';

import { formatBuiltAt, formatStampInstant } from './build-date.helpers.mjs';

/**
 * Run a git command and return trimmed stdout, or `fallback` on failure.
 * @param {string[]} args
 * @param {string} [fallback]
 * @returns {string}
 */
function git(args, fallback = '') {
  try {
    return execSync(['git', ...args].join(' '), { encoding: 'utf8' }).trim();
  } catch {
    return fallback;
  }
}

/**
 * Resolve build instant honoring SOURCE_DATE_EPOCH when set.
 * @returns {Date}
 */
function getBuildInstant() {
  const raw = process.env.SOURCE_DATE_EPOCH?.trim();
  if (raw) {
    const parsed = Number.parseInt(raw, 10);
    if (!Number.isNaN(parsed)) {
      return new Date(parsed * 1000);
    }
  }
  return new Date();
}

/**
 * Collect git/build metadata for stamping Vite bundles and build-stamp.json.
 * @returns {{
 *   gitSha: string,
 *   gitBranch: string,
 *   commitTime: string,
 *   commitAt: string,
 *   buildTime: string,
 *   buildAt: string,
 * }}
 */
export function getBuildStamp() {
  const commitTime = git(['log', '-1', '--format=%ci'], 'unknown');
  const buildInstant = getBuildInstant();
  const buildTime = `${buildInstant.toISOString().replace('T', ' ').replace(/\.\d{3}Z$/, ' UTC')}`;

  return {
    gitSha: git(['rev-parse', '--short', 'HEAD'], 'unknown'),
    gitBranch: git(['rev-parse', '--abbrev-ref', 'HEAD'], 'unknown'),
    commitTime,
    commitAt: formatStampInstant(commitTime),
    buildTime,
    buildAt: formatBuiltAt(buildInstant),
  };
}

/**
 * JSON shape written to `apps/desktop/dist/build-stamp.json`.
 * @param {ReturnType<typeof getBuildStamp>} stamp
 * @returns {Record<string, string>}
 */
export function buildStampJson(stamp) {
  return {
    git_sha: stamp.gitSha,
    git_branch: stamp.gitBranch,
    commit_time: stamp.commitTime,
    commit_at: stamp.commitAt,
    build_time: stamp.buildTime,
    build_at: stamp.buildAt,
  };
}

/**
 * Format a gateway-style build line for console output.
 * @param {string} prefix
 * @param {ReturnType<typeof getBuildStamp>} stamp
 * @returns {string}
 */
export function formatBuildStampLine(prefix, stamp) {
  return `${prefix} | sha: ${stamp.gitSha} | branch: ${stamp.gitBranch} | committed: ${stamp.commitAt} | built: ${stamp.buildAt}`;
}
