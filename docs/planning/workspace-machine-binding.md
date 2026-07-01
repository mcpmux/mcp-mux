# Workspace Machine Binding

**Last Updated:** Jul 1, 2026
**Status:** In progress — Phases 1–4 largely landed; per-device header for tunneled multi-device routing landed Jun 30, 2026; PR #8 (this branch) reviewed Jul 1, 2026 — see architecture review below
**Branch:** `feat/workspace-machine-binding` (off `dev-rebased`)
**Depends on:** `dev-rebased` label/icon port complete (migration 032 landed)
**Unblocks:** Per-machine project organization and override routing; homelab multi-box workflow

---

## Update — Jul 1, 2026 (PR #8 architecture review — client_id vs machine_id)

PR #8 review surfaced that `machine_id` didn't replace the pre-existing `client_id` scoping (migration 027) — it stacked on top of it. `WorkspaceBinding` now carries **two independent optional scope fields**, and the resolver reconciles both instead of one:

- `client_id` — OAuth app identity (Cursor, Claude Desktop, …), pre-existing.
- `machine_id` — physical host identity, this feature.

`find_exact_for_machine` handles all four combinations (client+machine, machine-only canonical, client-only legacy, global), with client+machine taking priority over machine-only:

```322:346:crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs
async fn find_exact_for_machine(...) -> Result<Option<WorkspaceBinding>> {
    let bindings = self.list().await?;
    // Client+machine scoped binding takes priority over machine-only canonical.
    ...
}
```

This directly contradicts two "Out of scope" decisions below (marked ⚠️ **STALE** — see Scope section) that assumed client+machine combined scoping would never be needed. It shipped anyway (`007de1f`, Jun 27) to fix bindings created via the workspace-needs-binding popup that had both `machine_id` and `client_id` set and were invisible to the old machine-only lookup.

**Not a bug** — traced the priority order end-to-end, it's internally consistent and tested (`test_machine_id_round_trip` etc). But it means the OAuth client-identity system and the machine-identity system are two separate trust/scope axes stacked together with a compatibility fallback (`if local_machine_id.is_none() { find_exact_for_roots... }` for installs that haven't adopted machines yet), not one unified caller-identity concept. See **Future TODOs** below.

