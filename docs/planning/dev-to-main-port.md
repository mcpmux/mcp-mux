# Fork → Upstream Reconciliation (`dev` + `i18n` → `main`)

**Last Updated:** Jun 23, 2026
**Status:** Planning — not started
**Branch:** N/A — each phase is a separate feature branch off `main`
**Base branch:** `main` (upstream `mcpmux/mcp-mux`, synced to `4a69908` as of Jun 22, 2026)
**Depends on:** `main` kept in sync with upstream before each phase branch is cut
**Unblocks:** i18n landing on upstream; all fork-native features reaching production

---

## Problem

The fork (`crimsonsunset/mcp-mux`) and upstream diverged at the repo root — no shared ancestor commit. A direct `git merge main dev --allow-unrelated-histories` produces **211 `add/add` conflicts** with zero auto-resolutions. That path is not viable.

Upstream shipped 24 commits while `dev` accumulated 387 (+ 13 more on `i18n`). Both independently assigned **migration numbers 016–019** to different content. A full-history merge would require manually resolving every conflicted file as two competing full-file versions with no diff baseline.

The only tractable path is porting feature clusters as targeted PRs against `main`, one group at a time, with migrations renumbered to not collide with upstream's 016–019 sequence.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Merge strategy | **Feature-branch PRs against `main`** — no full history merge | 211 `add/add` conflicts with no shared ancestor makes a direct merge a manual rewrite. Targeted PRs keep each delta reviewable, bisectable, and independently shippable. |
| 2 | Migration renumbering | **Fork migrations 016–027 → 020–031** | Upstream occupies 016–019 (`space_builtin_servers`, `purge_orphaned_feature_set_members`, `starter_is_default_fallback_copy`, `space_base_dirs`). Fork's 016–027 must start at 020 to avoid collision. |
| 3 | Phase ordering | **Storage first, then gateway, then UI features** | UI feature pages depend on Tauri commands that depend on domain entities that depend on storage. Inverting this order means each PR would be broken in isolation. |
| 4 | `i18n` branch | **Port last, as a rebase on top of all prior phases** | `i18n` sits 13 commits ahead of `dev`. Rebasing it on top of a fully reconciled `main` is cleaner than porting it mid-sequence and having to rebase again after each subsequent phase lands. |
| 5 | `HomePage` vs `Dashboard` | **Keep both; route `HomePage` as `/` and `Dashboard` as `/dashboard`** | Upstream's `HomePage` is a summary/stats widget. Fork's `Dashboard` is a separate richer surface. No code overlap — both can coexist under different nav entries. Revisit de-duplication after landing. |
| 6 | Meta-tools file structure | **Use dev's split layout (`invoke_tool.rs`, `search_tools.rs`, etc.)** | Upstream consolidated into fewer files; dev split them for maintainability. The split is better. Port the split layout; it supersedes upstream's consolidated version for these files. |
| 7 | `packages/ui` | **Additive only — do not remove upstream's `PageHeader`** | Upstream added `PageHeader`; dev added `ChipButton`, `ConfirmDialog`, `DropdownMenu`, `HoverTooltip`, `SearchField`, `AppShell`/`Sidebar`, `use-confirm.hook`, `useClickOutside`. No overlap; just merge the index. |

---

## Scope

**In:**

