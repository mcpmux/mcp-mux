# AGENTS.md

Guidance for coding agents working inside the `mcp-mux` repo — the McpMux desktop app and local gateway. Complements [`README.md`](README.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md); when anything here conflicts with an explicit user instruction in the current session, the user wins.

## Project Overview

McpMux is a Tauri 2 desktop app (Rust + React 19) with a local Axum HTTP gateway on `localhost:45818`. It lets users configure MCP servers once and connect every AI client (Cursor, Claude Desktop, VS Code, Windsurf) through a single endpoint, with credentials encrypted in the OS keychain instead of plain-text JSON files.

A more detailed map of the workspace lives in [`CLAUDE.md`](CLAUDE.md) at the repo root — read it for the crate layout, frontend architecture, and cross-project context. This file captures the minimum an agent needs to make safe, useful changes here.

## Workspace Layout

```
mcp-mux/
├── apps/desktop/          # Tauri shell — React frontend (src/) + Rust Tauri commands (src-tauri/)
├── crates/
│   ├── mcpmux-core/       # Domain entities, repository traits, service layer, EventBus
│   ├── mcpmux-gateway/    # Axum gateway — routing, OAuth refresh, FeatureSet filtering
│   ├── mcpmux-storage/    # SQLite + AES-256-GCM field encryption + OS keychain
│   └── mcpmux-mcp/        # MCP protocol client wrapper (rmcp SDK)
├── packages/ui/           # Shared UI components (`@mcpmux/ui`)
├── schemas/               # JSON Schemas surfaced in the Monaco editor
└── tests/                 # Rust integration, TS unit (vitest), desktop E2E (WDIO), web E2E (playwright)
```

## Build & Dev Commands

Run everything from `mcp-mux/`:

| Command | What it does |
|---------|--------------|
| `pnpm setup` | First-time dev environment setup (PowerShell on Windows). |
| `pnpm dev` | Tauri desktop dev mode (Rust + React hot-reload). |
| `pnpm dev:web` | Web UI only via Vite — no Rust, no Tauri shell. |
| `pnpm dev:admin` | Full stack + web admin: `tauri dev` with admin enabled, opens browser at `:1420` once the gateway is stable. Primary dev driver. |
| `pnpm dev:web:admin` | Browser-only admin UI; auto-starts the backend detached if it isn't already up. |
| `pnpm dev:stop` | Quit McpMux.app and free the dev ports (`:1420`, `:45818`, `:45819`). |
| `pnpm dev:rebuild` | Force `cargo build --workspace` (debug) without launching — recovery for stale binaries. |
| `pnpm build` | Production Tauri build for the current platform. |
| `pnpm validate` | Full correctness gate — runs the items below in sequence. |
| `pnpm lint` | ESLint (recursive) + `cargo clippy --workspace -- -D warnings`. |
| `pnpm lint:fix` | Auto-fix lint issues. |
| `pnpm format` | `prettier --write .` + `cargo fmt --all`. |
| `pnpm format:check` | Formatting check (no writes). |
| `pnpm typecheck` | Recursive TypeScript typecheck. |

**Hot-reload while developing:** `pnpm dev:admin` keeps the desktop window, the `:1420` browser tab (Vite HMR), and the CF tunnel / `:45819` static UI in sync. TS/CSS changes hot-reload instantly on `:1420`; the same edits trigger `vite build --watch` to rebuild `apps/desktop/dist/` for `:45819` and remote tunnel tabs (hard-refresh those after each rebuild, ~10s). Rust changes in any workspace crate trigger `tauri dev` to recompile and restart the backend automatically. Rust is compiled, so "auto-reload" means recompile + process restart, not live patching. After a stalled restart, orphaned backend, or stale binary, recover with `pnpm dev:stop && pnpm dev:rebuild && pnpm dev:admin`. The repo's `.vscode/settings.json` sets `rust-analyzer.cargo.targetDir` so the editor's `cargo check` doesn't fight `tauri dev` over `target/` (avoids double compiles). When `.env` carries CF Access credentials the admin runs with `cf_access=true`; the dev scripts' health probes send `adminCfProbeHeaders()` so `/api/v1/health` doesn't 401. One-off production bundle: `pnpm build:web:admin`.

**Before claiming a change is done**, run `pnpm validate` (or the relevant subset) — it mirrors what CI enforces.

## Testing

| Command | Scope |
|---------|-------|
| `pnpm test` | Rust + TypeScript, everything. |
| `pnpm test:rust` | `cargo nextest run --workspace`. |
| `pnpm test:rust:unit` | `cargo nextest run --workspace --lib`. |
| `pnpm test:rust:int` | `cargo nextest run -p tests` — integration crate in `tests/rust`. |
| `pnpm test:rust:doc` | `cargo test --workspace --doc`. |
| `pnpm test:ts` | Vitest run (`tests/ts/vitest.config.ts`). |
| `pnpm test:ts:watch` | Vitest watch. |
| `pnpm test:e2e` | Desktop E2E via WebDriver IO — requires `MCPMUX_REGISTRY_URL`. |
| `pnpm test:e2e:file -- tests/e2e/specs/foo.ts` | One WDIO spec file. |
| `pnpm test:e2e:grep -- "test name"` | WDIO tests matching a name. |
| `pnpm test:e2e:web` | Playwright on the web UI. |
| `pnpm test:coverage` | `cargo llvm-cov` + Vitest coverage. |

