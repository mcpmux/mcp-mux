# Web Admin Completion

**Last Updated:** Jun 24, 2026
**Status:** Active — pending implementation
**Branch:** `dev-rebased` (HEAD: `17d71ad`)
**Depends on:** Phase 2 lib/api migration complete (`dev-rebased-post-port-completion.md`), admin SSE hub live
**Unblocks:** Web admin at `mux.joe-hassio.com` fully functional with no `transformCallback` errors, config export, live consent flow, and working HMR dev path

---

## Problem

Three phases of `invoke` → `apiCall` migration landed cleanly. The web admin loads spaces, data syncs, SSE events flow, and the console is clean of `transformCallback` spam. Five gaps remain before the web admin is feature-complete:

**1. `pnpm dev:web:admin` is broken.** `scripts/dev-env.mjs` does not exist (referenced by `dev-web-admin.mjs:34`). Even if the prep call is removed, Vite at `:1420` has no `/api` proxy configured — API calls fall through to the Vite dev server which has no handler.

**2. Config export has zero Rust routes.** Five commands in `lib/api/configExport.ts` (`preview_config_export`, `get_config_paths`, `check_config_exists`, `backup_existing_config`, `export_config_to_file`) are mapped in `fetch-api.routes/` but have no handlers in `router.rs` or `command_bridge/`. All five throw 404 in web admin.

**3. OAuth consent popup is dead on web.** `oauth-consent-request` and `oauth-client-changed` are emitted via `app.emit()` (Tauri IPC only) in `oauth.rs`. The admin SSE hub never receives them, so the consent modal and client-changed events are silently dropped in the browser.

**4. Native file pickers crash on web.** `SpaceBaseDirsModal.tsx` calls `@tauri-apps/plugin-dialog` `openDialog()` with no `isTauri()` guard. `pickPath()` in `shell/index.ts` (used by ServersPage, WorkspacesPage) returns `undefined` in browser context but the callers never expect that.

**5. Dead `apiCall` commands pollute the codebase.** Three commands (`export_config`, `connect_server`, `disconnect_server_v2`) are called in `lib/api/` but have no corresponding feature usage and are superseded by newer commands. They add noise to the transport layer and route coverage tests.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Dev HMR path | Fix properly — `scripts/dev-env.mjs` + Vite `/api` proxy to `:45819` | HMR is the primary iteration loop. Dead tooling slows down future web admin work. |
| 2 | Config export on web | Full implementation — all 5 Rust command bridges | Config export is same-filesystem on the local machine. No upload/download needed; file-write semantics are identical to desktop. |
| 3 | OAuth consent events | Route through `emit_ui_channel()` | Consent request is an active MCP session flow. Web admin connecting a server must be able to complete OAuth. SSE already carries other session events; reuse the pattern. |
| 4 | File pickers on web | `isTauri()` guard → native picker, else `<input type="text">` | Minimal change. Path entry by text is fully functional and avoids a new multipart upload API. |
| 5 | Dead commands | Delete callers + Tauri commands | No feature code calls them. Deletion is cleaner than wiring unused routes. |

---

## Scope

**In:**
- Write `scripts/dev-env.mjs` (gateway liveness check); add `/api` Vite proxy when `VITE_ADMIN_WEB=1`
- Implement 5 config-export Rust handlers in `command_bridge/read.rs` + `router.rs`
- Route `oauth-consent-request` and `oauth-client-changed` through `emit_ui_channel()`
- Guard `SpaceBaseDirsModal` `openDialog()` and `pickPath()` call sites with text-input fallback
- Delete `export_config`, `connect_server`, `disconnect_server_v2` callers and Tauri commands
- Extend `tests/ts/admin-transport.test.ts` to cover builtins, config-export, and the new routes
- Align `builtin-server-config-changed` SSE channel name (desktop bridge uses this name; `ui_events.rs` maps to `server-changed`)

**Out:**

| Item | Reason |
| ---- | ------ |
| Browser file-upload for base dirs / icons | Punted — text input is sufficient for local paths on the same machine |
| Connect IDE flow on web | Desktop-only by design; requires Tauri deep-link to IDE |
| Meta-tools approval dialog on web | Desktop-only modal; approval via Settings toggle works on web |
| Cloudflare `Permissions-Policy` header noise | External to this repo — fix belongs in CF Zero Trust dashboard (remove `Permissions-Policy: *` from the Access application response headers) |
| Web E2E test authoring for new routes | Deferred — manual verification matrix covers this pass |

---

## Architecture

### Dev tooling

```
pnpm dev:web:admin
  └─ node scripts/dev-web-admin.mjs
       ├─ runPrep() → scripts/dev-env.mjs   ← NEW: health-check :45819
       └─ execa vite (VITE_ADMIN_WEB=1)
            └─ vite.config.ts server.proxy['/api'] → http://127.0.0.1:45819
```

`vite.config.ts` conditionally adds the proxy only when `process.env.VITE_ADMIN_WEB` is set so Tauri dev builds are unaffected.

