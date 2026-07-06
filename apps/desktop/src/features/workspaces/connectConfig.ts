/**
 * Shared MCP-client config generation for the "how a client connects" surfaces
 * (the setup wizard preview + the mapping inspector copy panels). Centralising
 * the JSON shape here keeps every copy button in sync as the routing model
 * evolves, instead of each surface re-deriving the snippet.
 *
 * Routing model the snippets express:
 *   • A Bearer API key identifies a *client* — mcpmux routes that client to its
 *     auto-created client mapping automatically (even with inbound auth off).
 *   • The `X-Mcpmux-Workspace` header is an explicit override — its value
 *     matches a binding's identifier (a folder path, normalized; or an exact id
 *     string).
 */

/** Default local gateway endpoint — used when the gateway isn't running yet so
 *  a copied snippet is still paste-ready. */
export const DEFAULT_MCP_ENDPOINT = 'http://localhost:45818/mcp';

/** Bearer placeholder — the user swaps in a real per-client API key. */
export const BEARER_HEADER_VALUE = 'Bearer <your API key>';

/**
 * Canonical labels for the copy-config controls. Shared across every surface
 * that offers them (the setup wizard preview + the mapping inspector) so the
 * buttons stay byte-identical instead of drifting (e.g. "Copy with Bearer" vs
 * "Copy with Bearer key"). `COPY_CONFIG_LABEL` copies the plain/header config;
 * `COPY_CONFIG_BEARER_LABEL` copies the variant that adds the `Authorization:
 * Bearer` header.
 */
export const COPY_CONFIG_LABEL = 'Copy config';
export const COPY_CONFIG_BEARER_LABEL = 'Copy with Bearer';
export const COPIED_LABEL = 'Copied';

/**
 * Build the McpMux **server entry** for an MCP client config — just the
 * `"mcpmux": { … }` fragment, NOT the surrounding `{ "mcpServers": { … } }`
 * wrapper, so it pastes straight into a client's existing `mcpServers` block.
 *
 *   • `workspace` — when set, pins the `X-Mcpmux-Workspace` routing header
 *     (a folder path or an arbitrary identifier).
 *   • `bearer` — when true, adds `Authorization: Bearer <your API key>`.
 *
 * Header order is stable (workspace first, then Authorization) so previews and
 * tests read predictably. When neither header applies the `headers` key is
 * omitted entirely.
 */
export function buildMcpConfig({
  endpoint = DEFAULT_MCP_ENDPOINT,
  workspace,
  bearer,
}: {
  endpoint?: string;
  workspace?: string | null;
  bearer?: boolean;
}): string {
  const headers: Record<string, string> = {};
  if (workspace) headers['X-Mcpmux-Workspace'] = workspace;
  if (bearer) headers.Authorization = BEARER_HEADER_VALUE;
  const mcpmux: Record<string, unknown> = { url: endpoint };
  if (Object.keys(headers).length > 0) mcpmux.headers = headers;
  return `"mcpmux": ${JSON.stringify(mcpmux, null, 2)}`;
}
