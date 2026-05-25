# Web Admin Mode (Remote UI via HTTP)

**Last Updated:** May 25, 2026
**Status:** Planning
**Branch:** TBD — branch off `main` (or current fork head)
**Base branch:** `main`
**Issue:** TBD — file after planning review
**Depends on:** None for core admin HTTP layer; benefits from merged fork features (session meta-tools, account clones) but does not require them
**Unblocks:** [`jsg-tech-check` homelab wiring Step 6](../../../jsg-tech-check/docs/setup/home-lab-wiring-plan.md) — remote McpMux admin UI from Weathertop / Rohan at `https://mux.joe-hassio.com`

---

## Problem

The McpMux admin UI (Spaces, servers, credentials, workspace bindings, FeatureSets, OAuth consent) is a Tauri desktop app. The React frontend talks to Rust exclusively via Tauri `invoke()` — ~80 calls across 15 API modules, backed by ~110 Tauri commands in 19 command modules. There is no HTTP admin surface.

The homelab wiring plan already exposes two public endpoints via Cloudflare Tunnel on Gondor:

| Hostname | Target | What it serves |
| -------- | ------ | -------------- |
| `mcp.joe-hassio.com` | `localhost:45818` | MCP gateway (`/mcp`) for AI clients |
| `code.joe-hassio.com` | `localhost:3001` | ClaudeCodeUI |

Neither exposes the admin UI. Tunneling Vite dev (`:1420`) serves a React shell with no backend — every action fails because nothing answers `invoke()`. Tunneling the MCP gateway (`:45818`) serves the protocol endpoint, not admin pages.

The user-facing ask:

> I want to be able to reach the UI — that's the main point.

Screen sharing / VNC behind CF Access works today but is not a web UI. This doc defines a **web admin mode**: an optional HTTP server that serves the built React SPA and exposes a REST API mirroring Tauri commands, gated by Cloudflare Access at the edge.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Deployment model | **Single-user homelab** — one McpMux instance on Gondor, one operator | Avoids multi-tenant auth, cloud KMS, and per-user DB isolation. The Rust process still runs locally with OS keychain access. |
| 2 | Auth | **Cloudflare Access at the tunnel edge** — app trusts `CF-Access-Jwt-Assertion` when `gateway.admin_trust_cf_access` is enabled | No login UI to build. Same pattern as `b.joe-hassio.com` (Beeper). Reject requests without a valid JWT when admin mode is enabled. |
| 3 | Admin server placement | **Separate Axum router on configurable port** (default `45819`), not mixed into MCP gateway routes | Keeps MCP protocol surface unchanged. Admin and MCP can be tunneled independently (`mux.joe-hassio.com` vs `mcp.joe-hassio.com`). Easier to disable admin without stopping the gateway. |
| 4 | Static UI | **Serve `frontendDist` from the Tauri build** at `/` with SPA fallback | Reuses the existing React app. No separate web bundle. |
| 5 | API shape | **REST JSON at `/api/v1/*`** mirroring Tauri command names (kebab → snake mapping) | Predictable mapping: `get_gateway_status` → `GET /api/v1/gateway/status`. One handler module per Tauri command group. |
| 6 | Frontend transport | **Transport abstraction in `lib/api/`** — `invoke()` in Tauri, `fetch()` in web mode | Detect via `window.__TAURI__` or build-time `import.meta.env.VITE_ADMIN_WEB`. Same function signatures, different backend. |
| 7 | OAuth consent | **Re-enable guarded HTTP consent endpoint** for web admin only — `POST /api/v1/oauth/consent/approve` behind CF Access + CSRF token | Production desktop keeps Tauri-IPC-only consent (existing security model). Web mode needs an HTTP path because there is no Tauri shell on Weathertop. |
| 8 | Bind address | **Default `127.0.0.1:45819`** — same loopback-first posture as MCP gateway | CF tunnel reaches localhost; no need to bind `0.0.0.0`. `AGENTS.md` loopback rule preserved. |
| 9 | Event streaming | **SSE at `/api/v1/events`** bridging existing `EventBus` | Replaces Tauri event listeners (`useDomainEvents`) in web mode. Desktop keeps Tauri events. |
| 10 | Scope phasing | **Read-only views first, then writes, then OAuth** | Each phase is independently testable behind CF Access. Avoids a big-bang API dump. |

