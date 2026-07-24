# Cursor Workspace Routing via Global `mcp-remote` Bridge

**Last Updated:** Jul 20, 2026
**Status:** Complete (Phases 1–3 on `dev-rebased`)
**Branch:** `dev-rebased`

### Phase 1 spike results (Jul 20, 2026)

- **mcp-remote:** `0.1.38` via `npx`; supports `--allow-http` and `--header` (no space after `:`).
- **Gateway:** `localhost:45818` up (`0.5.0`).
- **Auth + connect:** `phase1-spike-bridge` client reached gateway; machine-naming dialog appeared and was approved.
- **Remaining manual QA:** two-window `${workspaceFolder}` routing not yet verified in real Cursor; transport/auth path is confirmed.
**Depends on:** `docs/manual/workspace-header-routing.md` (existing per-repo header fix this supersedes as the recommended path), `upstream-client-mapping-reconciliation.md` Phase 1 (`mcpk_` API-key auth — this feature's auth mechanism)
**Unblocks:** Zero-maintenance Cursor workspace routing — no per-repo files, no agent cooperation required

---

## Problem

Cursor doesn't reliably report the MCP `roots` capability — it can report a stale or wrong workspace folder (e.g. a different open window's folder), so the resolver's path-based `WorkspaceBinding` lookup gets the wrong root and two folders mapped to different FeatureSets can cross-contaminate (`docs/manual/workspace-header-routing.md`).

The existing fix (`apps/desktop/src-tauri/src/commands/workspace_install.rs`) writes a project-local `.cursor/mcp.json` per repo with an `X-Mcpmux-Workspace` header baked in, because a *global* Cursor config can only hold one static header value — it can't vary per project. This works, but it's a real, standing maintenance burden: every new repo needs a manual "Install into 1 app" click plus a `.gitignore` entry, forever.

The other obvious escape hatch — the `mcpmux_set_workspace_root` meta tool, which lets an agent self-report its root — trades the per-repo file for a dependency on the LLM actually calling it every session. Not deterministic enough to rely on as the primary mechanism.

Cursor's own docs, however, resolve `${workspaceFolder}` reliably in the `command`/`args`/`env` fields of a stdio server entry — even one declared in the *global* `~/.cursor/mcp.json` — because Cursor spawns a stdio child process fresh per workspace window and substitutes variables at spawn time, not at file-parse time. The known interpolation flakiness (Cursor forum bug reports) is specific to the `headers` field on a native `url`-type (remote) entry, not to `args` on a `command`-type (stdio) entry. That gap is exploitable: route Cursor through a stdio bridge instead of connecting to the gateway's HTTP endpoint directly, and pass the workspace header through the bridge's `args`, where interpolation is the reliable path.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Bridge implementation | **`mcp-remote`** (existing npm package, `npx mcp-remote`), not a first-party McpMux binary | Already solves stdio↔remote-HTTP bridging with a `--header` flag that supports arbitrary custom headers. Building our own binary duplicates it for no gain unless `mcp-remote` proves unreliable in practice (Phase 1 spike decides this). |
| 2 | Workspace signal | `${workspaceFolder}` passed inline inside a `--header` value in the bridge's `args`, e.g. `--header X-Mcpmux-Workspace:${workspaceFolder}` | This is the documented-reliable interpolation path (`args`/`command`), not the flaky one (`headers` on a native remote entry). No space around the `:` to dodge Cursor's known arg-escaping bug with `npx`. |
| 3 | Auth | Static `mcpk_` API-key header (`Authorization: Bearer mcpk_...`) via a second `--header` flag, not OAuth-through-the-bridge | `mcp-remote` does its own OAuth dance if no static header is given, which is one more auth surface to reason about. The API-key auth path shipped in `upstream-client-mapping-reconciliation.md` Phase 1 exists for exactly this kind of headless/remote-client case. |
| 4 | Relationship to existing per-repo install | **Keep both** — the global bridge becomes the *recommended* Cursor setup; the existing per-repo `.cursor/mcp.json` header install (`workspace_install.rs`) stays as a fallback for anyone who doesn't want an `npx`/Node dependency in the loop | Don't rip out a working, tested mechanism to replace it with an unverified one. `${workspaceFolder}`-via-`args` needs to be confirmed against real Cursor behavior before it's trusted as the default (Phase 1). |
| 5 | Scope of client support | Cursor only — no changes for VS Code, Claude Code, or other clients | Those clients already route correctly via standard `roots` reporting (confirmed in `docs/manual/workspace-header-routing.md`: "VS Code / Claude Code are good controls — they already route correctly via roots"). This is a Cursor-specific spec-compliance gap, not a general McpMux limitation. |

