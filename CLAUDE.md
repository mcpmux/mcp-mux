# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Codebase Search

This repository is indexed for semantic code search via the `claude-context` MCP server. Prefer `mcp__claude-context__search_code` (with path `D:\mcpmux`) over Grep/Glob when looking for implementations, understanding how features work, or finding related code across the codebase. Use Grep/Glob for exact string matches or filename patterns. If search returns an indexing error, re-index with `mcp__claude-context__index_codebase` first.

## What is McpMux

McpMux is a desktop app + local gateway that lets users configure MCP servers once and connect every AI client (Cursor, Claude Desktop, VS Code, Windsurf) through a single `localhost:45818` endpoint. Credentials are encrypted in the OS keychain instead of plain-text JSON files.

## Repository Structure

This is a multi-project workspace with 6 independent projects (not a unified monorepo):

| Project | Tech | Purpose |
|---------|------|---------|
| `mcp-mux/` | Tauri 2 (Rust + React 19), pnpm workspace | Desktop app + local MCP gateway |
| `mcpmux.bundler/` | Cloudflare Worker | GitHub webhook receiver that processes server definitions into D1/R2 |
| `mcpmux.serverhub.api/` | Cloudflare Worker (Hono) | REST API for server registry discovery (KV -> D1 -> R2 fallback chain) |
| `mcpmux.discover.ui/` | Next.js 16, shadcn/ui, Tailwind 4 | Web UI for browsing the server registry |
| `mcp-servers/` | JSON + AJV validation | Community MCP server definitions repository |
| `mcpmux.space/` | Docs | Documentation and design space |

## mcp-mux (Desktop App) - Main Project

### Build & Dev Commands

All commands run from `mcp-mux/`:

```bash
pnpm setup              # First-time dev environment setup (PowerShell)
pnpm dev                # Tauri desktop app dev mode (Rust + React hot-reload)
pnpm dev:web            # Web UI only (Vite, no Rust)
pnpm build              # Production Tauri build (all platforms)
```

### Testing

```bash
pnpm test               # All tests (Rust + TypeScript)
pnpm test:rust          # cargo nextest run --workspace
pnpm test:rust:unit     # cargo nextest run --workspace --lib
pnpm test:rust:int      # cargo nextest run -p tests
pnpm test:rust:doc      # cargo test --workspace --doc
pnpm test:ts            # vitest run -c tests/ts/vitest.config.ts
pnpm test:ts:watch      # vitest watch mode
pnpm test:e2e           # WebDriver IO desktop E2E (needs MCPMUX_REGISTRY_URL)
pnpm test:e2e:file      # Single E2E spec: pnpm test:e2e:file -- tests/e2e/specs/foo.ts
pnpm test:e2e:grep      # E2E by name: pnpm test:e2e:grep -- "test name"
pnpm test:e2e:web       # Playwright web UI E2E
pnpm test:coverage      # cargo llvm-cov + vitest coverage
```

### Linting & Validation

```bash
pnpm validate           # Full check: cargo fmt + clippy + check + eslint + typecheck
pnpm lint               # ESLint (recursive) + cargo clippy --workspace -- -D warnings
pnpm lint:fix           # Auto-fix lint issues
pnpm format             # prettier --write . && cargo fmt --all
pnpm format:check       # Check formatting without modifying
pnpm typecheck          # TypeScript type checking (recursive)
```

### Rust Crate Architecture

The Cargo workspace has 4 library crates + 1 app crate + 1 test crate:

- **mcpmux-core** (`crates/mcpmux-core/`) - Domain layer: entities (Space, InstalledServer, FeatureSet, Client), repository traits, domain services, application services with event emission, and the central EventBus
- **mcpmux-gateway** (`crates/mcpmux-gateway/`) - Axum HTTP gateway: routes MCP calls to correct servers, manages OAuth 2.1+PKCE token refresh, filters tools/resources/prompts based on FeatureSets, per-client access key auth, server connection pooling
- **mcpmux-storage** (`crates/mcpmux-storage/`) - SQLite persistence with AES-256-GCM field-level encryption via ring, typed credential rows (per-token encryption), DPAPI key storage on Windows (`keychain_dpapi.rs`), OS keychain on macOS/Linux via keyring crate, zeroize for secure memory clearing
- **mcpmux-mcp** (`crates/mcpmux-mcp/`) - MCP protocol client management using rmcp SDK
- **apps/desktop/src-tauri** - Tauri 2 app shell, Tauri commands, system tray, deep-link handler (`mcpmux://`)
- **tests/rust** - Integration test crate

