/**
 * Links + helpers for "Contribute / Request / Report" CTAs scattered across
 * the app (registry empty-state, settings, etc.).
 *
 * All URLs live here so we can update the target org / repo / site from one
 * place instead of grepping for hardcoded strings.
 *
 * Open-in-browser goes through `openUrl` (our Tauri command wrapping
 * `tauri-plugin-opener`) so the user's default browser handles the URL
 * rather than loading it inside the webview.
 */

import { openUrl } from '@/lib/api/gateway';

export const CONTRIBUTE = {
  /** Main desktop + gateway repo. */
  repo: 'https://github.com/mcpmux/mcp-mux',
  /** Community-maintained server-definition registry. */
  serversRepo: 'https://github.com/mcpmux/mcp-servers',
  /** Marketing site. */
  site: 'https://mcpmux.com',
  /** New bug report, pre-labelled. */
  bug: 'https://github.com/mcpmux/mcp-mux/issues/new?labels=bug',
  /** Feature request for the app itself. */
  featureRequest:
    'https://github.com/mcpmux/mcp-mux/issues/new?labels=enhancement',
  /**
   * Request a new server definition in the community registry. Encodes the
   * user's search term into the issue title when provided.
   */
  requestServer(searchTerm?: string): string {
    const base =
      'https://github.com/mcpmux/mcp-servers/issues/new?labels=server-request';
    if (!searchTerm) return base;
    const title = encodeURIComponent(`Request: ${searchTerm.slice(0, 120)}`);
    return `${base}&title=${title}`;
  },
  /** Root of the server-definitions contributing guide. */
  contributeServer: 'https://github.com/mcpmux/mcp-servers/blob/main/CONTRIBUTING.md',
} as const;

/**
 * Open an external URL via the Tauri opener plugin. Falls back to the plugin
 * directly if our gateway wrapper fails (mirrors OAuthConsentModal's pattern).
 */
export async function openExternal(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch {
    const { openUrl: plugin } = await import('@tauri-apps/plugin-opener');
    await plugin(url);
  }
}