### Config export Rust bridges

New handlers land in `crates/mcpmux-gateway/src/admin/command_bridge/read.rs` alongside existing read handlers. Each handler calls the same underlying `ApplicationServices` method that the Tauri command already calls. Router mounts them under `/api/v1/config-export/`.

### OAuth SSE fan-out

`oauth.rs` currently calls `app.emit("oauth-consent-request", ...)` and `app.emit("oauth-client-changed", ...)`. Change to `emit_ui_channel(&app, UiEvent::OAuthConsentRequest { ... })` which fans out to both Tauri IPC and the admin SSE hub simultaneously. No new SSE channels needed — SSE already carries `oauth-consent-request` and `oauth-client-changed` mappings via `ui_events.rs`.

---

## Files to create / modify

| Phase | File | Action |
| ----- | ---- | ------ |
| 1 | `scripts/dev-env.mjs` | **Create** — gateway liveness check, port guard |
| 1 | `scripts/dev-web-admin.mjs` | Update `runPrep()` to call `dev-env.mjs` |
| 1 | `apps/desktop/vite.config.ts` | Add `/api` proxy block when `VITE_ADMIN_WEB` |
| 2 | `crates/mcpmux-gateway/src/admin/command_bridge/read.rs` | Add 5 config-export handlers |
| 2 | `crates/mcpmux-gateway/src/admin/router.rs` | Mount `/api/v1/config-export/*` routes |
| 3 | `apps/desktop/src-tauri/src/services/oauth.rs` | `app.emit` → `emit_ui_channel` for consent + client-changed |
| 3 | `apps/desktop/src-tauri/src/services/ui_events.rs` | Verify `OAuthConsentRequest` / `OAuthClientChanged` variants exist; add if missing |
| 3 | `apps/desktop/src-tauri/src/services/ui_events.rs` | Align `builtin-server-config-changed` channel: emit under same name in both Tauri bridge and SSE |
| 4 | `apps/desktop/src/features/spaces/SpaceBaseDirsModal.tsx` | Guard `openDialog()` with `isTauri()`; replace with `<input type="text">` on web |
| 4 | `apps/desktop/src/features/servers/ServersPage.tsx` | Guard `pickPath()` — show text input on web |
| 4 | `apps/desktop/src/features/workspaces/WorkspacesPage.tsx` | Guard `pickPath()` for icon upload — show text input on web |
| 5 | `apps/desktop/src/lib/api/gateway.ts` | Delete `export_config`, `connect_server` callers |
| 5 | `apps/desktop/src/lib/api/serverManager.ts` | Delete `disconnect_server_v2` caller |
| 5 | `apps/desktop/src-tauri/src/commands/` | Remove corresponding Tauri commands if no other callers |
| 5 | `apps/desktop/src/lib/backend/data/fetch-api.routes/` | Remove dead route mappings for deleted commands |
| 5 | `tests/ts/admin-transport.test.ts` | Add builtins, config-export, new SSE event coverage |

---

## Phases

### Phase 1 — Dev tooling (~1 hour)

- Write `scripts/dev-env.mjs`: check `:45819/api/v1/health`, fail fast with a clear message if gateway is not running, else exit 0
- Update `scripts/dev-web-admin.mjs` to call `dev-env.mjs` via `execa`
- Add conditional Vite proxy in `apps/desktop/vite.config.ts`:
  ```ts
  ...(process.env.VITE_ADMIN_WEB ? { server: { proxy: { '/api': 'http://127.0.0.1:45819' } } } : {})
  ```

**Outcome:** `pnpm dev:web:admin` starts Vite HMR at `:1420`. API calls proxy through to the running gateway. Stopping with the gateway down prints a helpful "gateway not running" message instead of a missing-module crash.

---

### Phase 2 — Config export HTTP routes (~half day)

Add five read handlers in `command_bridge/read.rs`:

- `preview_config_export(space_id)` → `GET /api/v1/config-export/preview?space_id=`
- `get_config_paths(space_id)` → `GET /api/v1/config-export/paths?space_id=`
- `check_config_exists(client_name)` → `POST /api/v1/config-export/check`
- `backup_existing_config(client_name)` → `POST /api/v1/config-export/backup`
- `export_config_to_file(space_id, client_name, path)` → `POST /api/v1/config-export/export`

Mount all five in `router.rs` under `/api/v1/config-export/`. Follow the existing `BridgeContext` + `ApplicationServices` handler pattern from adjacent read handlers.

**Outcome:** Config export UI fully functional in web admin. Preview shows generated config, paths reports target locations, backup and export write to the server filesystem. `pnpm validate` clean.

---

### Phase 3 — OAuth SSE fan-out + builtin channel alignment (~half day)

