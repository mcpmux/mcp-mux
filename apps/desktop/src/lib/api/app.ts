/** @deprecated Prefer `@/lib/backend` — shim during facade migration. */
import { apiCall } from './transport';

/**
 * Read the running application version from the Rust backend.
 */
export async function getVersion(): Promise<string> {
  return apiCall('get_version');
}

/**
 * Read the on-disk bundle version when it differs from the running process
 * (e.g. after a Homebrew Cask upgrade). Returns null on non-macOS platforms.
 */
export async function getBundleVersion(): Promise<string | null> {
  return apiCall('get_bundle_version');
}

/** Git/build metadata the running backend was compiled from. */
export interface BuildInfo {
  git_sha: string;
  git_branch: string;
  commit_time: string;
  build_time: string;
}

/**
 * Read build metadata from the Rust backend (git SHA stamped at compile time).
 */
export async function getBuildInfo(): Promise<BuildInfo> {
  return apiCall('get_build_info');
}