- All fork-specific storage migrations (dev's 016–027), renumbered to 020–031
- Web admin server stack (`crates/mcpmux-gateway/src/admin/`, Tauri admin services)
- Backend facade restructure (`apps/desktop/src/lib/backend/`)
- macOS-specific Tauri features (`macos_dock.rs`, `macos_permissions.rs`, `main_window.rs`)
- Meta-tools enhancements (invoke ergonomics, search, token budget, split module layout)
- Server account cloning (backend + UI)
- Server update policy feature (already fully implemented in `dev`)
- Tool embeddings + semantic search
- Dashboard feature
- Workspace appearances (icons, theme)
- `packages/ui` shared component additions
- `i18n` rebase + landing

**Out:**

| Item | Reason / Deferral |
| ---- | ----------------- |
| Full git history unification | No shared ancestor; the 387-commit history stays on `dev` as the canonical fork history. Only forward work goes to `main`. |
| Upstream feature back-porting to `dev` | Once reconciliation is done `dev` becomes a staging branch off `main`; no more parallel histories after Phase 9 lands. |
| Fork CI workflow changes (`ci.yml` on `dev` branch) | Upstream CI only runs on `main`; fork CI customizations stay in `dev`'s `.github/` — they don't need to land on `main`. |
| Update history log table (Phase 4 of server-update-policy) | Deferred as documented in [`server-update-policy.md`](./server-update-policy.md) — revisit after notify flow validated in prod. |

---

## Migration Renumbering Map

Upstream's 016–019 are occupied. Fork's sequence shifts:

| Old (dev) | New (main) | Content |
| --------- | ---------- | ------- |
| `016_workspace_binding_label.sql` | `020_workspace_binding_label.sql` | Workspace binding display label column |
| `017_installed_server_cloned_from.sql` | `021_installed_server_cloned_from.sql` | Server account clone lineage |
| `018_installed_server_display_name_override.sql` | `022_installed_server_display_name_override.sql` | Per-install display name override |
| `019_feature_set_member_surfaced.sql` | `023_feature_set_member_surfaced.sql` | Tool surfacing flag on feature set members |
| `020_workspace_icons.sql` | `024_workspace_icons.sql` | Workspace appearance icon storage |
| `021_tool_embeddings.sql` | `025_tool_embeddings.sql` | Semantic embedding cache table |
| `022_installed_server_default_params.sql` | `026_installed_server_default_params.sql` | Per-install default params column |
| `023_workspace_binding_client_scope.sql` | `027_workspace_binding_client_scope.sql` | Per-client workspace binding scope |
| `024_server_update_policy.sql` | `028_server_update_policy.sql` | Update policy + pinned_version columns |
| `025_server_version_cache.sql` | `029_server_version_cache.sql` | Version probe cache columns |
| `026_default_params_strategy.sql` | `030_default_params_strategy.sql` | Default params strategy enum |
| `027_server_current_version.sql` | `031_server_current_version.sql` | Current probed version cache |

---

## Phases

### Phase 1 — Foundation: shared UI library + backend facade (~half day)

Pure additive work. No conflicts with upstream — all new files.

**`packages/ui` additions:**
- `ChipButton.tsx`, `ConfirmDialog.tsx`, `DropdownMenu.tsx`, `HoverTooltip.tsx`, `SearchField.tsx`
- `use-confirm.hook.tsx`, `useClickOutside.ts`
- `AppShell.tsx`, `Sidebar.tsx` (layout layer)
- Update `packages/ui/src/index.ts` to export new components alongside upstream's `PageHeader`

**`lib/backend/` facade restructure:**
- `apps/desktop/src/lib/backend/data/fetch-api.ts` + types, helpers
- `apps/desktop/src/lib/backend/data/fetch-api.routes/` (app-settings, catalog, config-export, gateway, permissions, servers, spaces, workspaces routes)
- `apps/desktop/src/lib/backend/events/` (admin-sse-hub, tauri-adapter, use-backend-event-subscription, domain/meta/oauth/workspace event hooks + web variants)
- `apps/desktop/src/lib/backend/shell/index.ts`
- `apps/desktop/src/lib/backend/index.ts`
- Helper files: `build-info.helpers.ts`, `build-date.helpers.ts`, `desktop-shell.ts`, `monaco-setup.ts`, `analytics.ts`, `contribute.ts`

**Scripts:**
- Port fork-specific dev scripts: `dev-admin.mjs`, `build-web-admin.mjs`, `dev-web-admin.mjs`, `build-stamp.mjs`, `cf-access-env.mjs`, `admin-e2e-fixture.mjs`, `remote-gateway-smoke.mjs`, `run-with-repo-env.mjs`, `count-meta-tool-tokens.py`
- Add corresponding `package.json` entries

**Outcome:** `packages/ui` exports all shared components; any feature page can import `ChipButton`, `ConfirmDialog`, etc. without patching. The `lib/backend` façade is in place so subsequent feature PRs can import from `@/lib/backend` without wiring Tauri directly. No Rust changes; `pnpm validate` clean.

---

### Phase 2 — Storage layer: migration reconciliation + new repositories (~1 day)

The most load-bearing phase. Gets all fork-specific DB schema onto `main` without colliding with upstream's 016–019.

**Migrations:**
- Copy + rename all 12 fork migrations per the renumbering map above (020–031)
- Verify `database.rs` migration array registers them in numeric order after upstream's 019

**New domain entities (additive fields on existing entities):**
- `InstalledServer`: `cloned_from`, `display_name_override`, `default_params`, `default_params_strategy`, `update_policy`, `pinned_version`, `latest_available_version`, `version_checked_at`, `current_version`
- `WorkspaceBinding`: `label`, `client_scope`
- `FeatureSetMember`: `surfaced` flag

**New repositories:**
- `embedding_repository.rs` — tool embedding read/write
- `workspace_appearance_repository.rs` — workspace icons + theme
- Add to `repositories/mod.rs` and wire into `ApplicationServices`

**Repository updates:**
- `installed_server_repository.rs` — read/write all new columns
- `feature_set_repository.rs` — `surfaced` field
- `workspace_binding_repository.rs` — `label` + `client_scope`

**Outcome:** All fork-specific schema is on `main`, numbered sequentially after upstream's 019. A fresh DB build runs all 31 migrations in order cleanly. New repo traits compile; `pnpm test:rust:unit` passes. No UI changes in this phase — feature pages come later.

---

### Phase 3 — Web admin server stack (~1.5 days)

Ports the entire gateway-side admin server and the Tauri-side service wiring. This is the biggest Rust surface.

**`crates/mcpmux-gateway/src/admin/` (new directory):**
- `mod.rs`, `router.rs`, `server.rs`, `runtime.rs`, `live_runtime.rs`, `write_runtime.rs`
- `config.rs`, `event_hub.rs`, `bridge_context.rs`, `ui_events.rs`
- `command_bridge/` — `mod.rs`, `read.rs`, `write.rs`, `oauth.rs`, `space.rs`
- `handlers/` — `mod.rs`, `read.rs`, `write.rs`, `oauth.rs`, `events.rs`, `health.rs`, `spa.rs`, `error.rs`
- `middleware/` — `mod.rs`, `cf_access.rs`, `csrf.rs`

**Tauri-side services:**
- `apps/desktop/src-tauri/src/services/admin_server.rs`
- `apps/desktop/src-tauri/src/services/admin_write_runtime.rs`
- `apps/desktop/src-tauri/src/services/ui_events.rs`
- `apps/desktop/src-tauri/src/services/mod.rs`

**Gateway wiring:**
- `crates/mcpmux-gateway/src/server/startup.rs` — boot admin server alongside MCP gateway
- `crates/mcpmux-gateway/src/server/service_container.rs` — include admin services
- `crates/mcpmux-gateway/src/server/dependencies.rs` — admin dependency injection
- `crates/mcpmux-gateway/src/lib.rs` — expose admin module

**Public base URL:**
- `crates/mcpmux-gateway/src/public_base_url.rs`

**Outcome:** `pnpm dev:web:admin` brings up the web admin on `:45819`. Fetch requests from the browser hit the admin API and return data. SSE events flow from the gateway's `AdminUiEventBus` to the browser. The admin build (`pnpm build:web:admin`) produces a working SPA. CF Access middleware compiles (not yet tested end-to-end without a tunnel). `pnpm validate` clean.

---

### Phase 4 — macOS shell + Tauri features (~half day)

Isolated macOS-specific additions that don't depend on Phase 3.

- `apps/desktop/src-tauri/src/macos_dock.rs` — dock badge / bounce
- `apps/desktop/src-tauri/src/macos_permissions.rs` — `ensure_contacts_registered()` and other TCC calls
- `apps/desktop/src-tauri/src/main_window.rs` — window centering / focus helpers
- `apps/desktop/src-tauri/Info.plist` — `NSContactsUsageDescription`, `NSCalendarsUsageDescription`, `NSRemindersUsageDescription`, `NSAppleEventsUsageDescription` (merge with upstream plist additions)
- `apps/desktop/src-tauri/src/lib.rs` — call `ensure_contacts_registered()` from setup hook

**Tauri commands:**
- `apps/desktop/src-tauri/src/commands/workspace_appearance.rs` — workspace icon + theme commands
- Register in `commands/mod.rs`

**Outcome:** A macOS build shows the correct TCC prompts on first launch, dock badge updates on gateway events, and workspace appearance Tauri commands are callable. Linux/Windows builds are unaffected — all new code is `#[cfg(target_os = "macos")]` gated or additive. `pnpm validate` clean across platforms.

---

### Phase 5 — Meta-tools enhancements (~1 day)

Upstream shipped a base meta-tools implementation; dev has a significantly richer one. This phase supersedes the upstream files in the overlap zone and adds the new split-module structure.

**Split layout (replaces upstream's consolidated files):**
- Upstream has `meta_tools/tools.rs`, `mod.rs`, `registry.rs`, `approval.rs` — dev has split these into 20+ focused modules
- Port: `invoke_tool.rs`, `invoke_backend.rs`, `invoke_payload_parse.rs`, `invoke_result_filter.rs`, `invoke_result_shaping.rs`, `invoke_alias.rs`, `invoke_tool_tests.rs`, `invoke_result_filter_tests.rs`
- Port: `search_tools.rs`, `search_tools_index.rs`, `list_servers.rs`, `meta_tool_common.rs`
- Port: `disclosure_read.rs`, `disclosure_search.rs`, `disclosure_backend.rs`
- Port: `feature_set_tools.rs`, `bind_workspace.rs`, `set_workspace_root.rs`
- Port: `token_budget.rs`
- Port: `approval_broker.rs`, `approval_types.rs`, `approval_broker_tests.rs`
- Port: `diagnose_server.rs`, `diagnose_view.rs`, `diagnose_tests.rs`

**Gateway services:**
- `tool_discovery.rs`, `tool_discovery_index.rs`, `tool_discovery_search.rs`, `tool_discovery_tests.rs`, `tool_discovery_types.rs`
- `embedding.rs`, `embedding_warmer.rs`
- `package_version.rs`, `server_version_probe.rs`
- `discovery_rank.rs`, `prompt_discovery.rs`, `resource_discovery.rs`

**Tauri command:**
- `apps/desktop/src-tauri/src/commands/meta_tool_approval.rs` (upstream has this too — reconcile, dev's version is more complete)

**Outcome:** All meta-tool capabilities from `dev` work on `main`: invoke ergonomics (bare names, aliases, prefilled params, token budget), search with synonyms and inactive preview, `get_tool_schema` bare-name resolution, `diagnose_server`, approval flow. Token count target: ≤1,381 Claude-est tokens for the 4 advertised tools (matches dev's validated count). `pnpm test:rust` passes.

---

### Phase 6 — Server features: cloning + update policy (~1.5 days)

Two shipping features from `dev` that depend on Phase 2 migrations.

**Server account cloning:**
- `apps/desktop/src-tauri/src/commands/server_clone.rs` + register in `mod.rs`
- `apps/desktop/src/features/servers/CloneAccountModal.tsx`
- `apps/desktop/src/features/servers/UninstallSourceWithClonesDialog.tsx`
- Update `ServersPage.tsx`, `ServerActionMenu.tsx` to wire clone actions
- Update `installed_server_repository.rs` for `cloned_from` reads (migration 021 from Phase 2)

**Server update policy (migrations 028–031 already landed in Phase 2):**
- `crates/mcpmux-gateway/src/services/server_version_probe.rs` (background probe service)
- `crates/mcpmux-gateway/src/services/package_version.rs`
- Update `pool/transport/resolution.rs` — inject `@latest` / `@version` / `uv tool upgrade`
- `apps/desktop/src/features/servers/server-update-policy.helpers.ts`
- `apps/desktop/src/features/servers/server-pending-updates.helpers.ts`
- `apps/desktop/src/features/servers/ServerPendingUpdatesList.tsx` → `apps/desktop/src/features/settings/ServerPendingUpdatesList.tsx`
- `apps/desktop/src/features/settings/ServerUpdatesSection.tsx`
- `apps/desktop/src/features/settings/BuildStampPanel.tsx`, `use-build-stamp.hook.ts`
- `apps/desktop/src/components/StaleBuildBanner.tsx`
- Update `ServersPage.tsx`, `ServerActionMenu.tsx` for update badges + menu items
- Update `SettingsPage.tsx` for the Server Updates section

**Outcome:** Server cloning works end-to-end (source server spawns a named clone with independent config). Update policy (Auto/Notify/Pinned) is operational: an Auto-mode npm server always spawns `@latest`; Notify-mode servers show amber badges when a newer version is available; Pinned servers lock to a specific version. Build stamp visible in Settings. Matches the shipped behaviour documented in [`server-update-policy.md`](./server-update-policy.md).

---

### Phase 7 — Dashboard + workspace appearances (~1 day)

**Dashboard (new feature, no upstream conflict):**
- `apps/desktop/src/features/dashboard/` — `DashboardPage.tsx`, `DashboardQuickLinks.tsx`, `DashboardRecentActivity.tsx`, `DashboardServerHealth.tsx`, `DashboardStatCards.tsx`, `dashboard.helpers.ts`, `useDashboardData.ts`, `index.ts`
- Add `/dashboard` route in `App.tsx` alongside upstream's `/` `HomePage`
- Add nav entry in `Sidebar`

**Workspace appearances:**
- `crates/mcpmux-core/src/domain/workspace_appearance.rs` — new domain entity
- `crates/mcpmux-storage/src/repositories/workspace_appearance_repository.rs` (migration 024 from Phase 2)
- `apps/desktop/src-tauri/src/commands/workspace_appearance.rs` already landed in Phase 4
- `apps/desktop/src/lib/api/workspaceAppearances.ts`
- Wire appearance data into `WorkspacesPage.tsx`, `SpaceSwitcher.tsx`, `SpacePanel.tsx`
- `apps/desktop/src/lib/spaceAccent.ts` — space color accent helpers (upstream also has this; reconcile)

**Remaining UI components not yet ported:**
- `apps/desktop/src/components/SourceBadge.tsx` + `source-badge.helpers.ts`
- `apps/desktop/src/features/servers/AddServerMenu.tsx`, `ServerEnabledToggle.tsx`, `ServersCountSummary.tsx`, `ServersFiltersPopover.tsx`
- `apps/desktop/src/features/servers/server-display-name.helpers.ts`, `servers-page.helpers.ts`
- `apps/desktop/src/features/spaces/SpacePanel.tsx`
- `apps/desktop/src/features/settings/AboutSection.tsx`
- `apps/desktop/src/stores/registryStore.ts`
- Remaining hooks: `useMetaToolEvents.ts`, `useOAuthClientEvents.ts`, `useWorkspaceEvents.ts`, `useServerManager.ts` (reconcile with upstream)

**Outcome:** Dashboard route resolves and shows live server health, recent activity, and stat cards against real gateway data. Workspace switcher shows custom icons and accent colours. Servers page filters, count summary, and enabled toggle work. `pnpm validate` clean; web admin Playwright smoke passes.

---

### Phase 8 — i18n rebase + landing (~1 day)

The `i18n` branch (13 commits ahead of `dev`) covers full UI string extraction across all feature pages and is the current active work.

**Pre-requisite:** All of Phases 1–7 merged to `main`.

**Work:**
- Rebase `i18n` onto the reconciled `main` (resolve any conflicts from the feature page rewrites in earlier phases — primarily `App.tsx`, `ServersPage.tsx`, `SettingsPage.tsx`, `SpacesPage.tsx`)
- Verify i18n translation keys still resolve across all ported components (Phase 7 UI additions may need keys added)
- `pnpm test:ts` — vitest i18n harness passes
- E2E: testid selectors added in Phase 4 of the i18n plan pass against the reconciled app
- `pnpm validate` clean

**Outcome:** All user-visible strings are keyed through `react-i18next`. The `en` locale file covers all strings including Phase 5–7 additions. A second locale (if in progress) passes without missing keys. The app builds and tests clean from `main`.

---

## Files to create / modify (summary)

| Phase | File cluster | Action |
| ----- | ------------ | ------ |
| 1 | `packages/ui/src/components/common/*` (7 new components) | Create |
| 1 | `packages/ui/src/hooks/useClickOutside.ts` | Create |
| 1 | `packages/ui/src/components/layout/AppShell.tsx`, `Sidebar.tsx` | Create |
| 1 | `packages/ui/src/index.ts` | Modify — add exports |
| 1 | `apps/desktop/src/lib/backend/**` | Create (entire directory) |
| 1 | `apps/desktop/src/lib/build-info.helpers.ts`, `desktop-shell.ts`, etc. | Create |
| 1 | `scripts/dev-admin.mjs`, `build-web-admin.mjs`, etc. | Create |
| 2 | `crates/mcpmux-storage/src/migrations/020_*.sql` – `031_*.sql` | Create (renamed) |
| 2 | `crates/mcpmux-storage/src/database.rs` | Modify — register 020–031 |
| 2 | `crates/mcpmux-core/src/domain/installed_server.rs` | Modify — new fields |
| 2 | `crates/mcpmux-storage/src/repositories/embedding_repository.rs` | Create |
| 2 | `crates/mcpmux-storage/src/repositories/workspace_appearance_repository.rs` | Create |
| 3 | `crates/mcpmux-gateway/src/admin/**` | Create (~20 files) |
| 3 | `apps/desktop/src-tauri/src/services/admin_server.rs`, etc. | Create |
| 3 | `crates/mcpmux-gateway/src/server/startup.rs` | Create |
| 4 | `apps/desktop/src-tauri/src/macos_dock.rs`, `macos_permissions.rs`, `main_window.rs` | Create |
| 4 | `apps/desktop/src-tauri/Info.plist` | Modify — merge TCC keys |
| 4 | `apps/desktop/src-tauri/src/commands/workspace_appearance.rs` | Create |
| 5 | `crates/mcpmux-gateway/src/services/meta_tools/*` | Create/modify (~20 files) |
| 5 | `crates/mcpmux-gateway/src/services/tool_discovery*.rs`, `embedding*.rs` | Create |
| 6 | `apps/desktop/src-tauri/src/commands/server_clone.rs` | Create |
| 6 | `apps/desktop/src/features/servers/CloneAccountModal.tsx`, etc. | Create |
| 6 | `crates/mcpmux-gateway/src/services/server_version_probe.rs`, `package_version.rs` | Create |
| 6 | `crates/mcpmux-gateway/src/pool/transport/resolution.rs` | Modify — update policy injection |
| 6 | `apps/desktop/src/features/settings/ServerUpdatesSection.tsx`, etc. | Create |
| 7 | `apps/desktop/src/features/dashboard/**` | Create (8 files) |
| 7 | `crates/mcpmux-core/src/domain/workspace_appearance.rs` | Create |
| 7 | `apps/desktop/src/features/servers/AddServerMenu.tsx`, etc. | Create |
| 8 | `i18n` branch | Rebase onto `main` after phases 1–7 |

---

## Key files referenced

| File | Note |
| ---- | ---- |
| [`crates/mcpmux-storage/src/database.rs`](../../crates/mcpmux-storage/src/database.rs) | Migration registration array — Phase 2 primary target |
| [`crates/mcpmux-core/src/domain/installed_server.rs`](../../crates/mcpmux-core/src/domain/installed_server.rs) | Phase 2 entity extension |
| [`crates/mcpmux-gateway/src/services/meta_tools/mod.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/mod.rs) | Phase 5 meta-tools entrypoint |
| [`crates/mcpmux-gateway/src/pool/transport/resolution.rs`](../../crates/mcpmux-gateway/src/pool/transport/resolution.rs) | Phase 6 update policy injection site |
| [`apps/desktop/src/App.tsx`](../../apps/desktop/src/App.tsx) | Route wiring for Dashboard + all new pages |
| [`apps/desktop/src/stores/appStore.ts`](../../apps/desktop/src/stores/appStore.ts) | Global Zustand store — new slices per phase |
| [`packages/ui/src/index.ts`](../../packages/ui/src/index.ts) | Phase 1 export additions |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Tauri app setup — macOS hook registration (Phase 4) |

---

## Related documentation

- [`docs/planning/server-update-policy.md`](./server-update-policy.md) — full spec for Phase 6 update policy feature
- [`docs/planning/server-update-policy-audit-and-fixes.md`](./server-update-policy-audit-and-fixes.md) — Phase 6 post-ship audit
- [`docs/planning/meta-surface-lean-core.md`](./meta-surface-lean-core.md) — Phase 5 lean core decisions
- [`docs/planning/meta-tool-invoke-ergonomics.md`](./meta-tool-invoke-ergonomics.md) — Phase 5 invoke ergonomics
- [`docs/planning/i18n-react-i18next.md`](./i18n-react-i18next.md) — Phase 8 i18n feature
- [`docs/planning/i18n-react-i18next-phase-2.md`](./i18n-react-i18next-phase-2.md) — Phase 8 i18n continuation
- [`docs/planning/fork-pr-ci.md`](./fork-pr-ci.md) — CI setup on fork; each phase PR will need fork CI clean before merge
- [`docs/frontend/technical/backend-facade.md`](../frontend/technical/backend-facade.md) — Phase 1 facade architecture doc
- [`docs/backend/technical/architecture.md`](../backend/technical/architecture.md) — full backend subsystem map