Prefer narrow commands over `pnpm test` while iterating — the full suite is slow.

## Code Style

- **Rust:** 100-char max width, 4-space indent. Clippy runs with `avoid-breaking-exported-api = false`; all warnings are denied in CI.
- **TypeScript / JSX:** Prettier — single quotes, 2-space indent, 100-char width, trailing commas (es5), Tailwind CSS plugin for class ordering.
- **Path aliases:** `@/` → `apps/desktop/src/`; `@mcpmux/ui` → `packages/ui`.
- **No emojis in code or commits** unless the user explicitly asks for them.
- **Comments:** only when the *why* is non-obvious. Identifiers should explain the *what*.

## Commit & PR Guidelines

- Commits must be **signed off** (DCO): `git commit -s -m "..."`. CI rejects unsigned commits.
- Prefer conventional-style subjects — releases use release-please for semantic versioning.
- PRs follow [`.github/pull_request_template.md`](.github/pull_request_template.md): describe the change, how you tested, and check the `pnpm test` / `pnpm lint` / `pnpm typecheck` boxes.
- Don't bypass hooks (`--no-verify`) or DCO signing unless explicitly told to.

## Platform Gotchas

### Child-process flags

Anything that spawns a child process (stdio MCP servers, installers, etc.) **must** go through `mcpmux_gateway::pool::transport::configure_child_process_platform()`. That helper applies:

- **Windows:** `CREATE_NO_WINDOW` (`0x08000000`) — release builds use `windows_subsystem = "windows"`, so without this the OS briefly flashes a console window when a child starts.
- **Unix:** `process_group(0)` — stops SIGINT/SIGTSTP from the parent terminal from tearing down the child.

`tokio::process::Command` already exposes `creation_flags()` (Windows) and `process_group()` (Unix). **Do not** import `std::os::*::process::CommandExt` — those traits are unused with Tokio's `Command` and trigger clippy.

### Cross-platform CI

- The pre-commit hook runs `cargo clippy --workspace -- -D warnings` on your dev machine.
- `#[cfg(unix)]` only compiles on Unix; `#[cfg(windows)]` only on Windows. CI is Linux, so Windows-gated code is **not** linted in CI, and Unix-gated code is not linted on a Windows dev box.
- When you touch platform-conditional code, check the *other* platform compiles before pushing — CI won't catch a Windows-only clippy regression.

### Secret handling

- Never log tokens, API keys, headers with auth material, or raw OAuth responses. Use the existing sanitised-log helpers in `mcpmux-gateway`.
- Credentials encrypt at rest via AES-256-GCM in SQLite plus DPAPI (Windows) / OS keychain (macOS, Linux). Don't add new code paths that persist secrets any other way.
- Secrets should be wiped from memory after use via `zeroize`.
- The gateway binds to `127.0.0.1`. Don't bind to `0.0.0.0` or expose it on the network.

## Frontend Notes

- Entry point: `apps/desktop/src/main.tsx` → `App.tsx`.
- Global state: a single Zustand store at `src/stores/appStore.ts`.
- Key hooks: `useServerManager` (server CRUD), `useSpaces` (workspace switching), `useDomainEvents` (Rust-side EventBus listener), `useDataSync`.
- UI: React 19, Tailwind CSS, Lucide icons, Monaco Editor for JSON config surfaces.
- Open external URLs through `openExternal` in `apps/desktop/src/lib/contribute.ts` — it routes through the Tauri opener plugin so links open in the user's default browser, not the webview.
- For UI changes, launch `pnpm dev` and exercise the feature in the running app before reporting done — typecheck and tests verify correctness, not UX regressions.

## Rust Architecture Cues

- Cross-layer communication goes through the `EventBus` in `mcpmux-core`. Prefer emitting a domain event over reaching across module boundaries directly.
- Storage is behind repository traits — don't call SQLx or SQLite APIs directly from gateway or app code; add or use a repo method.
- Services are wired up via the `ApplicationServices` builders in `mcpmux-core`. New services should follow the same DI pattern.

## MCP Specification

The full MCP spec is vendored at `../modelcontextprotocol/docs/specification/`. Default to the latest stable version (`2025-11-25`) and **read the relevant section before** implementing or modifying protocol behaviour (transports, lifecycle, capability negotiation, OAuth flows, tools / resources / prompts). For features targeting a specific protocol version, use that version's folder.

## Server Definitions

Server catalog entries live in the separate [`mcp-servers`](https://github.com/mcpmux/mcp-servers) repo — **not here**. If a task involves adding, editing, or fixing a server definition, switch to that repo and follow its `AGENTS.md`.

## Things Not To Do

- Don't add backwards-compatibility shims, deprecated aliases, or `// removed` placeholder comments when removing code — delete it cleanly.
- Don't introduce new fallbacks or input validation for states that are already framework-guaranteed. Trust internal invariants; validate only at the boundary (user input, external APIs).
- Don't edit generated files: `CHANGELOG.md`, release-please manifests, `bundle/*.json` in sibling repos, `packages/ui/dist`.
- Don't commit screenshots, videos, or large binaries to the repo — link out instead.
