# Manual test — global Cursor bridge via `mcp-remote`

Regression check for the global `~/.cursor/mcp.json` bridge (see
[`cursor-workspace-routing-bridge` planning doc](../planning/cursor-workspace-routing-bridge.md)).

This path replaces per-repo `.cursor/mcp.json` header installs for Cursor by
routing through `npx mcp-remote` with `${workspaceFolder}` in the bridge args.

Cursor resolves that variable unreliably (~21% failure, measured), so the bridge
also sends `${WORKSPACE_FOLDER_PATHS}` as `X-Mcpmux-Workspace-Set`. That set is a
constraint on which folder a session may claim, never a way to pick one — see
[Fallback](#fallback) for why inference is off the table.

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

## 1b. Install the workspace hook

Same installer, two surfaces — both write files on the **gateway host**
(Gondor, in the usual setup), including when you click from the web admin:

1. The register-client result screen after **Generate global config**.
2. Any Cursor connection's side panel in **Connections** (Gondor Cursor, the
   global bridge, Rohan Cursor, …). Look for **Workspace hook**.

That writes `~/.cursor/hooks/mcpmux-workspace-context.js` and merges one
`preToolUse` entry into `~/.cursor/hooks.json` (backup: `hooks.json.mcpmux-bak`).
Unrelated hooks (including WakaTime) stay. If `hooks.json` is JSONC, the
installer refuses and shows a copyable entry. Status / install / uninstall are
`GET /api/v1/cursor-hook`, `POST /api/v1/cursor-hook/install`, and
`POST /api/v1/cursor-hook/uninstall` on the admin server, not Tauri-only.

The hook injects `_mcpmux_context` on `MCP:mcpmux_*` calls when
`workspace_roots` has exactly one path. The gateway uses that root for that
`tools/call` only and never writes `pinned` or `window_pins`.

**Expected:** gateway logs
`call_tool exact workspace context source=cursor_pre_tool_use` with the
agent's root and `tool_use_id`. Two concurrent agents on one `mcp-session-id`
should resolve to different bindings. `tools/list` stays the six core meta
tools when the shared session is still ambiguous.

Cloud Agents do not load `~/.cursor/hooks.json`. They stay on the rootless /
`mcpmux_set_workspace_root` path. See
[`cursor-agent-hooks-workspace-hint.md`](../planning/cursor-agent-hooks-workspace-hint.md#cloud-agents-researched-aug-21).

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

- `[SessionRoots] X-Mcpmux-Workspace held until mcp-session-id exists` on
  initialize, then `pinned explicit workspace root source=X-Mcpmux-Workspace`
  with the correct path per session. A non-empty header without a session id
  is held, not skipped. `source` distinguishes a working header from a manual
  `source=mcpmux_set_workspace_root` recovery and from a one-member
  `source=X-Mcpmux-Workspace-Set(single)` — the whole question this log answers.
- `[SessionRoots] window identity from peer socket window_key=pid:…` once per
  session. The same PID should cover that window's later sessions (Reload MCP
  included, if `mcp-remote` did not respawn).
- `[SessionRoots] window pin stored` once per distinct claim, naming the
  `window_key` that will inherit it. Absence after a pin means the peer socket
  had no owning PID yet, so nothing will survive session churn.
- `→ MCP` lines include `window_key=pid:…` next to `session_id`.
- `[FeatureSetResolver] resolved via WorkspaceBinding workspace_root=…` matching
  each window's folder.

Three warns exist to report that the bridge's assumptions broke. None should
appear in a healthy two-window run:

| Log line                                            | Means                                                                                                                                                            |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `X-Mcpmux-Workspace-Set arrived unexpanded`         | Cursor mangled `${WORKSPACE_FOLDER_PATHS}` instead of passing it to `mcp-remote`. No candidate set, so routing degrades to pre-set-header behavior.              |
| `pinned root is absent from X-Mcpmux-Workspace-Set` | The active folder isn't a member of the reported set. This violates the invariant the constraint rests on; `set_workspace_root` will start refusing valid roots. |
| `held X-Mcpmux-Workspace names a folder this window does not have open` | A pending header parked under the shared access key belonged to a *different* window. Pin skipped rather than applied. |
| `no pinned root and multiple folders open`          | The 16% case. Expected occasionally; the session needs one `mcpmux_set_workspace_root` call. After that call, Reload MCP should log `inherited workspace pin from window` instead of this warn again. |
| `inherited workspace pin from window`               | Healthy. This session had no header; the window's previous explicit pin was reused.                                                                              |
| `window pin is absent from X-Mcpmux-Workspace-Set`  | The remembered folder is not in this session's open set — inheritance skipped rather than misrouting.                                                            |

## 4. Bridge flags sanity check

Confirm the generated config includes:

- `--allow-http` (gateway is loopback HTTP, not TLS).
- `--header` with **no space** after the colon:
  `X-Mcpmux-Workspace:${workspaceFolder}`.
- `--header X-Mcpmux-Workspace-Set:${WORKSPACE_FOLDER_PATHS}`. Not a Cursor
  variable, so it passes through untouched and `mcp-remote` expands it from the
  child environment. Carries every folder open in the calling window.
- `Authorization:Bearer mcpk_…` with the key **inlined**, not referenced through
  `env.MCPMUX_API_KEY`. The two workspace headers have to be variables because
  they differ per window; the key is a constant, and routing it through `env`
  only exposed auth to the same substitution flake. A respawn was observed
  sending the literal `${MCPMUX_API_KEY}`, which the gateway 401s with
  `Authorization header contains an unexpanded ${...} template` while Cursor
  reports nothing but `Timed out waiting for connection`. If you see that 401,
  regenerate the snippet (Connections → Global Cursor setup) — an older config
  still using `env` is the cause.

