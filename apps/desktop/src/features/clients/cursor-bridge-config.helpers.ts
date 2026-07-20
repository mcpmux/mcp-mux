/** Default API-key client name for the global Cursor mcp-remote bridge. */
export const CURSOR_BRIDGE_CLIENT_NAME = 'cursor-global-bridge';

/**
 * Build the `~/.cursor/mcp.json` snippet for the global mcp-remote bridge.
 *
 * Cursor resolves `${workspaceFolder}` in `args` at spawn time, so one global
 * entry routes each window to the correct workspace header.
 */
export function buildCursorBridgeMcpJson(apiKey: string, gatewayUrl: string): string {
  const mcpUrl = `${gatewayUrl.replace(/\/$/, '')}/mcp`;
  const config = {
    mcpServers: {
      mcpmux: {
        command: 'npx',
        args: [
          '-y',
          'mcp-remote',
          mcpUrl,
          '--allow-http',
          '--header',
          'X-Mcpmux-Workspace:${workspaceFolder}',
          '--header',
          'Authorization:Bearer ${MCPMUX_API_KEY}',
        ],
        env: { MCPMUX_API_KEY: apiKey },
      },
    },
  };
  return JSON.stringify(config, null, 2);
}