---

## The Model

### What web admin mode is

An optional HTTP server started alongside (or instead of) the Tauri window when `gateway.admin_enabled` is true:

```text
AdminServer (Axum, :45819)
├── GET  /*                    → SPA static files (frontendDist)
├── GET  /api/v1/health       → { status: "ok", gateway_running: bool }
├── GET  /api/v1/events       → SSE stream (EventBus bridge)
├── /api/v1/gateway/*         → gateway commands
├── /api/v1/spaces/*          → space commands
├── /api/v1/servers/*         → server manager + install + clone
├── /api/v1/workspaces/*      → workspace bindings + session overrides
├── /api/v1/feature-sets/*    → feature sets + members
├── /api/v1/clients/*         → inbound MCP clients
├── /api/v1/oauth/*           → consent approve/reject (web only)
└── /api/v1/settings/*        → app settings
```

All handlers delegate to the same `ApplicationServices` / command-layer logic Tauri uses today — no duplicated business logic.

### What web admin mode is NOT

- Not a hosted multi-tenant SaaS ("McpMux Cloud")
- Not a replacement for the Tauri desktop app on Gondor (desktop remains primary for local use)
- Not exposing the MCP gateway without separate hardening (that route stays on `:45818` with its own OAuth JWT model)
- Not moving secrets off OS keychain — encryption keys stay local

### Homelab tunnel layout (target)

```yaml
# gondor cloudflared config (addition to home-lab-wiring-plan.md Step 5)
ingress:
  - hostname: mux.joe-hassio.com
    service: http://localhost:45819    # NEW — admin UI
  - hostname: mcp.joe-hassio.com
    service: http://localhost:45818    # existing — MCP clients
  - hostname: code.joe-hassio.com
    service: http://localhost:3001     # existing — ClaudeCodeUI
  - service: http_status:404
```

CF Access policy on `mux.joe-hassio.com`: allow `jsangio1@gmail.com` (or equivalent Zero Trust rule).

---

## Architecture

```
Weathertop / Rohan browser
        │
        │ HTTPS + CF Access (Google login)
        ▼
  mux.joe-hassio.com ──── cloudflared tunnel ────► localhost:45819
                                                          │
                              ┌───────────────────────────┤
                              │                           │
                              ▼                           ▼
                    ┌──────────────────┐        ┌──────────────────┐
                    │  Static SPA      │        │  /api/v1/* REST  │
                    │  (frontendDist)  │        │  + SSE /events   │
                    └──────────────────┘        └────────┬─────────┘
                                                         │
                                                         ▼
                                              ┌──────────────────────┐
                                              │ ApplicationServices  │
                                              │ (same as Tauri cmds) │
                                              └──────────┬───────────┘
                                                         │
                    ┌────────────────────────────────────┼────────────────────┐
                    ▼                                    ▼                    ▼
              SQLite +                           OS Keychain              Gateway :45818
              AES-256-GCM                          JWT secret              (unchanged)
```

**Middleware stack (admin router):**

1. `CF-Access-Jwt-Assertion` validation (when enabled)
2. CSRF token check on mutating routes (web OAuth consent)
3. Request logging (sanitized — no secrets)
4. CORS: deny by default; allow same-origin only (SPA served from same host)

**Frontend transport switch:**

```typescript
// lib/api/transport.ts (new)
export async function apiCall<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    return invoke(command, args);
  }
  return fetchApi<T>(command, args);
}
```

Existing `lib/api/*.ts` modules swap `invoke(...)` → `apiCall(...)` with no signature changes.

---

## Files to create

