import type { TFunction } from 'i18next';
import type { UpdatePolicy } from '@/lib/api/settings';

/**
 * Per-server update policy labels for Configure and Settings.
 */
export function getUpdatePolicyOptions(t: TFunction<'servers'>): {
  value: UpdatePolicy;
  label: string;
  description: string;
}[] {
  return [
    {
      value: 'notify',
      label: t('updatePolicy.notify.label'),
      description: t('updatePolicy.notify.description'),
    },
    {
      value: 'auto',
      label: t('updatePolicy.auto.label'),
      description: t('updatePolicy.auto.description'),
    },
    {
      value: 'pinned',
      label: t('updatePolicy.pinned.label'),
      description: t('updatePolicy.pinned.description'),
    },
  ];
}

/** Basic semver pattern (major.minor.patch with optional pre-release/build). */
const BASIC_SEMVER_PATTERN =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

const FLOATING_NPM_TAGS = new Set([
  'latest',
  '*',
  'next',
  'beta',
  'canary',
  'stable',
  'release',
]);

/**
 * Returns true for npm dist-tags that do not pin an exact semver.
 */
export function npmVersionTagIsFloating(tag: string): boolean {
  return FLOATING_NPM_TAGS.has(tag.trim().replace(/^@/, '').toLowerCase());
}

/**
 * Returns true when `version` matches a basic semver shape.
 */
export function isValidSemver(version: string): boolean {
  return BASIC_SEMVER_PATTERN.test(version.trim());
}

/**
 * Returns true when the stdio transport uses npx or uvx/uv (package-managed).
 */
export function isPackageManagedTransport(command: string | undefined): boolean {
  if (!command) {
    return false;
  }
  return command === 'npx' || command === 'uvx' || command === 'uv';
}

/**
 * Single UI guard mirroring Rust `probe_update_available` plus pinned/auto exclusion.
 */
export function shouldShowPackageUpdate(input: {
  updatePolicy: UpdatePolicy;
  latestVersion: string | null | undefined;
  currentVersion: string | null | undefined;
  transportCommand?: string;
  transportArgs?: string[];
}): boolean {
  if (input.updatePolicy === 'pinned' || input.updatePolicy === 'auto') {
    return false;
  }

  if (!input.latestVersion) {
    return false;
  }

  if (packageUsesFloatingNpmTag(input.transportCommand, input.transportArgs)) {
    return false;
  }

  if (!input.currentVersion) {
    return false;
  }

  return isNewerVersion(input.latestVersion, input.currentVersion);
}

/**
 * Returns true when the npx package arg already tracks a floating dist-tag like `@latest`.
 */
function packageUsesFloatingNpmTag(
  transportCommand: string | undefined,
  transportArgs: string[] | undefined
): boolean {
  if (transportCommand !== 'npx' || !transportArgs) {
    return false;
  }
  const packageArg = findNpxPackageArg(transportArgs);
  if (!packageArg) {
    return false;
  }
  const version = splitNpmPackageArg(packageArg)[1];
  return version != null && npmVersionTagIsFloating(version);
}

/**
 * Parse a semver-ish version string into numeric segments for comparison.
 */
function parseVersionParts(version: string): number[] {
  return version
    .trim()
    .replace(/^v/, '')
    .replace(/^=/, '')
    .split(/[^0-9]+/)
    .filter(Boolean)
    .map((part) => Number.parseInt(part, 10))
    .filter((part) => !Number.isNaN(part));
}

/**
 * Returns true when `latest` is strictly newer than `current`.
 */
function isNewerVersion(latest: string, current: string): boolean {
  const latestParts = parseVersionParts(latest);
  const currentParts = parseVersionParts(current);
  const maxLen = Math.max(latestParts.length, currentParts.length);

  for (let index = 0; index < maxLen; index += 1) {
    const latestPart = latestParts[index] ?? 0;
    const currentPart = currentParts[index] ?? 0;
    if (latestPart > currentPart) {
      return true;
    }
    if (latestPart < currentPart) {
      return false;
    }
  }

  return latest !== current;
}

/**
 * Derive the effective current version for update badge display.
 *
 * Precedence:
 *  1. `pinnedVersion` — explicit user pin (`UpdatePolicy::Pinned`)
 *  2. `installedVersion` — actual installed version written by the backend probe
 *     (`current_version` DB column, populated from npx cache / `uv tool list`).
 *     This takes precedence over args so that post-update the badge clears even
 *     when the args still carry the pre-update semver.
 *  3. `argVersion` — semver baked into transport args (`@semver` / `==semver`),
 *     used as a cold-cache fallback before the server has ever been probed.
 */
export function resolveCurrentPackageVersion(input: {
  pinnedVersion?: string | null;
  transportCommand?: string;
  transportArgs?: string[];
  installedVersion?: string | null;
}): string | null {
  if (input.pinnedVersion) {
    return input.pinnedVersion;
  }

  if (input.installedVersion) {
    return input.installedVersion;
  }

  return resolveArgPackageVersion(input.transportCommand, input.transportArgs);
}

/**
 * Extract an exact semver baked into the npx/uvx package argument, if any.
 */
function resolveArgPackageVersion(
  transportCommand: string | undefined,
  transportArgs: string[] | undefined
): string | null {
  if (transportCommand === 'npx' && transportArgs) {
    const packageArg = findNpxPackageArg(transportArgs);
    if (!packageArg) {
      return null;
    }
    const version = splitNpmPackageArg(packageArg)[1];
    if (!version || npmVersionTagIsFloating(version) || !isValidSemver(version)) {
      return null;
    }
    return version;
  }

  if ((transportCommand === 'uvx' || transportCommand === 'uv') && transportArgs) {
    const packageArg = findUvxPackageArg(transportCommand, transportArgs);
    if (!packageArg) {
      return null;
    }
    const eqIndex = packageArg.indexOf('==');
    if (eqIndex >= 0) {
      const version = packageArg.slice(eqIndex + 2) || null;
      if (!version || !isValidSemver(version)) {
        return null;
      }
      return version;
    }
  }

  return null;
}

/**
 * Split an npm package arg into name and optional version tag.
 */
function splitNpmPackageArg(packageArg: string): [string, string | null] {
  if (packageArg.startsWith('@') && packageArg.indexOf('@', 1) > 0) {
    const scopedSplit = packageArg.indexOf('@', 1);
    return [packageArg.slice(0, scopedSplit), packageArg.slice(scopedSplit + 1) || null];
  }
  const atIndex = packageArg.lastIndexOf('@');
  if (atIndex > 0) {
    return [packageArg.slice(0, atIndex), packageArg.slice(atIndex + 1) || null];
  }
  return [packageArg, null];
}

/**
 * Locate the npm package argument after `-y` / `--yes`.
 */
function findNpxPackageArg(args: string[]): string | undefined {
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if ((arg === '-y' || arg === '--yes') && index + 1 < args.length && !args[index + 1].startsWith('-')) {
      return args[index + 1];
    }
  }
  return args.find((arg) => !arg.startsWith('-') && arg !== '--');
}

/**
 * Locate the first positional package arg for uvx / uv run.
 */
function findUvxPackageArg(command: string, args: string[]): string | undefined {
  if (command === 'uvx') {
    return args.find((arg) => !arg.startsWith('-'));
  }
  if (command === 'uv' && args[0] === 'run') {
    for (let index = 1; index < args.length; index += 1) {
      const arg = args[index];
      if (arg.startsWith('-')) {
        if (arg === '-m' || arg === '--module') {
          index += 1;
        }
        continue;
      }
      return arg;
    }
  }
  return undefined;
}