---

## Scope

**In:**

- Manual spike confirming `${workspaceFolder}` resolves per-window correctly through a global `mcp-remote` entry in real Cursor (not just per docs)
- A generated global bridge config snippet, surfaced in the desktop app, that mints an `mcpk_` API key and emits ready-to-paste JSON for `~/.cursor/mcp.json`
- Docs update recommending the global bridge as the primary Cursor setup path, with the existing per-repo header install documented as the fallback

**Out:**

| Item | Reason / Deferral |
| ---- | ------------------ |
| First-party McpMux bridge binary (replacing `mcp-remote`) | Decision 1 — only worth building if the Phase 1 spike finds `mcp-remote` unreliable or insufficient. Not blocking this feature. |
| Cursor/VS Code extension reading `vscode.workspace.workspaceFolders` directly | Disproportionate effort (a whole editor extension) for a gap that's isolated to one client's `roots` implementation. Revisit only if this class of bug recurs across other clients. |
| Deprecating/removing the per-repo `.cursor/mcp.json` install panel | Decision 4 — stays as a supported fallback indefinitely, not a transitional shim to delete later. |
| Gateway-side process-tree introspection to infer workspace without any client config | Dead end — the gateway sees a TCP connection over streamable HTTP, not a spawned child process; there's no PID to walk. Not pursued. |

---

## Architecture

### Connection shape (before → after)

```text
Before:
  Cursor  --url: http://localhost:45818/mcp-->  McpMux Gateway
          (roots/list unreliable, or per-repo .cursor/mcp.json header)

After (global, zero per-repo files):
  Cursor  --spawns per window-->  npx mcp-remote (stdio child)
                                       |
                                       | --header X-Mcpmux-Workspace:<resolved per window>
                                       | --header Authorization:Bearer mcpk_...
                                       v
                                  McpMux Gateway (http://localhost:45818/mcp)
```

Cursor resolves `${workspaceFolder}` to the active window's project root *before* spawning `npx`, so each window's `mcp-remote` child process carries a different, correct header value — from one global config entry, with no per-repo file and no agent involvement.

### Global config shape

```jsonc
// ~/.cursor/mcp.json
{
  "mcpServers": {
    "mcpmux": {
      "command": "npx",
      "args": [
        "-y", "mcp-remote",
        "http://localhost:45818/mcp",
        "--allow-http",
        "--header", "X-Mcpmux-Workspace:${workspaceFolder}",
        "--header", "Authorization:Bearer ${MCPMUX_API_KEY}"
      ],
      "env": { "MCPMUX_API_KEY": "mcpk_..." }
    }
  }
}
```

`--allow-http` is required since the gateway binds plain HTTP on loopback (`127.0.0.1:45818`), not HTTPS — `mcp-remote` otherwise assumes a TLS remote endpoint.

### Interaction with existing resolver tiers

No resolver changes. The bridge is purely a transport-layer trick to get `X-Mcpmux-Workspace` populated correctly — the gateway already treats that header as authoritative and pins it ahead of probed `roots` (`session_roots.rs`, `SessionRootsRegistry`). This feature doesn't touch `feature_set_resolver.rs`, `workspace_binding_repository.rs`, or any migration.

---

## Files to create / modify

| Area | File cluster | Action |
| ---- | ------------- | ------ |
| Desktop UI | `apps/desktop/src/features/clients/CursorBridgeSection.tsx` (or fold into `ClientsPage.tsx`) | Create — "Global Cursor setup (no per-repo files)" panel: mints an `mcpk_` key via the existing Phase 1 API-key commands, renders the ready-to-paste `~/.cursor/mcp.json` snippet, one-click copy |
| Tauri | `apps/desktop/src-tauri/src/commands/oauth.rs` | Modify (if needed) — reuse `create_client_api_key`/`register_api_key_client` from `upstream-client-mapping-reconciliation.md` Phase 1; no new command expected unless the UI needs a combined "register + mint key + render snippet" convenience call |
| Docs | `docs/manual/workspace-header-routing.md` | Modify — add a section presenting the global bridge as the recommended path, existing per-repo install as fallback |
| Docs | `docs/guide/remote-access.mdx` | Modify — mention the bridge option alongside existing tunneled-client config guidance, if applicable |
| Manual test | `docs/manual/cursor-workspace-bridge.md` | Create — step-by-step verification doc for Phase 1's spike (two windows, two folders, confirm correct routing per window) |

