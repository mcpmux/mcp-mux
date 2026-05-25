# Personal fork integration (crimsonsunset)

**Last updated:** May 25, 2026  
**Purpose:** Single source of truth for local/homelab work on [crimsonsunset/mcp-mux](https://github.com/crimsonsunset/mcp-mux). Upstream PR stacking is separate and optional.

---

## Canonical branch

| Branch | Role |
|--------|------|
| **`feat/meta-gateway-invoke`** | **Integration tip** — run `pnpm dev`, gateway QA, and all new work here |
| `feat/dynamic-mcp-toggle-meta-tools` | Pointer kept in sync with tip (session meta-tools, bindings base) |
| `feat/server-account-clones` | Pointer kept in sync with tip (account clones + meta-gateway invoke) |
| `main` | Tracks upstream `mcpmux/mcp-mux` — **not** where fork features live |

**Rule:** If it is not on `feat/meta-gateway-invoke`, you are not running your fork.

### What is on the integration tip

Linear stack (newest at top):

1. Meta-gateway invoke (search → schema → invoke, surfaced tools, opt-in invoke filters)
2. Server account clones
3. Dynamic MCP toggle meta-tools + workspace/session routing (fork lineage ahead of upstream `main`)
4. Planning docs and homelab QA sign-offs

See [`meta-gateway-invoke.md`](./meta-gateway-invoke.md), [`agent-mcp-session-readiness.md`](./agent-mcp-session-readiness.md), [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md).

---

## Daily workflow

```bash
git checkout feat/meta-gateway-invoke
git pull origin feat/meta-gateway-invoke   # after pushes from other machines
pnpm dev                                   # gateway on localhost:45818
```

New feature work:

```bash
git checkout feat/meta-gateway-invoke
git pull origin feat/meta-gateway-invoke
git checkout -b feat/my-topic
# ... commits ...
git checkout feat/meta-gateway-invoke
git merge feat/my-topic                    # or rebase topic onto tip first
git push origin feat/meta-gateway-invoke
```

Homelab Cursor config: one `mcpmux` entry → `http://localhost:45818/mcp`. Migration tracker: [mcpmux-server-migration.md](../../../jsg-tech-check/docs/setup/mcpmux-server-migration.md).

---

## Upstream PR policy (mcpmux/mcp-mux)

| PR | Action | Why |
|----|--------|-----|
| [#152](https://github.com/mcpmux/mcp-mux/pull/152) `fix/dcr-skip-invalid-redirect-uris` | **Keep open** | Small, standalone OAuth fix (~47 lines) → `main` |
| [#154](https://github.com/mcpmux/mcp-mux/pull/154) `feat/dynamic-mcp-toggle-meta-tools` | **Keep open (draft)** | Proper stack: base `feat/workspace-root-routing`, not a megapr |
| [#155](https://github.com/mcpmux/mcp-mux/pull/155) `feat/meta-gateway-invoke` | **Closed** | Wrong base (`main`); entire fork stack (~28k LOC). Work lives on fork integration branch only |

**Not owned by this fork:** [#151](https://github.com/mcpmux/mcp-mux/pull/151) workspace-root-routing (upstream). #154 targets that branch when contributing meta-tools upstream.

Future upstream contributions: branch off fresh `upstream/main` (or merged #151), cherry-pick or restack **one topic per PR** — do not reopen a megapr to `main`.

---

## Branch pointer sync

After the integration tip moves, fast-forward stale names (optional hygiene):

```bash
git checkout feat/dynamic-mcp-toggle-meta-tools && git merge --ff-only feat/meta-gateway-invoke
git checkout feat/server-account-clones && git merge --ff-only feat/meta-gateway-invoke
git push origin feat/meta-gateway-invoke feat/dynamic-mcp-toggle-meta-tools feat/server-account-clones
```

---

## Next implementation priorities (on integration branch)

1. [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md) — cold start / `gateway_warming`
2. Homelab `mcp.json` cutover (bindings + `bundle:core`)
3. Replace stock `McpMux.app` with a build from this branch ([`run-from-source-macos.md`](../run-from-source-macos.md))

---

## Reconciliation

When the integration tip gains a major milestone, update this file's **Last updated** and the **What is on the integration tip** list. Close new upstream megaprs to `main`; keep topic PRs stacked on the appropriate upstream feature branch.