Full PR review + architecture discussion: [PR #8](https://github.com/crimsonsunset/mcp-mux/pull/8).

---

## Update — Jun 26, 2026 (Settings Machine Identity, remote web admin)

Remote web admin (`mux.joe-hassio.com` → static SPA on `:45819`) now uses a **viewer-only main card** in Settings → Machine Identity:

- **Main card (remote):** `"This viewer"` editor bound to `useViewerIdentity()` — shows the browser's machine (e.g. Rohan on MacBook), matching the status bar `Viewer: Rohan`.
- **Main card (local/Tauri):** single `"This install"` editor (viewer and gateway collapse to `gateway.local_machine_id`).
- **Gateway machine (Gondor):** editable only in the **Manage all machines** expander (row labeled `THIS GATEWAY` + `local` badge when remote). The dedicated `"This gateway"` editor block was removed from the main card when the gateway is already registered.
- **Unregistered gateway (remote only):** if `local_machine_id` is unset, a one-time register prompt still appears below the viewer editor so a fresh install can name the gateway host.

`pnpm dev:admin` now runs `vite build --watch` in parallel, so `apps/desktop/dist/` auto-rebuilds on every frontend save. Hard-refresh `mux.joe-hassio.com` after edits (~10s rebuild). One-off bundle: `pnpm build:web:admin`.

Implementation: [`apps/desktop/src/features/settings/SettingsPage.tsx`](../../apps/desktop/src/features/settings/SettingsPage.tsx) `MachineIdentitySection`; viewer resolution in [`use-viewer-identity.hook.tsx`](../../apps/desktop/src/hooks/use-viewer-identity.hook.tsx) + [`viewer-device.helpers.ts`](../../apps/desktop/src/lib/viewer-device.helpers.ts).

---

## Update — Jun 30, 2026 (machine ID in viewer modal + Settings viewer card)

Machine catalog UUID is surfaced on all viewer identity surfaces via [`machine-id-section.component.tsx`](../../apps/desktop/src/components/machine-id-section.component.tsx):

- **Status bar modal** (`ViewerIdentityModal`): always-visible Machine ID section — read-only UUID + **Copy UUID** / **Copy MCP header** when linked; paste-to-link when unlinked.
- **Settings viewer card** (`"This viewer"` / `"This install"`): same `MachineIdSection` wired to `useViewerIdentity()`.
- **Manage all machines** rows: **Copy UUID** added beside existing **Copy MCP header** (shared [`machine-id.helpers.ts`](../../apps/desktop/src/lib/machine-id.helpers.ts)).

`linkMachineById` in the viewer identity hook validates UUID format, confirms the row exists in the catalog, and calls `setViewerMachineId` so remote browsers can attach to an existing machine without creating a duplicate row.

---

## Update — Jun 30, 2026 (per-device header for shared tunnel)

When multiple physical devices reach **one** gateway via a tunnel (`gateway.public_url`), the gateway cannot infer which laptop/desktop made the request from `gateway.local_machine_id` alone — that always identifies the host running McpMux (e.g. Gondor), not the device running Cursor (e.g. Rohan).

**Fix:** each device's MCP client config sends `X-Mcpmux-Machine-Id: <machine-uuid>`. The resolver uses that header as the machine signal for binding lookup. When the header is present, client and gateway-local machine tags are skipped so a tunneled caller is not mistaken for the gateway host.

See [`per-device-machine-header.md`](./per-device-machine-header.md) for full design, resolver priority, and client setup. User-facing docs: [Remote Access](/docs/remote-access/), [Workspaces](/docs/workspaces/), [Clients](/docs/clients/).

---

## Update — Jun 30, 2026 (meta-tools machine-scoped binding)

`mcpmux_bind_current_workspace` and `mcpmux_set_workspace_root` now thread `X-Mcpmux-Machine-Id` through `MetaToolCall.request_machine_id` and write machine-scoped bindings (`machine_id` set, `client_id` unset) when any machine identity is available. See [`meta-tools-machine-scoped-binding.md`](./meta-tools-machine-scoped-binding.md).

---

## Problem

McpMux runs as a single central gateway (Box 1 / Gondor in the homelab). Remote machines (Box 4 MacBook, cloud agents) connect into it and report workspace roots. Today the Projects page has no concept of which physical machine a project lives on — all 13 bindings look identical regardless of origin box, and the same path can't exist more than once even if it belongs to a different machine.

Two things break down:

**1. No organization by machine.** A homelab with 4 machines produces one undifferentiated list. There's no way to filter "show me only Box 4 projects" or quickly see which box a binding is for.

**2. Same-path disambiguation is impossible.** If Box 1 and Box 4 both have `/Users/joe/projects/gait`, there can only be one binding for that path today. Per-machine feature set overrides (e.g. Box 4 gets a read-only subset) require the resolver to pick by `(machine, path)`, not just `path`.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Machine catalog | Dedicated `machines` table — `id, name, icon, hostname, created_at, updated_at` | Machines need their own label/icon (same pattern as Spaces). Free-text `machine_name` on each binding would diverge and can't carry metadata or be renamed atomically. |
| 2 | Local machine identity | `gateway.local_machine_id` app setting (UUID FK into `machines`); on first use, prompt user to name/register this install | Each McpMux install self-identifies. For **local** MCP (`localhost:45818`), session roots and `local_machine_id` are enough. For **tunneled** multi-device setups, also send `X-Mcpmux-Machine-Id` per device — see [`per-device-machine-header.md`](./per-device-machine-header.md). |
| 3 | Naming | `machine` / `machine_id` throughout (domain, SQL, TS, UI) | More generic than "box" — covers cloud agents, CI runners, etc. Matches how the concept appears in homelab docs. |
| 4 | Routing model | Same partial unique index pattern as `client_id` (migration 027): global binding `WHERE machine_id IS NULL`, machine-scoped binding `WHERE machine_id IS NOT NULL`. Resolver: machine-specific match first, global fallback. | Directly mirrors the existing `client_id` scope layer. Same binding, same path — machine override inherits the canonical intent and only changes what needs to change for that box. |
| 5 | Resolver wiring | Plumb `local_machine_id` from `AppSettingsService` into `FeatureSetResolverService` at startup | The resolver is the natural chokepoint. No protocol change needed — the gateway already knows its own machine identity via settings. |
| 6 | UI entry point | Machine picker in binding create/edit form + machine filter dropdown on Projects page | Machines are binding metadata — same inspector flow as label/icon. Filter is a dropdown (not segmented chips) because machine count is unbounded. |

---

## Scope

**In:**
- `machines` table + `Machine` domain entity + `MachineRepository` trait + SQLite impl (migration 033)
- `machine_id` FK on `workspace_bindings` + updated partial unique indexes (migration 034)
- `gateway.local_machine_id` app setting + first-time machine registration prompt
- Tauri commands: `list_machines`, `create_machine`, `update_machine`, `delete_machine`, `get_local_machine_id`, `set_local_machine_id`
- Admin bridge equivalents for all machine CRUD + `local_machine_id` get/set
- `machine_id` field on `WorkspaceBindingInput` / `WorkspaceBindingDto`
- Resolver: machine-aware binding lookup with global fallback
- Projects page: machine filter dropdown + machine badge on `EntryCard`
- Settings: Machine Identity section — viewer-only main card when remote; gateway editable in "Manage all machines" expander; local install uses single "This install" editor

**Out:**

| Item | Reason |
| ---- | ------ |
| ~~Per-client + per-machine combined scoping `(client_id, machine_id, workspace_root)`~~ | ⚠️ **STALE — shipped anyway.** `find_exact_for_machine` matches client+machine scoped bindings before falling back to machine-only canonical (`007de1f`, Jun 27) — the workspace-needs-binding popup writes both fields and needed a lookup path that could see them. See Jul 1 review update above. |
| ~~Machine-aware `client_id` scoped bindings~~ | ⚠️ **STALE — shipped anyway.** Client-scoped bindings are the fallback path (`WorkspaceBinding::new_scoped_multi`) when no machine identity is available at bind time (`bind_workspace.rs`). Machine and client scope are not orthogonal in the current resolver. |
| Cross-machine DB sync / export | Each McpMux install has its own SQLite. Syncing machines across installs is a future cloud/sync feature. |
| Machine icon upload from web admin | Icon upload requires a file picker; follow the same deferral as base-dirs on web (text/emoji only for now). |
| Automatic hostname detection | Querying `hostname` at runtime is easy but creates a false sense of automation — the user should consciously name their machines. Pre-fill the hostname field from `gethostname()` as a hint only. |

---

## Architecture

### Data model

```
machines
  id          TEXT PK        — UUID
  name        TEXT NOT NULL  — "Gondor (Box 1)", "MacBook (Box 4)"
  icon        TEXT           — emoji or local: ref, same convention as bindings
  hostname    TEXT           — optional hint (gethostname on first register)
  created_at  TEXT NOT NULL
  updated_at  TEXT NOT NULL

workspace_bindings (after migration 034)
  ...existing cols...
  machine_id  TEXT REFERENCES machines(id) ON DELETE SET NULL

Partial unique indexes (replacing migration 027's global index):
  idx_wb_root_global   UNIQUE(workspace_root)            WHERE machine_id IS NULL AND client_id IS NULL
  idx_wb_root_machine  UNIQUE(machine_id, workspace_root) WHERE machine_id IS NOT NULL AND client_id IS NULL
  idx_wb_root_scoped   UNIQUE(client_id, workspace_root)  WHERE client_id IS NOT NULL   (unchanged)

app_settings
  gateway.local_machine_id  — UUID or NULL; the machine this install is registered as
```

### Resolver flow (with machine dimension)

**No `X-Mcpmux-Machine-Id` header** (local MCP or legacy remote):

```
resolve(session_id, client_id):
    for root in roots:
        if client.machine_id: try binding for that machine
        if local_machine_id:  try binding for gateway host machine
        try global binding (machine_id IS NULL)
```

**Header present** (`X-Mcpmux-Machine-Id: <uuid>` on tunneled clients):

```
resolve(session_id, client_id, request_machine_id):
    for root in roots:
        try binding for request_machine_id only
        try global binding (machine_id IS NULL)
        // client + local_machine_id skipped — avoids Rohan→Gondor mis-routing
```

Bound-elsewhere: a path scoped only to machine A does not match when the request carries machine B's header (or when `local_machine_id` is B with no header).

### First-time machine registration

On `WorkspacesPage` load (or first binding create), if `local_machine_id` is unset and at least one binding exists, show a one-time inline banner:

> "This McpMux install has no machine identity. Name it to organize and filter your projects by machine."

Clicking "Set up" opens a small modal: name field (required), icon (optional), hostname (pre-filled from `get_hostname` Tauri command, editable). On confirm, creates a `Machine` and writes `gateway.local_machine_id`.

---

## Files to Create

| File | Purpose |
| ---- | ------- |
| [`crates/mcpmux-storage/src/migrations/033_machines.sql`](../../crates/mcpmux-storage/src/migrations/033_machines.sql) | `machines` table |
| [`crates/mcpmux-storage/src/migrations/034_workspace_binding_machine_scope.sql`](../../crates/mcpmux-storage/src/migrations/034_workspace_binding_machine_scope.sql) | `machine_id` FK on bindings + rebuild partial unique indexes |
| [`crates/mcpmux-core/src/domain/machine.rs`](../../crates/mcpmux-core/src/domain/machine.rs) | `Machine` entity + `normalize_optional_metadata` helpers |
| [`crates/mcpmux-storage/src/repositories/machine_repository.rs`](../../crates/mcpmux-storage/src/repositories/machine_repository.rs) | SQLite CRUD for `machines` |
| [`apps/desktop/src-tauri/src/commands/machines.rs`](../../apps/desktop/src-tauri/src/commands/machines.rs) | Tauri commands: `list_machines`, `create_machine`, `update_machine`, `delete_machine`, `get_local_machine_id`, `set_local_machine_id` |
| [`apps/desktop/src/lib/api/machines.ts`](../../apps/desktop/src/lib/api/machines.ts) | TS API surface for machine CRUD + local machine id |
| [`apps/desktop/src/lib/backend/data/fetch-api.routes/machines.routes.ts`](../../apps/desktop/src/lib/backend/data/fetch-api.routes/machines.routes.ts) | Admin HTTP route mappings for all machine commands |

## Files to Modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-core/src/domain/mod.rs`](../../crates/mcpmux-core/src/domain/mod.rs) | Export `Machine`, `MachineRepository` |
| [`crates/mcpmux-core/src/domain/workspace_binding.rs`](../../crates/mcpmux-core/src/domain/workspace_binding.rs) | Add `machine_id: Option<Uuid>` field |
| [`crates/mcpmux-core/src/repository/mod.rs`](../../crates/mcpmux-core/src/repository/mod.rs) | Add `MachineRepository` trait |
| [`crates/mcpmux-core/src/service/app_settings_service.rs`](../../crates/mcpmux-core/src/service/app_settings_service.rs) | Add `keys::gateway::LOCAL_MACHINE_ID`, `get/set_local_machine_id()` |
| [`crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs`](../../crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs) | `SELECT_COLS` + `row_to_binding_no_fs` + create/update for `machine_id` |
| [`crates/mcpmux-storage/src/database.rs`](../../crates/mcpmux-storage/src/database.rs) | Register migrations 033 + 034; add `MachineRepository` constructor |
| [`crates/mcpmux-storage/src/lib.rs`](../../crates/mcpmux-storage/src/lib.rs) | Export `SqliteMachineRepository` |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Accept `local_machine_id: Option<Uuid>` + machine-first lookup |
| [`crates/mcpmux-gateway/src/admin/command_bridge/read.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/read.rs) | `list_machines`, `get_local_machine_id`, `to_workspace_binding_response` + `machine_id` |
| [`crates/mcpmux-gateway/src/admin/command_bridge/write.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/write.rs) | `create_machine`, `update_machine`, `delete_machine`, `set_local_machine_id`; `WorkspaceBindingBody` + `machine_id` |
| [`crates/mcpmux-gateway/src/admin/router.rs`](../../crates/mcpmux-gateway/src/admin/router.rs) | Mount `/api/v1/machines` routes |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Register machine Tauri commands |
| [`apps/desktop/src-tauri/src/commands/workspace_binding.rs`](../../apps/desktop/src-tauri/src/commands/workspace_binding.rs) | `WorkspaceBindingDto` + `WorkspaceBindingInput` add `machine_id` |
| [`apps/desktop/src/lib/api/workspaceBindings.ts`](../../apps/desktop/src/lib/api/workspaceBindings.ts) | `WorkspaceBinding.machine_id`, `WorkspaceBindingInput.machine_id` |
| [`apps/desktop/src/lib/backend/data/fetch-api.routes/index.ts`](../../apps/desktop/src/lib/backend/data/fetch-api.routes/index.ts) | Include `machines.routes.ts` |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | Machine filter dropdown, `EntryCard` machine badge, first-time registration banner |
| [`apps/desktop/src/features/settings/SettingsPage.tsx`](../../apps/desktop/src/features/settings/SettingsPage.tsx) | Machine Identity section |
| [`apps/desktop/src/locales/en/workspaces.json`](../../apps/desktop/src/locales/en/workspaces.json) | Machine filter + badge i18n strings |
| [`apps/desktop/src/locales/en/settings.json`](../../apps/desktop/src/locales/en/settings.json) | Machine identity i18n strings |

---

## Phases

### Phase 1 — Domain + storage (~half day)

- Write `033_machines.sql`: `machines` table with `id, name, icon, hostname, created_at, updated_at`
- Write `034_workspace_binding_machine_scope.sql`: `ALTER TABLE workspace_bindings ADD COLUMN machine_id TEXT REFERENCES machines(id) ON DELETE SET NULL`; `DROP INDEX idx_wb_root_global`; recreate it with `WHERE machine_id IS NULL AND client_id IS NULL`; create `idx_wb_root_machine UNIQUE(machine_id, workspace_root) WHERE machine_id IS NOT NULL AND client_id IS NULL`; create `idx_workspace_bindings_machine`
- `Machine` struct in `domain/machine.rs` with `id, name, icon, hostname, created_at, updated_at` + `#[serde(default)]` on optional fields
- `MachineRepository` trait on `repository/mod.rs`: `list`, `get`, `create`, `update`, `delete`
- `SqliteMachineRepository` in `mcpmux-storage` following the same `Arc<Mutex<Database>>` pattern as other repos
- Add `machine_id: Option<Uuid>` to `WorkspaceBinding` with `#[serde(default)]`; update `SELECT_COLS`, `row_to_binding_no_fs`, `create`, `update` in the binding repo
- Register both migrations in `database.rs`; export repo from `mcpmux-storage`
- Update fork migration test expected version to 34

**Outcome:** `cargo test --workspace` passes with the two new migrations applied. A `Machine` can be created and listed from the repo in an integration test. `workspace_bindings` round-trips `machine_id` through create/update/list.

---

### Phase 2 — Settings layer + Tauri/admin commands (~half day)

- Add `keys::gateway::LOCAL_MACHINE_ID: &str = "gateway.local_machine_id"` to `app_settings_service.rs`
- `get_local_machine_id() -> Option<Uuid>` and `set_local_machine_id(id: Option<Uuid>)` on `AppSettingsService`
- `commands/machines.rs`: Tauri commands for all six ops + `MachineDto` (mirrors `Machine` with string id/dates)
- Register commands in `lib.rs`
- Admin bridge read: `list_machines`, `get_local_machine_id`
- Admin bridge write: `create_machine`, `update_machine`, `delete_machine`, `set_local_machine_id`; `WorkspaceBindingBody.machine_id: Option<String>`
- Mount `/api/v1/machines` (GET list, POST create, PUT `:id`, DELETE `:id`) + `/api/v1/machines/local` (GET, PUT) in `router.rs`
- `apps/desktop/src/lib/api/machines.ts` + `machines.routes.ts` + wire into route index
- Add `get_hostname` Tauri command that returns `hostname::get()` as a hint for the registration prompt

**Outcome:** `list_machines` returns `[]` on a fresh DB from both Tauri IPC and the admin HTTP API. Creating a machine via admin curl returns the new row. `get_local_machine_id` returns `null` until set. `pnpm typecheck` passes.

---

### Phase 3 — Binding CRUD with machine dimension (~half day)

- `WorkspaceBindingInput.machine_id?: string | null` and `WorkspaceBindingDto.machine_id: string | null` on both Tauri and TS sides
- Tauri `create_workspace_binding` and `update_workspace_binding`: pass `machine_id` through (no resolution logic — just store what's given; `None` = global canonical)
- Admin bridge create/update: same pass-through
- `apps/desktop/src/lib/api/workspaceBindings.ts`: add `machine_id` to `WorkspaceBinding` interface and `WorkspaceBindingInput`
- Projects inspector panel: add a machine picker (`<select>` populated from `listMachines()`) in the binding create/edit form, under the label/icon fields; pre-selects `local_machine_id` when creating a new binding on this install

**Outcome:** Creating a binding with a `machine_id` persists it and returns it in `list_workspace_bindings`. Two bindings for the same path but different machines coexist without a unique conflict. The inspector picker is visible in the UI with "No machine" as the default option.

---

### Phase 4 — Resolver: machine-aware lookup (~quarter day)

- Plumb `local_machine_id` into `FeatureSetResolverService` — read from `AppSettingsService` at gateway startup, store as `Option<Uuid>` on the service struct
- Update Tier 1 binding lookup: for each reported root, try `find_exact(machine_id=local_machine_id, root)` first; on miss fall back to `find_exact(machine_id=NULL, root)`
- Add `find_exact_for_machine` and `find_exact_global` to `WorkspaceBindingRepository` trait + `SqliteWorkspaceBindingRepository` impl (two simple `WHERE` queries against the in-memory `list()` result, same O(n) pattern as existing exact match)
- Update `FeatureSetResolverService::new` builder in the gateway and desktop wiring to pass `local_machine_id`

**Outcome:** With a machine-scoped binding and a global fallback for the same path, a gateway whose `local_machine_id` matches routes to the machine-specific binding's feature sets. A gateway with no `local_machine_id` routes to the global binding. `cargo nextest run -p tests` passes.

---

### Phase 5 — Projects UI: filter dropdown + card badges + first-time prompt (~half day)

- Load `listMachines()` in `WorkspacesPage` initial data fetch (alongside bindings/spaces/feature-sets)
- Add `machine` filter state (`string | 'all'`); default `'all'`
- Replace the static chip set with a `<select>` dropdown for machine filter next to `SegmentedFilter` — options: "All machines" + one entry per machine that has at least one binding (name only, no counts)
- `filtered` useMemo: add machine filter clause — include entry if `filter.machine === 'all'` or `entry.binding?.machine_id === filter.machine`
- `EntryCard`: add machine badge (machine name as a `<Chip>`) in the footer when `binding.machine_id` is set; resolve machine name from a `machinesById` map passed down as prop
- First-time banner: if `localMachineId` is null and `bindings.length > 0`, show a dismissible inline alert above the grid with a "Set up machine identity" button that opens a small modal (name + icon + hostname pre-filled); on confirm calls `createMachine` then `setLocalMachineId`
- Settings Machine Identity section: **done (Jun 26, 2026)** — remote main card shows `"This viewer"` only (`useViewerIdentity`); gateway host editable in "Manage all machines" expander (`THIS GATEWAY` row); local/Tauri shows single `"This install"` editor. Unregistered gateway on remote still gets inline register prompt. Rebuild static SPA (`pnpm build:web:admin`) for tunnel users.
- Add i18n strings (`filter.machine`, `machineIdentity.*`, `card.machine`)

**Outcome:** The Projects page shows a machine dropdown. Selecting a machine filters the grid to only that machine's bindings. Cards with a machine show the machine name in the footer. A new McpMux install with existing bindings and no local machine identity shows the registration banner exactly once. `pnpm typecheck && pnpm lint` pass clean.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`docs/planning/per-device-machine-header.md`](./per-device-machine-header.md) | Tunneled multi-device routing via `X-Mcpmux-Machine-Id` |
| [`crates/mcpmux-storage/src/migrations/027_workspace_binding_client_scope.sql`](../../crates/mcpmux-storage/src/migrations/027_workspace_binding_client_scope.sql) | Template for partial unique index pattern; machine scope mirrors client scope exactly |
| [`crates/mcpmux-storage/src/migrations/020_workspace_binding_label.sql`](../../crates/mcpmux-storage/src/migrations/020_workspace_binding_label.sql) | Minimal `ALTER TABLE ADD COLUMN` migration pattern |
| [`crates/mcpmux-core/src/domain/workspace_binding.rs`](../../crates/mcpmux-core/src/domain/workspace_binding.rs) | `client_id` precedent for optional scoping fields |
| [`crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs`](../../crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs) | `SELECT_COLS`, `row_to_binding_no_fs`, bulk FS load pattern |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Tier 1 binding lookup; insertion point for machine-first resolution |
| [`crates/mcpmux-core/src/service/app_settings_service.rs`](../../crates/mcpmux-core/src/service/app_settings_service.rs) | Key constant convention + typed `get/set` helper pattern |
| [`crates/mcpmux-storage/src/database.rs`](../../crates/mcpmux-storage/src/database.rs) | `MIGRATIONS` array registration; fork migration test expected version |
| [`apps/desktop/src-tauri/src/commands/workspace_binding.rs`](../../apps/desktop/src-tauri/src/commands/workspace_binding.rs) | `WorkspaceBindingDto` + `WorkspaceBindingInput`; `resolve_binding_icon` pattern for optional field pass-through |
| [`crates/mcpmux-gateway/src/admin/command_bridge/write.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/write.rs) | `WorkspaceBindingBody` + admin create/update shape |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `SegmentedFilter`, `EntryCard`, `filtered` useMemo — insertion points for machine filter + badge |
| [`/Users/joe/Desktop/Repos/Personal/jsg-tech-check/docs/setup/home-lab-overview.md`](../../../jsg-tech-check/docs/setup/home-lab-overview.md) | Box 1–4 inventory; Box 1 = Gondor = Mac Studio M4 Max = central gateway host |

---

## Related Documentation

- [`dev-rebased-post-port-completion.md`](./dev-rebased-post-port-completion.md) — label/icon port this feature builds on (migration 032)
- [`dev-to-main-port.md`](./dev-to-main-port.md) — broader port history; Phase 7 workspace binding metadata work

---

## Future TODOs (from PR #8 review, Jul 1 2026)

**Identity model:**
- [ ] Decide whether `client_id` scoping and `machine_id` scoping should be unified into one caller-identity concept, or formally documented as two intentionally-separate axes (client = app, machine = host) with the current four-combination priority as the permanent design. Right now it's implicit — nobody decided it, it accreted across migrations 027 → 033–035 → `007de1f`.
- [ ] If unified: evaluate whether `WorkspaceBinding.client_id` can be deprecated in favor of always deriving scope through `inbound_clients.machine_id`, now that every OAuth client can be assigned a machine. Blocked on: pre-existing client-scoped bindings in the wild would need a migration.

**File size (deferred twice already — `workspace-binding-project-adopt.md`, `sidesheet-panel-identity-header.md`):**
- [ ] `WorkspacesPage.tsx` (2014 lines, 9 components in one file) — extract `EntryCard`, `EntryCardRoutingTable`, `EffectiveFeaturesContent`, `MachineRegistrationModal` into their own files.
- [ ] `SettingsPage.tsx` (2052 lines, one 1436-line `SettingsPage()` function) — extract `MachineIdentitySection` and other settings sections out of the main component.
- [ ] `workspace-binding-panel.component.tsx` (1296 lines) and `workspace-binding-form.component.tsx` (996 lines) — both born oversized; split by section (identity header / routing fields / scope fields) now while the code is fresh, before the next feature adds to them.

**Doc hygiene:**
- [ ] `deny-by-default-bindable-callers.md` still says `Status: Planning — ready to implement` and `Branch: feat/deny-by-default-routing` despite shipping on this branch Jun 29 — update status/branch fields.
- [ ] `projects-grouped-machine-cards.md` planned a follow-on branch that never happened (landed in-branch Jun 25) — same cleanup.

**Lower priority / tracked, not urgent:**
- [ ] `find_exact_for_machine` / `find_exact_global` do an in-memory `list()` scan per lookup per root — fine at homelab scale, revisit if bindings scale past "tens."
- [ ] `feature_set_repo` field on `FeatureSetResolverService` is `#[allow(dead_code)]`, kept for constructor API stability — either finish removing it or drop the "temporary" framing in the comment.
- [ ] No CI ran against this branch (fork, Actions not enabled) — fine for now per solo-project workflow, but the gap grows with PR size like this one.
