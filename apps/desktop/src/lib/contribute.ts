/**
 * Links + helpers for "Contribute / Request / Report" CTAs scattered across
 * the app (registry empty-state, settings, etc.).
 *
 * All URLs live here so we can update the target org / repo / site from one
 * place instead of grepping for hardcoded strings.
 *
 * Open-in-browser goes through `backend.shell.openExternal` so the user's
 * default browser handles the URL rather than loading it inside the webview.
 */

import { openExternal as shellOpenExternal } from '@/lib/backend/shell';

export const CONTRIBUTE = {
  /** Main desktop + gateway repo. */
  repo: 'https://github.com/mcpmux/mcp-mux',
  /** Community-maintained server-definition registry. */
  serversRepo: 'https://github.com/mcpmux/mcp-servers',
  /** Marketing site. */
  site: 'https://mcpmux.com',
  /** New bug report, pre-filled with the bug_report template. */
  bug: 'https://github.com/mcpmux/mcp-mux/issues/new?template=bug_report.yml',
  /** Feature request for the app itself, pre-filled with the feature_request template. */
  featureRequest:
    'https://github.com/mcpmux/mcp-mux/issues/new?template=feature_request.yml',
  /**
   * Request a new server definition in the community registry. Opens the
   * `request-server.yml` issue template and encodes the user's search term
   * into the title when provided.
   */
  requestServer(searchTerm?: string): string {
    const base =
      'https://github.com/mcpmux/mcp-servers/issues/new?template=request-server.yml';
    if (!searchTerm) return base;
    const title = encodeURIComponent(`[Request] ${searchTerm.slice(0, 120)}`);
    return `${base}&title=${title}`;
  },
  /**
   * Contribute a new server definition — points at the registry's
   * CONTRIBUTING guide. Server definitions are JSON files landed via PR,
   * not issues, so we send users straight down the fork → PR path.
   */
  contributeServer: 'https://github.com/mcpmux/mcp-servers/blob/main/CONTRIBUTING.md',
  /** Report a bug in an existing server definition. */
  serverDefinitionBug:
    'https://github.com/mcpmux/mcp-servers/issues/new?template=bug-report.yml',
} as const;

/**
 * Open an external URL via the desktop shell opener (no-op fallback on web).
 */
export async function openExternal(url: string): Promise<void> {
  await shellOpenExternal(url);
}