Key patterns: event-driven architecture (EventBus), repository pattern (trait-based storage abstraction), service layer pattern with DI via ApplicationServices builders.

### Frontend Architecture

- **Entry**: `apps/desktop/src/main.tsx` -> `App.tsx`
- **State**: Zustand store (`stores/appStore.ts`)
- **Hooks**: `useServerManager` (server CRUD), `useSpaces` (workspace switching), `useDomainEvents` (Rust event listeners), `useDataSync` (data synchronization)
- **UI**: React 19 + Tailwind CSS + Lucide icons + Monaco Editor (config editing)
- **Path aliases**: `@/` -> `src/`, `@mcpmux/ui` -> shared UI package

### Data Flow

```
AI Clients -> McpMux Gateway (localhost:45818/mcp) -> MCP Servers (stdio/HTTP)
                     |
              Authenticates (access keys)
              Routes (per-space server config)
              Filters (FeatureSet permissions)
              OAuth token refresh (automatic)
              Credentials (DPAPI files on Windows / OS Keychain on macOS+Linux + encrypted SQLite)
```

### Prerequisites

Rust 1.75+, Node.js 20+, pnpm 9+. Linux: `gnome-keyring libsecret-1-dev librsvg2-dev pkg-config`.

### Code Style

- Rust: 100 char max width, 4-space indent, `clippy` with `avoid-breaking-exported-api = false`
- TypeScript/JSX: Prettier with single quotes, 2-space indent, 100 char width, trailing commas (es5), Tailwind CSS plugin
- Commits require `Signed-off-by` line (use `git commit -s`)

## mcp-servers (Server Definitions Repository)

All commands run from `mcp-servers/` (sibling repo):

```bash
pnpm install            # Install dependencies (vitest, ajv, glob)
pnpm validate:all       # Validate all server definitions against JSON schema
pnpm validate <file>    # Validate specific server definition file(s)
pnpm build              # Build registry bundle (bundle/bundle.json)
pnpm test               # Run all tests (schema validation, consistency, categories, bundle)
```

The JSON schema at `schemas/server-definition.schema.json` defines the structure for server definitions in `servers/`. Input definitions support: `id`, `label`, `type`, `required`, `secret`, `description`, `default`, `placeholder`, and `obtain` (with `url`, `instructions`, `button_label`).

## Cloudflare Workers

Each worker is an independent project with its own `package.json`:

**mcpmux.serverhub.api/** - Hono-based registry API:
```bash
cd mcpmux.serverhub.api && pnpm dev    # wrangler dev
cd mcpmux.serverhub.api && pnpm test   # vitest (cloudflare pool)
```

**mcpmux.bundler/** - Webhook-triggered bundle processor:
```bash
cd mcpmux.bundler && pnpm dev          # wrangler dev
cd mcpmux.bundler && pnpm test         # vitest run
```

## mcpmux.discover.ui (Discovery Web UI)

```bash
cd mcpmux.discover.ui && pnpm dev      # next dev
cd mcpmux.discover.ui && pnpm test     # vitest
cd mcpmux.discover.ui && pnpm test:e2e # playwright
```

## Important Patterns

### Child Process Platform Flags

When spawning child processes (e.g., stdio MCP servers), **always** use `configure_child_process_platform()` from `mcpmux_gateway::pool::transport`. This applies:
- **Windows**: `CREATE_NO_WINDOW` (`0x08000000`) — prevents visible console windows in release builds (where `windows_subsystem = "windows"` means the app is a GUI subsystem process)
- **Unix**: `process_group(0)` — isolates child from parent terminal signals (SIGINT, SIGTSTP)

Note: `tokio::process::Command` already exposes `creation_flags()` (Windows) and `process_group()` (Unix) natively — do **not** import `std::os::unix::process::CommandExt` or `std::os::windows::process::CommandExt` as the traits are unused with Tokio's Command.

### Cross-Platform CI Awareness

The pre-commit hook runs `cargo clippy --workspace -- -D warnings` locally, but `#[cfg(unix)]` / `#[cfg(windows)]` blocks are only compiled on the matching platform. **CI runs on Linux**, so:
- `#[cfg(unix)]` code is only linted in CI, not on a Windows dev machine
- `#[cfg(windows)]` code is only linted locally on Windows, not in CI
- Always validate that platform-conditional code compiles correctly on both platforms before pushing

## CI

GitHub Actions runs on push/PR to main: Rust format + clippy + check, ESLint + typecheck, cargo nextest, vitest, desktop E2E (Windows/macOS/Linux), web E2E (Playwright). Releases use release-please for semantic versioning with multi-platform Tauri builds.