| File | Purpose |
| ---- | ------- |
| `crates/mcpmux-gateway/src/admin/mod.rs` | Admin router module entry |
| `crates/mcpmux-gateway/src/admin/server.rs` | `AdminServer` — bind, static file serving, route mounting |
| `crates/mcpmux-gateway/src/admin/middleware/cf_access.rs` | Validate `CF-Access-Jwt-Assertion` against CF team domain certs |
| `crates/mcpmux-gateway/src/admin/middleware/csrf.rs` | CSRF token generation + validation for mutating routes |
| `crates/mcpmux-gateway/src/admin/handlers/mod.rs` | Handler module tree |
| `crates/mcpmux-gateway/src/admin/handlers/gateway.rs` | Gateway status/start/stop REST handlers |
| `crates/mcpmux-gateway/src/admin/handlers/spaces.rs` | Space CRUD handlers |
| `crates/mcpmux-gateway/src/admin/handlers/servers.rs` | Server manager + install + clone handlers |
| `crates/mcpmux-gateway/src/admin/handlers/workspaces.rs` | Workspace binding + session override handlers |
| `crates/mcpmux-gateway/src/admin/handlers/feature_sets.rs` | FeatureSet + member handlers |
| `crates/mcpmux-gateway/src/admin/handlers/clients.rs` | Inbound MCP client handlers |
| `crates/mcpmux-gateway/src/admin/handlers/oauth.rs` | Web consent approve/reject handlers |
| `crates/mcpmux-gateway/src/admin/handlers/settings.rs` | App settings handlers |
| `crates/mcpmux-gateway/src/admin/handlers/events.rs` | SSE EventBus bridge |
| `crates/mcpmux-gateway/src/admin/command_bridge.rs` | Shared helper: call Tauri command logic without Tauri runtime |
| `apps/desktop/src/lib/api/transport.ts` | Tauri vs fetch transport abstraction |
| `apps/desktop/src/lib/api/fetch-api.ts` | REST client mapping command names → HTTP paths |
| `apps/desktop/src/hooks/useDomainEventsWeb.ts` | SSE-based event listener for web mode |
| `tests/rust/tests/integration/admin_api.rs` | Admin API integration tests (health, auth rejection, read endpoints) |
| `docs/planning/web-admin-remote-access.md` | This doc |

## Files to modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-gateway/src/lib.rs`](../../crates/mcpmux-gateway/src/lib.rs) | `pub mod admin;` |
| [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) | `GatewayConfig` gains `admin_enabled`, `admin_port`, `admin_trust_cf_access`, `admin_cf_team_domain` |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Start `AdminServer` when setting enabled; share `ApplicationServices` Arc |
| [`apps/desktop/src-tauri/src/commands/gateway.rs`](../../apps/desktop/src-tauri/src/commands/gateway.rs) | Extract shared gateway logic callable from admin handlers |
| [`apps/desktop/src/lib/api/*.ts`](../../apps/desktop/src/lib/api/) | Replace direct `invoke()` with `apiCall()` from transport layer |
| [`apps/desktop/src/hooks/useDomainEvents.ts`](../../apps/desktop/src/hooks/useDomainEvents.ts) | Delegate to SSE hook in web mode |
| [`apps/desktop/src/features/oauth/OAuthConsentModal.tsx`](../../apps/desktop/src/features/oauth/OAuthConsentModal.tsx) | Web mode: POST to `/api/v1/oauth/consent/approve` instead of Tauri command |
| [`apps/desktop/src/features/settings/SettingsPage.tsx`](../../apps/desktop/src/features/settings/SettingsPage.tsx) | Admin mode toggle + port setting |
| [`apps/desktop/vite.config.ts`](../../apps/desktop/vite.config.ts) | `VITE_ADMIN_WEB` build flag for web-only builds |
| [`apps/desktop/package.json`](../../apps/desktop/package.json) | `build:web:admin` script — production SPA build for admin serving |
| [`tests/e2e/playwright.config.ts`](../../tests/e2e/playwright.config.ts) | Optional admin web E2E project against `:45819` with mocked CF JWT |
| [`AGENTS.md`](../../AGENTS.md) | Document admin server loopback binding + CF Access requirement |

---

## Phasing

### Phase 1 — Admin server skeleton + static SPA + CF Access gate

**Effort:** ~2 days

