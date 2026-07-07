# isTauri() Usage Audit

*2026-07-07 — inventory of platform branching in the desktop/web admin UI.*

## Summary

| Metric | Count |
|--------|------:|
| Source files referencing `isTauri` | 28 (+ tests/scripts) |
| `isTauri()` call sites in `apps/desktop/src` | ~68 |
| Canonical definition | 1 (`lib/backend/data/transport.ts`) |
| Deprecated re-export | 1 (`lib/api/transport.ts`) |

**Detection:** `'__TAURI_INTERNALS__' in window` in `apps/desktop/src/lib/backend/data/transport.ts`.

**Rule of thumb:** `isTauri()` belongs in **lib/facade layers** (transport, shell, events). Pages should call abstractions, not branch on platform — except for **purely presentational** forks (show/hide a native picker button).

---

## Verdict by layer

| Layer | Call sites | Verdict |
|-------|----------:|---------|
| Core transport | 2 | **Keep** — source of truth |
| Shell facade | 16 | **Keep** — this *is* the abstraction |
| Event system | ~22 | **Keep** — dual-hook / SSE hub pattern |
| Lib helpers | 8 | **Mostly keep** — 1 delete candidate block |
| Page/component UI | ~20 | **Mixed** — legitimate UI forks vs migrate |
| Tests/scripts | ~6 | **Keep** — mocks and E2E stubs |

---

## 1. Core transport — KEEP

**File:** `apps/desktop/src/lib/backend/data/transport.ts`

| Lines | Behavior | Opinion |
|-------|----------|---------|
| 8–9 | Defines `isTauri()` | **Keep** — single source of truth |
| 19 | `apiCall`: Tauri `invoke` vs admin HTTP `fetchApi` | **Keep** — correct dispatcher boundary |

All data-layer commands should go through `apiCall`. No page should import Tauri invoke directly for domain data.

---

## 2. Shell facade — KEEP (16 call sites)

**File:** `apps/desktop/src/lib/backend/shell/index.ts`

This is where platform branching **should** live. Each function picks Tauri vs web/no-op internally:

| Function | Tauri | Web | Opinion |
|----------|-------|-----|---------|
| `openUrl` | `open_url` command | `window.open` | Keep |
| `openExternal` | plugin-opener + fallback | `window.location.href` | Keep |
| `performWindowControl` | minimize/maximize/close | no-op | Keep |
| `listenWhenTauri` | `listen()` | `undefined` | Keep (duplicate in `tauri-adapter.ts` — consolidate later) |
| `fileSrcFromAbsolutePath` | `convertFileSrc` | `null` | Keep |
| `pickPath` | native dialog | `null` | Keep |
| `flushPendingDeepLink` | invoke | no-op | Keep |
| `subscribeOAuthConsentRequest` | Tauri event | no-op | Keep |
| `subscribeOAuthConsentEvents` | Tauri listener | admin SSE hub | Keep — good unified template |
| `openLogsFolder` | invoke | no-op | Keep |
| `openSpaceConfigFile` | invoke | no-op | Keep |
| `addToVscode` / `addToCursor` | deep link invoke | no-op | Keep |
| `checkForAvailableUpdate` / `checkAppUpdate` | updater | `null` | Keep |
| `relaunchApp` | relaunch | no-op | Keep |

**Opinion:** Do not remove these checks. Optionally export `supportsNativeFilePicker(): boolean` here so pages stop importing `isTauri` for button visibility only.

---

## 3. Event system — KEEP (~22 call sites)

Established pattern: instantiate Tauri + Web hooks; dispatcher returns `isTauri() ? tauri : web`. Callers never branch.