To verify `mcp-remote` accepts these flags outside Cursor:

```bash
npx -y mcp-remote http://127.0.0.1:45818/mcp --allow-http \
  --header "X-Mcpmux-Workspace:/path/to/folder" \
  --header "X-Mcpmux-Workspace-Set:/path/to/folder,/path/to/other" \
  --header "Authorization:Bearer mcpk_…"
```

The process should stay up and the gateway should log an incoming MCP session.

## Fallback

Cursor fails to substitute `${workspaceFolder}` before spawning `mcp-remote` in
roughly 21% of spawns. `mcp-remote` then expands the leftover literal to an
empty string, so the gateway sees `X-Mcpmux-Workspace` present but empty and
skips the pin.

```
[SessionRoots] X-Mcpmux-Workspace present but empty — pin skipped
```

This is **not** an Agents-window problem, despite what earlier revisions of this
doc claimed. A 282-spawn probe measured editor windows failing at 29% and Agents
windows at 4%, across folder counts from zero to five. It's a flaky
substitution, not a surface-specific one.

There is no fallback signal for the active folder. The probe checked all 22
Cursor and VS Code environment variables in the child process:
`CURSOR_WORKSPACE_LABEL` is stale (it names the window that started the
extension host, often a folder not even in the set), `VSCODE_PID` and
`VSCODE_IPC_HOOK` are app-level rather than per-window, `cwd` is always the home
directory, and no `.code-workspace` file exists for ad-hoc multi-root windows.

`WORKSPACE_FOLDER_PATHS` is the one usable signal, and only as a constraint. The
active folder was a member of it in 212 of 212 resolved multi-folder spawns, but
its position identified the active folder in only 70% of them. A 30% misroute
rate would hand one project's credentials to another, so the gateway does not
infer from position. What it does instead:

- **One folder in the set:** pins outright. Unambiguous by construction.
- **No folders:** no workspace to route to; falls through to grants.
- **Two or more:** refuses to guess. The session gets meta tools only until it
  calls `mcpmux_set_workspace_root`, which is now validated against the set so a
  caller can't declare a folder its window doesn't have open.

## Window pin

An explicit claim — a substituted `X-Mcpmux-Workspace` header, or one
`mcpmux_set_workspace_root` call — is remembered for the life of that window's
`mcp-remote` process (loopback peer port → owning PID). Later sessions from the
same process inherit it. A live header on a new session always wins, so a
folder switch is never overridden by the leftover answer.

Grep:

```
[SessionRoots] window identity
[SessionRoots] window pin stored
[SessionRoots] inherited workspace pin from window
[SessionRoots] X-Mcpmux-Workspace present but empty
```

The first two are the write side, the third is the read side. Seeing `window
pin stored` but never an inherit line means the process died between sessions;
seeing neither means the claim never reached a window at all.

A Reload MCP that still shows the empty-header warn (and not an inherit line)
means the process died and there was nothing to inherit. One `set_workspace_root`
covers the new process.

Remote / tunnel clients have no local PID and keep today's per-session
behavior.

To avoid the whole class of problem, use the per-repo install in
[`workspace-header-routing.md`](./workspace-header-routing.md) section B. It
writes a literal path into `.cursor/mcp.json` with no variable to substitute,
which is why it never flakes. Note that it also writes the bearer token into a
file inside the repo and does not add a `.gitignore` entry, so exclude it
yourself before committing.

## Git worktrees

Bindings match a reported root by exact path, and a linked git worktree is a
different path from its main checkout. When the exact lookup finds nothing and
a reported root is a linked worktree (a `.git` file that points into
`<main>/.git/worktrees/<name>`, whose `gitdir` file points back), the gateway
retries with the main checkout's path. A binding on the worktree itself still
wins. A `.git` file with no matching back-pointer is ignored, so a hostile repo
cannot borrow another checkout's bindings.

When `mcpmux_invoke_tool` refuses a server as `inactive`, the `action` text names
the root the caller resolved from and the resolution source.

## How to re-measure

The 21% / 29% / 4% figures above came from a 282-spawn `env-probe` wrap of
`mcp-remote`. Re-run that after a Cursor update with the committed scripts:

1. `pnpm probe:cursor-env` prints a `~/.cursor/mcp.json` snippet. `command` is
   `node`; the first arg is the absolute path to
   [`scripts/cursor-env-probe.mjs`](../../scripts/cursor-env-probe.mjs). Paste
   it over the existing `mcpmux` entry, substituting your real `mcpk_` key for
   the placeholder in the `Authorization` header.
2. Reload MCP. Use editor and Agents windows until you have hundreds of
   spawns. Each spawn appends one record to
   `$HOME/Desktop/mcpmux-env-probe.log` (override with `MCPMUX_ENV_PROBE_LOG`),
   then execs `mcp-remote` so the session still works.
3. `pnpm probe:cursor-env:summary` reprints the Aug 20 cuts: unresolved rate
   overall and by `CURSOR_AGENT`, folder-count histogram, membership vs
   `WFP[0]` on resolved multi-folder spawns, unexpanded
   `${WORKSPACE_FOLDER_PATHS}` count.
4. Restore the generated bridge config (Connections → Global Cursor setup).

The wrapper logs argv/env/pwd only. It does not contain tokens from the child
stdio. The Desktop log path is outside the repo; do not copy it in.