- [ ] `AdminServer` Axum router on `127.0.0.1:45819` (configurable)
- [ ] Serve `frontendDist` with SPA fallback (`index.html` for unknown routes)
- [ ] `GET /api/v1/health` — returns gateway running status
- [ ] CF Access middleware: validate `CF-Access-Jwt-Assertion` when `admin_trust_cf_access` is true; 401 without it
- [ ] Settings: `gateway.admin_enabled` (default `false`), `gateway.admin_port` (default `45819`)
- [ ] Start admin server from Tauri app when setting enabled (alongside gateway)
- [ ] Unit test: health endpoint returns 200; unauthenticated request returns 401 when CF Access enabled

**Outcome:** With admin mode enabled and a valid CF Access JWT, `https://mux.joe-hassio.com` (via tunnel) loads the McpMux UI shell. API calls still fail (no handlers yet), but static assets render and auth gate works.

### Phase 2 — Transport abstraction + read-only API

**Effort:** ~3 days

- [ ] `transport.ts` + `fetch-api.ts` — command name → HTTP path mapping
- [ ] Refactor all `lib/api/*.ts` modules to use `apiCall()`
- [ ] `command_bridge.rs` — extract shared logic from Tauri commands into functions callable from both IPC and HTTP
- [ ] Read-only handlers: gateway status, list spaces, list installed servers, list workspace bindings, list feature sets, list clients, list session overrides, get settings
- [ ] `GET /api/v1/events` — SSE bridge from `EventBus`
- [ ] `useDomainEventsWeb.ts` — SSE listener; `useDomainEvents` switches on environment
- [ ] Integration tests for each read endpoint

**Outcome:** From Weathertop, authenticated user can browse Spaces, My Servers, Workspaces, FeatureSets, Clients, and Settings in read-only mode. Domain events (server connected, gateway started) stream via SSE. No writes yet.

### Phase 3 — Write API (config mutations)

**Effort:** ~4 days

- [ ] Write handlers: install/uninstall server, enable/disable, configure inputs, clone server, CRUD spaces, CRUD workspace bindings, CRUD feature sets + members, gateway start/stop, export config, clear session overrides, update settings
- [ ] CSRF middleware on all `POST`/`PUT`/`DELETE` routes
- [ ] Error mapping: domain errors → HTTP status codes with JSON body
- [ ] Integration tests: install + configure + enable round-trip via HTTP
- [ ] Playwright admin web E2E: smoke test install flow against `:45819`

**Outcome:** Full admin CRUD works from the browser. User can install servers, edit credentials, manage bindings and FeatureSets, start/stop gateway — all remote. OAuth consent still requires Phase 4.

### Phase 4 — Web OAuth consent

**Effort:** ~2 days

- [ ] `POST /api/v1/oauth/consent/approve` and `/reject` — guarded HTTP endpoints (web admin only; desktop keeps Tauri IPC)
- [ ] CSRF + consent token validation (reuse existing cryptographic consent token from gateway)
- [ ] `OAuthConsentModal.tsx` — web path posts to HTTP endpoint; desktop path unchanged
- [ ] Deep link bridge: `mcpmux://` URLs on Gondor still work for desktop; web mode polls consent pending state via SSE
- [ ] Integration test: OAuth authorize → consent approve via HTTP → token issued

**Outcome:** Remote user can complete OAuth flows (Notion, GitHub, Google Workspace, etc.) from the browser without needing the Tauri shell on Gondor.

### Phase 5 — Homelab integration + docs

**Effort:** ~1 day

- [ ] Update [`jsg-tech-check/docs/setup/home-lab-wiring-plan.md`](../../../jsg-tech-check/docs/setup/home-lab-wiring-plan.md) Step 5 with `mux.joe-hassio.com` ingress rule
- [ ] Document CF Access policy setup for `mux.joe-hassio.com`
- [ ] Add admin mode section to [`docs/guide/gateway.mdx`](../../docs/guide/gateway.mdx)
- [ ] `pnpm build:web:admin` + verify production SPA served correctly from admin server
- [ ] End-to-end smoke from Weathertop: open `https://mux.joe-hassio.com`, manage a server, approve OAuth

**Outcome:** Homelab wiring plan reflects the third public hostname. Operator can manage McpMux from phone/laptop browser with CF Access auth.

---

## Pre-PR validation