- In `apps/desktop/src-tauri/src/services/oauth.rs`, replace `app.emit("oauth-consent-request", ...)` and `app.emit("oauth-client-changed", ...)` with `emit_ui_channel(...)` calls so events reach both Tauri IPC and the admin SSE hub
- Verify `ui_events.rs` has matching enum variants for `OAuthConsentRequest` and `OAuthClientChanged`; add if missing
- Fix `builtin-server-config-changed` channel name: `ui_events.rs` currently emits this as `server-changed` (via `map_domain_event_to_ui`); change the SSE mapping to emit `builtin-server-config-changed` so `BuiltinServersPage` live-refreshes without a page reload

**Outcome:** OAuth consent modal fires in web admin when a server needs authorization. Client grant changes reflect live without refresh. Builtin server enable/disable toggles update the page immediately via SSE.

---

### Phase 4 — Web-native file picker fallback (~half day)

For each call site that uses `@tauri-apps/plugin-dialog` `openDialog()` or `shell/index.ts` `pickPath()`:

- `SpaceBaseDirsModal.tsx` — wrap `openDialog()` call in `if (isTauri())` block; render `<input type="text" placeholder="Enter absolute path" />` in the `else` branch
- `ServersPage.tsx` — wrap `pickPath()` usage similarly; the text input value feeds the same state setter
- `WorkspacesPage.tsx` — same pattern for icon path entry

No new API endpoints. The text input path value is passed to the existing `apiCall` command that was already accepting a string.

**Outcome:** Base dirs modal opens in web admin with a text field instead of a native picker. Server and workspace path fields render a text input. No crash on `openDialog()`. Desktop Tauri behaviour unchanged.

---

### Phase 5 — Dead code cleanup + test coverage (~1 hour)

- Delete `export_config` caller from `gateway.ts` (superseded by `export_config_to_file`)
- Delete `connect_server` caller from `gateway.ts` (superseded by `enable_server_v2`)
- Delete `disconnect_server_v2` caller from `serverManager.ts` (superseded by `disconnect_server`)
- Remove corresponding Tauri commands from `src-tauri/src/commands/` if no remaining callers
- Remove their route mappings from `fetch-api.routes/`
- Extend `tests/ts/admin-transport.test.ts`:
  - Add builtins route coverage (`list_builtin_servers`, `set_builtin_server_enabled`, `set_builtin_tool_enabled`)
  - Add config-export route coverage (all 5 new routes)
  - Add SSE event channel coverage for `oauth-consent-request`, `oauth-client-changed`, `builtin-server-config-changed`

**Outcome:** `pnpm validate` clean, no dead `apiCall` entries. `admin-transport.test.ts` covers all registered routes including the newly added config-export and builtin commands. Transport parity between Tauri and web admin is fully tested.

---

## Key files referenced

| File | Note |
| ---- | ---- |
| [`apps/desktop/src/lib/backend/data/transport.ts`](../../apps/desktop/src/lib/backend/data/transport.ts) | `apiCall` / `isTauri()` dispatcher |
| [`apps/desktop/src/lib/backend/data/fetch-api.routes/`](../../apps/desktop/src/lib/backend/data/fetch-api.routes/) | Command → HTTP route mappings |
| [`crates/mcpmux-gateway/src/admin/router.rs`](../../crates/mcpmux-gateway/src/admin/router.rs) | Rust admin route registry — 5 new config-export routes go here |
| [`crates/mcpmux-gateway/src/admin/command_bridge/read.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/read.rs) | Read handler pattern for new config-export bridges |
| [`apps/desktop/src-tauri/src/services/oauth.rs`](../../apps/desktop/src-tauri/src/services/oauth.rs) | Phase 3 target — `app.emit` → `emit_ui_channel` |
| [`apps/desktop/src-tauri/src/services/ui_events.rs`](../../apps/desktop/src-tauri/src/services/ui_events.rs) | `emit_ui_channel` + event channel name mappings |
| [`apps/desktop/src/lib/backend/events/admin-sse-hub.ts`](../../apps/desktop/src/lib/backend/events/admin-sse-hub.ts) | SSE hub — receives domain events for browser clients |
| [`apps/desktop/src/features/spaces/SpaceBaseDirsModal.tsx`](../../apps/desktop/src/features/spaces/SpaceBaseDirsModal.tsx) | Phase 4 target — unguarded `openDialog()` |
| [`scripts/dev-web-admin.mjs`](../../scripts/dev-web-admin.mjs) | Phase 1 target — broken `runPrep()` reference |
| [`apps/desktop/vite.config.ts`](../../apps/desktop/vite.config.ts) | Phase 1 target — missing `/api` proxy |
| [`tests/ts/admin-transport.test.ts`](../../tests/ts/admin-transport.test.ts) | Phase 5 target — incomplete route coverage |

---

## Related documentation

- [`docs/planning/dev-rebased-post-port-completion.md`](./dev-rebased-post-port-completion.md) — Phase 2 lib/api migration this doc builds on
- [`docs/planning/dev-to-main-port.md`](./dev-to-main-port.md) — original 8-phase port
- [`docs/frontend/technical/backend-facade.md`](../frontend/technical/backend-facade.md) — `apiCall` / fetch-api architecture reference
