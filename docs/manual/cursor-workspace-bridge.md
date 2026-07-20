# Manual test — global Cursor bridge via `mcp-remote`

Regression check for the global `~/.cursor/mcp.json` bridge (see
[`cursor-workspace-routing-bridge` planning doc](../planning/cursor-workspace-routing-bridge.md)).

This path replaces per-repo `.cursor/mcp.json` header installs for Cursor by
routing through `npx mcp-remote` with `${workspaceFolder}` in the bridge args.

## Prerequisites

- `pnpm dev:admin` (or production McpMux) with gateway on `localhost:45818`.
- Node.js / `npx` available (for `mcp-remote`).
- Two real workspace folders mapped to **different** FeatureSets in
  **Workspaces** (e.g. `~/proj/alpha` and `~/proj/beta`).
- Cursor installed.

## 1. Generate the global config

1. Open **Connections** in McpMux.
2. In **Global Cursor setup (no per-repo files)**, click **Generate global config**.
3. Copy the snippet and paste it into `~/.cursor/mcp.json` (replace any existing
   `mcpmux` entry, or merge if you have other servers).
4. Reload MCP in Cursor (**Settings → MCP → refresh**).

**Expected:** Cursor connects via stdio (`npx mcp-remote`), not a direct HTTP URL.
On first connect, McpMux may show **Name this machine** — approve it.

## 2. Two-window routing

1. Open folder A in one Cursor window, folder B in another.
2. In each window, list mcpmux tools (or invoke `@mux`).

**Expected:**

- Window A sees only FeatureSet tools bound to folder A.
- Window B sees only FeatureSet tools bound to folder B.
- No cross-contamination (the bug when Cursor reports the wrong `roots`).

## 3. Log verification

Check the McpMux log (macOS:
`~/Library/Application Support/com.mcpmux.desktop/logs/mcpmux.<date>.log`):

- `[SessionRoots] pinned explicit workspace root from X-Mcpmux-Workspace header`
  with the correct path per session.
- `[FeatureSetResolver] resolved via WorkspaceBinding workspace_root=…` matching
  each window's folder.

## 4. Bridge flags sanity check

Confirm the generated config includes:

- `--allow-http` (gateway is loopback HTTP, not TLS).
- `--header` with **no space** after the colon:
  `X-Mcpmux-Workspace:${workspaceFolder}`.
- `Authorization:Bearer ${MCPMUX_API_KEY}` with the key in `env.MCPMUX_API_KEY`.

To verify `mcp-remote` accepts these flags outside Cursor:

```bash
npx -y mcp-remote http://127.0.0.1:45818/mcp --allow-http \
  --header "X-Mcpmux-Workspace:/path/to/folder" \
  --header "Authorization:Bearer mcpk_…"
```

The process should stay up and the gateway should log an incoming MCP session.

## Fallback

If `${workspaceFolder}` does not resolve correctly in your Cursor version, use the
per-repo install path documented in
[`workspace-header-routing.md`](./workspace-header-routing.md) section B.
