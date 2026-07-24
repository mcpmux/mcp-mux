import { formatStampInstant } from '@/utils/build-date.helpers';
import { getBuildInfo, getVersion, type BuildInfo } from '@/lib/api/app';
import { isTauri } from '@/lib/backend/shell';

/** A labeled row for build stamp display in Settings UI. */
export interface BuildStampRow {
  label: string;
  value: string;
  mono?: boolean;
  testId: string;
}

/** Git/build metadata stamped into the web-admin SPA at Vite build time. */
export interface BuildStamp {
  gitSha: string;
  gitBranch: string;
  commitTime: string;
  commitAt: string;
  buildTime: string;
  buildAt: string;
}

const LABEL_STYLE = 'color: #888; font-weight: bold';
const VALUE_STYLE = 'color: inherit';

/**
 * Read SPA build metadata from Vite compile-time env vars.
 */
export function getSpaBuildStamp(): BuildStamp {
  const commitTime = import.meta.env.VITE_BUILD_COMMIT_TIME ?? '';
  const buildTime = import.meta.env.VITE_BUILD_TIME ?? '';
  return {
    gitSha: import.meta.env.VITE_BUILD_GIT_SHA ?? '',
    gitBranch: import.meta.env.VITE_BUILD_GIT_BRANCH ?? '',
    commitTime,
    commitAt: import.meta.env.VITE_BUILD_COMMIT_AT || formatStampInstant(commitTime),
    buildTime,
    buildAt: import.meta.env.VITE_BUILD_AT || formatStampInstant(buildTime),
  };
}

/**
 * Map SPA compile-time stamp fields to labeled display rows.
 */
export function buildStampDisplayRows(stamp: BuildStamp): BuildStampRow[] {
  return [
    { label: 'Branch', value: stamp.gitBranch || 'unknown', mono: true, testId: 'build-stamp-branch' },
    { label: 'Commit', value: stamp.gitSha || 'unknown', mono: true, testId: 'build-stamp-commit' },
    { label: 'Committed', value: stamp.commitAt || 'unknown', testId: 'build-stamp-committed' },
    { label: 'Built', value: stamp.buildAt || 'unknown', testId: 'build-stamp-built' },
  ];
}

/**
 * Map backend compile-time build info to labeled display rows.
 */
export function backendBuildInfoRows(info: BuildInfo): BuildStampRow[] {
  return [
    { label: 'Branch', value: info.git_branch || 'unknown', mono: true, testId: 'build-stamp-branch' },
    { label: 'Commit', value: info.git_sha || 'unknown', mono: true, testId: 'build-stamp-commit' },
    {
      label: 'Committed',
      value: formatStampInstant(info.commit_time),
      testId: 'build-stamp-committed',
    },
    {
      label: 'Built',
      value: formatStampInstant(info.build_time),
      testId: 'build-stamp-built',
    },
  ];
}

/**
 * Format a gateway-style build line for Node/build logs.
 */
export function formatBuildStampLine(prefix: string, stamp: BuildStamp): string {
  return `${prefix} | sha: ${stamp.gitSha} | branch: ${stamp.gitBranch} | committed: ${stamp.commitAt} | built: ${stamp.buildAt}`;
}

/**
 * Log a generAIt-style labeled row in the browser console.
 */
function logConsoleRow(label: string, value: string): void {
  const pad = Math.max(1, 13 - label.length);
  console.info(`%c${label}:%c${' '.repeat(pad)}${value}`, LABEL_STYLE, VALUE_STYLE);
}

/**
 * Log SPA and backend build metadata to the browser console on every boot
 * (dev, production Tauri, and web-admin static bundle).
 * Visual style matches generAIt Frontend startup banner (group + gray labels).
 */
export async function logWebAdminBuildInfo(): Promise<void> {
  const headerColor = import.meta.env.DEV ? '#70e000' : '#DA7756';
  const appLabel = import.meta.env.VITE_ADMIN_WEB ? 'McpMux Web Admin' : 'McpMux';
  console.group(
    `%c ${appLabel} `,
    `background: ${headerColor}; color: #000; font-weight: bold; border-radius: 4px; padding: 2px 6px;`,
  );

  logConsoleRow('Transport', isTauri() ? 'tauri' : 'admin-http');
  logConsoleRow('Host', window.location.hostname);
  logConsoleRow('Mode', import.meta.env.DEV ? 'development' : 'production');

  const spa = getSpaBuildStamp();

  try {
    const [backend, version] = await Promise.all([getBuildInfo(), getVersion()]);
    logConsoleRow('Version', version);

    if (spa.gitSha) {
      logConsoleRow('Branch', spa.gitBranch);
      logConsoleRow('Commit', spa.gitSha);
      logConsoleRow('Committed', spa.commitAt);
      logConsoleRow('Built', spa.buildAt);
    }

    if (backend.git_sha && backend.git_sha !== spa.gitSha) {
      logConsoleRow('Backend', formatStampInstant(backend.build_time));
      logConsoleRow('Backend sha', backend.git_sha);
    } else if (backend.build_time && backend.build_time !== spa.buildTime) {
      logConsoleRow('Backend', formatStampInstant(backend.build_time));
    }

    if (spa.gitSha && backend.git_sha && spa.gitSha !== backend.git_sha) {
      console.warn(
        `%cStale:%c       SPA (${spa.gitSha}) != backend (${backend.git_sha}) — run pnpm build:web:admin`,
        LABEL_STYLE,
        VALUE_STYLE,
      );
    }
  } catch {
    console.warn('%cBackend:%c    build info unavailable', LABEL_STYLE, VALUE_STYLE);
  }

  console.groupEnd();
}