| Step | Command | Purpose |
| ---- | ------- | ------- |
| Full validate | `pnpm validate` | fmt, clippy, check, eslint, typecheck |
| Rust tests | `pnpm test:rust` | unit + integration (`admin_api.rs`) |
| TS tests | `pnpm test:ts` | vitest (transport layer) |
| Admin web E2E | `pnpm test:e2e:web:admin` (new) | Playwright against `:45819` |
| Manual smoke | Enable admin mode, tunnel `mux.joe-hassio.com`, browse from phone | UX + CF Access verification |

---

## Out of scope

| Item | Reason |
| ---- | ------ |
| Multi-tenant / per-user accounts | Single-user homelab. Adding user management is a different product. |
| Cloud KMS / secrets off OS keychain | Admin server runs on Gondor; keychain access is preserved. No remote secret vault needed. |
| Binding admin server to `0.0.0.0` | Loopback + CF tunnel is the access path. Direct internet bind violates `AGENTS.md` posture. |
| Replacing Tauri desktop app | Desktop remains primary on Gondor. Web admin is for remote access only. |
| Mobile-optimized responsive UI | React app works in mobile browser but no dedicated mobile layout pass. Acceptable for v1 homelab use. |
| Public MCP gateway hardening (`mcp.joe-hassio.com`) | Separate concern — OAuth JWT auth exists but unauthenticated admin routes on `:45818` need CF Access too. Track as follow-up, not blocked on this doc. |
| WebSocket transport (instead of SSE) | SSE is sufficient for EventBus fan-out. WebSocket adds complexity with no v1 benefit. |
| Headless-only mode (no Tauri window) | v1 starts admin server from Tauri app. Headless/systemd mode is a follow-up for Rivendell-style deployment. |

---

## Key files referenced

| File | Why |
| ---- | --- |
| [`apps/desktop/src/lib/api/gateway.ts`](../../apps/desktop/src/lib/api/gateway.ts) | Largest API module (~20 invokes) — template for transport refactor |
| [`apps/desktop/src-tauri/src/commands/mod.rs`](../../apps/desktop/src-tauri/src/commands/mod.rs) | Command module registry — each module gets a corresponding admin handler |
| [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) | Existing Axum gateway — pattern reference for admin router |
| [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) (lines 340–365) | OAuth consent removed from HTTP for security — web admin re-adds guarded version |
| [`apps/desktop/src/hooks/useDomainEvents.ts`](../../apps/desktop/src/hooks/useDomainEvents.ts) | Tauri event listener — needs SSE equivalent for web mode |
| [`jsg-tech-check/docs/setup/home-lab-wiring-plan.md`](../../../jsg-tech-check/docs/setup/home-lab-wiring-plan.md) | CF tunnel config — gets `mux.joe-hassio.com` ingress in Phase 5 |

---

## Related documentation

- [`jsg-tech-check/docs/setup/home-lab-wiring-plan.md`](../../../jsg-tech-check/docs/setup/home-lab-wiring-plan.md) — Step 5 (CF tunnel), Step 6 (McpMux on Gondor), cross-device MCP access
- [`jsg-tech-check/docs/setup/mcpmux-server-migration.md`](../../../jsg-tech-check/docs/setup/mcpmux-server-migration.md) — server/bundle/binding migration tracker (orthogonal to web admin)
- [`docs/guide/security.mdx`](../../docs/guide/security.mdx) — credential encryption model (unchanged by web admin)
- [`docs/planning/dynamic-mcp-toggle-meta-tools.md`](./dynamic-mcp-toggle-meta-tools.md) — session override UI that web admin must expose via HTTP
- [`docs/planning/server-account-clones.md`](./server-account-clones.md) — clone wizard that web admin must expose via HTTP

---

## Reconciliation

This doc is the source of truth for web admin mode. When implementation starts, update **Status** and **Branch** at the top. Phase 5 homelab doc updates live in `jsg-tech-check` — track cross-repo separately.

**Decision record (May 25, 2026):** Web admin mode on fork selected over screen sharing (immediate but not web UI), tunneling `:1420` (broken), and full "McpMux Cloud" multi-tenant SaaS (months of work). CF Access at edge replaces building login UI. Separate admin port (`45819`) keeps MCP gateway surface unchanged.