| File | Role | Opinion |
|------|------|---------|
| `useDomainEvents.ts` | Hook dispatcher + Tauri subscribe no-op on web | **Keep** |
| `useWorkspaceEvents.ts` / `Web.ts` | Workspace event channels | **Keep** |
| `useOAuthClientEvents.ts` / `Web.ts` | OAuth client events | **Keep** |
| `useMetaToolEvents.ts` / `Web.ts` | Meta-tool events | **Keep** |
| `use-backend-event-subscription.ts` | Generic Tauri IPC vs SSE | **Keep** — use for ad-hoc channels |
| `tauri-adapter.ts` | Desktop-only listen helper | **Keep** |
| `admin-sse-hub.ts` | Skip EventSource on Tauri (4 guards) | **Keep** — inverse guard is correct |
| `metaToolActivityStore.ts` | Tauri listen vs SSE raw channel | **Keep** |
| `useDataSync.ts` | Web calls `enableAdminSse()` before sync | **Keep** |

**Recently fixed:** `space-servers-updated` / `space-servers-sync-failed` moved into `useDomainEvents` — ServersPage no longer branches on `isTauri` for file-watcher events.

---

## 4. Lib helpers — MOSTLY KEEP

| File | Lines | Purpose | Opinion |
|------|-------|---------|---------|
| `lib/updates.ts` | 2 | Updater no-op on web | **Keep** — could route all callers through shell |
| `lib/viewer-device.helpers.ts` | 1 | `isViewingLocally()` | **Keep** — Tauri always local; web checks hostname |
| `lib/api/workspaceAppearances.ts` | 1 | Icon URL: asset vs HTTP blob | **Keep** — good lib-level abstraction |
| `lib/build-info.helpers.ts` | 1 | Debug log label | **Keep** — trivial |
| `lib/api/serverManager.ts` | 3 | `onServerStatus` / `onAuthProgress` / `onFeaturesUpdated` | **Delete** — zero callers; superseded by `useDomainEvents` |

---

## 5. Page / component UI — MIXED (~20 call sites)

### Legitimate — KEEP (presentational or product scope)

| File | What branches | Opinion |
|------|---------------|---------|
| `App.tsx:332` | Custom title bar controls | **Keep** — web has no window chrome |
| `App.tsx:342` | Update banner | **Keep** — no Tauri updater on web |
| `App.tsx:111` | Auto-update check | **Keep** — could call `shell/checkForAvailableUpdate` only |
| `SettingsPage.tsx:652` | `UpdateChecker` vs `AboutSection` | **Keep** |
| `SettingsPage.tsx:1093` | Web-admin config panel (desktop-only control plane) | **Keep** |
| `SettingsPage.tsx:1487` | "Open logs folder" button visibility | **Keep** — shell already no-ops on web |
| `SettingsPage.tsx:249` | Load admin web settings | **Keep** — could move into subsection |
| `ConnectIDEs.tsx:106` | Hide deep-link IDE entries on web | **Keep** — product decision (docs: desktop-only) |
| `StaleBuildBanner.tsx:17` | Stale SHA check web-only | **Keep** — dev/HMR concern, not Tauri |
| `SpaceBaseDirsModal.tsx:196` | Native picker button vs text path row | **Keep** — Phase 4 pattern |
| `workspace-binding-form.component.tsx` | Native icon/root pickers | **Keep** — same pattern |

### Should migrate — REMOVE page-level `isTauri`

| File | What branches | Opinion |
|------|---------------|---------|
| `BuiltinServersPage.tsx:85` | Raw `listen('builtin-server-config-changed')` + guard | **Migrate** → `useBackendEventSubscription` (web SSE broken today) |
| `ServersPage.tsx:2173–2208` | Picker placeholder + button visibility (×4) | **Migrate** → shared `PathInput` component owns platform check internally |
| `SpaceBaseDirsModal.tsx:84` | Handler early-return | **Migrate** → same `PathInput` |

### Investigate before changing

| File | What branches | Opinion |
|------|---------------|---------|
| `AutoStartConflictResolver.tsx:46` | Gates on `isLoadingSpaces` only; still polls port conflict on web after load | **Investigate** — is auto-start port resolution desktop-only? |

---

## 6. Import path debt (not behavioral)