---

## Phases

### Phase 1 — Manual spike, no code (~1 hour)

Confirms the core assumption before any UI work is built on top of it.

- Manually write a global `~/.cursor/mcp.json` per the shape above, using a manually-minted `mcpk_` key (via existing Clients page UI from Phase 1 of the client-mapping reconciliation work)
- Open two real folders in two separate Cursor windows, each already mapped to a distinct FeatureSet via existing Workspace bindings
- Confirm via gateway logs that each window's `mcp-remote` child sends a different, correct `X-Mcpmux-Workspace` value, and that each window's agent sees only its own bound FeatureSet's tools
- Confirm `--allow-http` and the no-space `--header` syntax are both necessary/sufficient (verify against the actual installed `mcp-remote` version, not just its README)

**Outcome:** Either the bridge works exactly as designed (two Cursor windows on two folders, zero per-repo files, correct tool sets in each) — in which case Phase 2 proceeds — or it surfaces a real gap (e.g. `${workspaceFolder}` doesn't resolve for a global-scope entry the way the docs imply), in which case this doc gets amended before any UI is built.

---

### Phase 2 — Desktop UI generator (~1 day)

Removes the "hand-assemble JSON" friction so the bridge is actually usable by someone who isn't reading this planning doc.

- `CursorBridgeSection.tsx` (or equivalent): a panel that, on click, mints a new `mcpk_` API key scoped to a client named something like `cursor-global-bridge`, and renders the full `~/.cursor/mcp.json` snippet with the key already substituted in
- One-click copy of the snippet; a short inline note explaining it replaces the need for per-repo `.cursor/mcp.json` files
- No changes to the per-repo install panel — both paths coexist as documented alternatives (Decision 4)

**Outcome:** A user can go from "never configured this" to a working global bridge in under a minute, without touching a terminal or writing JSON by hand.

---

### Phase 3 — Docs consolidation (~half day)

- `docs/manual/workspace-header-routing.md`: add the global-bridge path as the recommended Cursor setup, explicitly keep the per-repo install documented as the supported fallback (not deprecated)
- New `docs/manual/cursor-workspace-bridge.md`: manual verification steps mirroring Phase 1's spike, so this stays a repeatable regression check rather than a one-time investigation
- Cross-link from `docs/guide/remote-access.mdx` if the tunneled/remote-gateway story overlaps

**Outcome:** Someone new to the repo can find and follow the recommended Cursor setup without reading this planning doc or the original brainstorm conversation.

---

## Key files referenced

| File | Note |
| ---- | ---- |
| [`apps/desktop/src-tauri/src/commands/workspace_install.rs`](../../apps/desktop/src-tauri/src/commands/workspace_install.rs) | The existing per-repo header install this feature supplements, not replaces |
| [`crates/mcpmux-gateway/src/services/session_roots.rs`](../../crates/mcpmux-gateway/src/services/session_roots.rs) | `X-Mcpmux-Workspace` is already authoritative here — no gateway changes needed |
| [`docs/manual/workspace-header-routing.md`](../manual/workspace-header-routing.md) | Documents the underlying Cursor `roots`-reporting bug this bridge works around |
| [`docs/planning/upstream-client-mapping-reconciliation.md`](./upstream-client-mapping-reconciliation.md) | Phase 1 — `mcpk_` API-key auth, reused here as the bridge's auth mechanism |
| [`apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx`](../../apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx) | Existing API-key minting UI this feature's Phase 2 panel is modeled on |

---

## Related documentation

- [`docs/manual/workspace-header-routing.md`](../manual/workspace-header-routing.md) — the Cursor `roots`-reporting bug and the per-repo header fix
- [`docs/planning/upstream-client-mapping-reconciliation.md`](./upstream-client-mapping-reconciliation.md) — `mcpk_` API-key auth this feature depends on
- [`docs/planning/per-device-machine-header.md`](./per-device-machine-header.md) — prior art for a header-based routing signal (`X-Mcpmux-Machine-Id`), same pattern applied to a different axis
