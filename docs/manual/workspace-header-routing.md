# Manual test — per-workspace routing via `X-Mcpmux-Workspace`

Covers the feature added on `feat/workspace-header-mapping`:

1. Deterministic per-workspace routing via the `X-Mcpmux-Workspace` header
   (fixes Cursor reporting the wrong/another workspace root).
2. One-click per-workspace client config install.
3. System-wide "disable authentication" toggle.

Automated coverage exists for the resolver, the config writer, the gateway
state toggle, and the install panel (see _Automated tests_ at the end). The
steps below verify the end-to-end behavior that automation can't — a real
client connecting through the gateway.

## Prerequisites

- `pnpm dev` (desktop app + gateway) running.
- Two real workspace folders, e.g. `D:\proj\alpha` and `D:\proj\beta`.
- Cursor installed (the client this feature primarily targets). VS Code /
  Claude Code are good controls — they already route correctly via roots.

---

## A. Header routing fixes the wrong-workspace bug

**Goal:** prove the header overrides what the client reports.

1. In the app, **Workspaces → New mapping**: map `D:\proj\alpha` to a Space +
   a distinctive FeatureSet (call it _Alpha FS_, with a tool only it has).
   Map `D:\proj\beta` to a different _Beta FS_.
2. In the `D:\proj\alpha` mapping, open **Connect apps to this folder**, tick
   **Cursor**, and click **Install into 1 app**. Confirm
   `D:\proj\alpha\.cursor\mcp.json` now contains an `mcpmux` entry with
   `"headers": { "X-Mcpmux-Workspace": "D:\\proj\\alpha" }`.
3. Repeat for `D:\proj\beta`.
4. Open **both** folders in Cursor (two windows). In each, ask the agent to
   list mcpmux tools (or invoke `@mux`).

**Expected:** the `alpha` window sees _Alpha FS_ tools; the `beta` window sees
_Beta FS_ tools. Before this change, both windows showed whichever folder
Cursor happened to report — the bug.

**Verify in logs** (`%LOCALAPPDATA%\com.mcpmux.desktop\logs\mcpmux.<date>.log`):

- `[SessionRoots] pinned explicit workspace root from X-Mcpmux-Workspace header`
  with the right path per session.
- `[FeatureSetResolver] resolved via WorkspaceBinding workspace_root=d:\proj\alpha`
  (and `…\beta`) — note the header path wins even if Cursor also reports a
  different root.

---

## B. One-click install — create and extend

1. **New folder, no config:** pick a fresh folder with no `.cursor/` etc.
   Install for Cursor + Claude Code + VS Code. Confirm three files are
   **created**: `.cursor/mcp.json`, `.mcp.json`, `.vscode/mcp.json`, each with
   the correct top-level key (`mcpServers` / `mcpServers` / `servers`) and the
   workspace header.
2. **Existing config, preserved:** in a folder that already has a
   `.cursor/mcp.json` with another server, install again. Confirm:
   - the other server is still present,
   - an `mcpmux` entry was added/updated,
   - a `mcp.json.mcpmux-bak` backup was written.
3. **Non-JSON guard:** put a `//` comment in `.cursor/mcp.json`, install, and
   confirm that client reports an **error** ("not plain JSON…") and the file is
   left untouched (no clobber).
4. **Copy config:** click the copy icon on a client row, paste — you get a full
   `{ "<key>": { "mcpmux": { … } } }` snippet for that client.

---

## C. Disable authentication

1. **Settings → Security → Disable authentication: ON.** Toast confirms.
2. Connect a client whose config has **no** `Authorization` header (the
   installer writes none) — e.g. the Cursor config from step A.

**Expected:** the client connects and resolves normally (no 401). Logs show
`→ MCP` lines with `client=mcpmux-anon…` for tokenless requests.

3. **Toggle OFF again.** A tokenless client now gets `401`; a client with a
   valid access key still connects (lenient — a valid token is always honored).
4. Restart the app with the toggle ON and confirm it persists (seeded into the
   gateway at startup).

The install panel's inline **Disable authentication** button (shown when auth
is on) performs the same toggle without leaving the flow.

---

## D. Self-introductory hints (discoverability)

- **Approval sheet** (open an unmapped folder in a connected app): shows the
  "Connect apps to this folder" tip.
- **Apps page → a client → "Routing is workspace-driven":** mentions installing
  a per-workspace config when a client doesn't report folders reliably.
- **Install panel:** shows the auth state and offers to disable it inline.

---

## Automated tests (run before manual)

```bash
pnpm test:rust:int    # resolver: pinned-header routing, override, fallback
pnpm test:rust:unit   # session_roots pin/shadow/clear; GatewayState auth toggle;
                      # workspace_install merge/create/extend/backup
pnpm test:ts -- WorkspaceInstallPanel
```

All should pass.