| Import path | Used by | Opinion |
|-------------|---------|---------|
| `@/lib/backend/data/transport` | 15 files | **Canonical** |
| `@/lib/backend/shell` (re-export) | 4 files | OK |
| `@/lib/api/transport` | `SettingsPage.tsx` only | **Fix import** — deprecated shim |
| `@/lib/backend` facade | `AutoStartConflictResolver.tsx` | OK |

---

## 7. Tests / scripts — KEEP

| File | Usage |
|------|-------|
| `tests/ts/lib/updates.test.ts` | Mocks `isTauri` |
| `tests/ts/setup.ts` | Sets `__TAURI_INTERNALS__` (default test mode = Tauri) |
| `scripts/take-screenshots.cjs` | Stubs `__TAURI_INTERNALS__` for E2E |

Must stay aligned with `transport.ts` detection logic.

---

## Competing patterns (when to use which)

1. **`apiCall`** — all Rust commands / admin REST
2. **Dual-hook dispatcher** — `useDomainEvents`, `useWorkspaceEvents`, etc.
3. **`admin-sse-hub`** — web EventSource; no-ops on Tauri
4. **`useBackendEventSubscription`** — ad-hoc channels (builtin-server-config, etc.)
5. **`shell/*`** — OS capabilities with built-in fallbacks
6. **Page UI fork** — only when divergence is purely presentational

---

## Prior doc decisions

From `docs/planning/web-admin-completion.md`:

- **Phase 4:** file pickers → native button on Tauri, text input on web (partially done)
- **Out of scope:** Connect IDE deep links on web
- **Out of scope:** meta-tools approval dialog on web
- **Pending:** `BuiltinServersPage` SSE alignment for `builtin-server-config-changed`

Docs still claim `SpaceBaseDirsModal` / `ServersPage` unguarded — **stale**; both now have guards.

---

## Recommended follow-ups (user decisions 2026-07-07)

| Item | Decision |
|------|----------|
| Path picker duplication | Extract shared **`PathInput`** component (owns `isTauri` internally) |
| `BuiltinServersPage` live refresh | Migrate to **`useBackendEventSubscription('builtin-server-config-changed')`** |
| `serverManager.ts` dead listeners | **Delete** |
| `AutoStartConflictResolver` on web | **Investigate** before changing |
| Scope for this pass | **Audit only** — no code changes from this doc |

---

## Full file inventory

Files with `isTauri` references in `apps/desktop/src`:

```
App.tsx
components/ConnectIDEs.tsx
components/StaleBuildBanner.tsx
features/builtinServers/BuiltinServersPage.tsx
features/gateway/AutoStartConflictResolver.tsx
features/servers/ServersPage.tsx
features/settings/SettingsPage.tsx
features/spaces/SpaceBaseDirsModal.tsx
features/workspaces/workspace-binding-form.component.tsx
hooks/useDataSync.ts
lib/api/serverManager.ts          ← delete candidate
lib/api/workspaceAppearances.ts
lib/backend/data/transport.ts     ← definition
lib/backend/events/admin-sse-hub.ts
lib/backend/events/tauri-adapter.ts
lib/backend/events/use-backend-event-subscription.ts
lib/backend/events/useDomainEvents.ts
lib/backend/events/useMetaToolEvents.ts
lib/backend/events/useMetaToolEventsWeb.ts
lib/backend/events/useOAuthClientEvents.ts
lib/backend/events/useOAuthClientEventsWeb.ts
lib/backend/events/useWorkspaceEvents.ts
lib/backend/events/useWorkspaceEventsWeb.ts
lib/backend/shell/index.ts        ← largest concentration
lib/build-info.helpers.ts
lib/updates.ts
lib/viewer-device.helpers.ts
stores/metaToolActivityStore.ts
```

Plus: `lib/api/transport.ts` (deprecated re-export), `tests/ts/lib/updates.test.ts`, `tests/ts/setup.ts`, `scripts/take-screenshots.cjs`.
