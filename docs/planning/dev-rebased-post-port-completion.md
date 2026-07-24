# dev-rebased Post-Port Completion

**Last Updated:** Jun 25, 2026
**Status:** Active — in progress
**Branch:** `dev-rebased` (HEAD: `a1e672b`)
**Depends on:** All 8 port phases landed on `dev-rebased` (clean working tree confirmed)
**Unblocks:** Web admin at `mux.joe-no-solo.com` functional; full feature parity with `dev` tip

---

## Problem

The 8-phase fork→upstream port landed cleanly on `dev-rebased` (38 commits, ~330 files). Two gaps remain before the branch is shippable:

**1. Web admin is broken.** 12 `lib/api/*` files still call Tauri `invoke()` directly. In-browser (`mux.joe-no-solo.com`) this explodes with `Cannot read properties of undefined (reading 'transformCallback')` ×62, spaces never load, and the UI shows an empty state. The fix (`apiCall`) already exists in `transport.ts` and is used by 6 other files — the remaining 12 just weren't migrated.

**2. dev branch delta not ported.** 10 commits on `dev` (`4a71eba` → `c489692`, dated Jun 17–19) postdate the Phase 5/6 planning window and were not included in the port. They cover production-grade meta-tool fixes (`get_tool_schema` bare-name resolution, prefilled params, search synonyms + inactive preview) and server-update probe corrections (stale badge clearing, PEP 508 stripping, bare npx/uvx badging). Without these, `dev-rebased` has regressed versions of both features relative to what's running on the fork.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | dev delta strategy | Deep-diff each of the 10 commits against `dev-rebased` HEAD; cherry-pick genuinely missing content | Phases 5/6 ported from the planning doc, not from `dev` tip. Some fixes landed after the planning window. Must compare file-by-file before cherry-picking to avoid doubling up on changes already ported. |
| 2 | lib/api migration target | Migrate all 12 remaining `invoke()` callers to `apiCall()` | `apiCall` already exists (`transport.ts`) and transparently uses `invoke` in Tauri and `fetch` in browser. No new pattern — extend its usage. |
| 3 | Verification scope | Full feature-by-feature pass (desktop Tauri + web admin) | The port covered ~330 files across 8 phases. Too large for spot-checking only. Per-feature walkthrough is the minimum needed to catch integration gaps. |
| 4 | Nav WIP | Already committed — no action needed | `a1e672b refactor(ui): apply fork nav renames across sidebar and dashboard` is on `dev-rebased` HEAD. Working tree is clean. |

---

## Scope

**In:**
- Audit + cherry-pick 10 `dev`-only commits (meta-tool / server-update fixes)
- Complete `lib/api` `invoke` → `apiCall` migration (12 remaining files)
- Add any missing HTTP routes to `fetch-api.routes/` surfaced during the migration
- Feature-by-feature verification of all 8 ported areas (desktop + web admin)
- ✅ **Projects metadata restore (Jun 25):** `label`/`icon` round-trip through Tauri + admin bridge; migration `032_workspace_icon_backfill`; appearance→binding unify on create/update
- ✅ **MCP meta-tool contract (Jun 25):** gateway `initialize` instructions + user docs clarify `mcpmux_search_tools` → `mcpmux_invoke_tool` path; no manifest proxy expansion

**Out:**

| Item | Reason |
| ---- | ------ |
| Full `dev` history unification | No shared ancestor — same constraint as the original port. `dev` stays as canonical fork history. |
| New features beyond what's on `dev` | This is a completion/verification pass only. New features go through normal planning. |
| E2E test authoring | Deferred — verification is manual for this pass; automated coverage tracked separately. |
| Update history log table (Phase 6 deferred item) | Already deferred in [`server-update-policy.md`](./server-update-policy.md) — revisit after notify flow validated in prod. |

---

## dev Branch Delta (10 commits not on dev-rebased)

Committed to `dev` between Jun 17–19, 2026 — after the Phase 5/6 planning doc was written.

| Commit | Message | Key files | Likely in Phase 5/6? |
| ------ | ------- | --------- | -------------------- |
| `4a71eba` | Merge PR #4 feat/meta-surface-lean-core | (merge commit) | — |
| `d741785` | docs: get_tool_schema bare-name resolution | planning docs only | Possibly — check |
| `7e1ab44` | fix: get_tool_schema resolves bare names via feature_name | `feature_set_tools.rs`, `tool_discovery_search.rs`, int tests | Check Phase 5 |
| `de5ccb4` | feat: surface prefilled params + meta-tool agent visibility | 7 files across `meta_tools/`, `tool_discovery_search.rs` | Check Phase 5 |
| `e428dcf` | feat: tool search synonyms + inactive preview | `discovery_rank.rs`, `search_tools.rs`, `tool_discovery_search.rs` | Check Phase 5 |
| `d0d0232` | fix: clear stale version badges after explicit update | `package_version.rs` (npx probe) | Check Phase 6 |
| `92e340f` | fix: strip PEP 508 version pin before uvx PyPI lookup | `package_version.rs` | Check Phase 6 |
| `c489692` | fix: persist probed current_version for bare npx/uvx | `package_version.rs` | Check Phase 6 |
| `115dc74` | test: Phase 5 verification + doc reconciliation | `admin/mod.rs`, `resolution.rs`, `services/mod.rs` | Check |
| `7414f75` | refactor: Phase 4 guards, async hygiene, headless, auto-exclusion | `ServersPage.tsx`, `resolution.rs`, `write_runtime.rs`, `package_version.rs` | Check Phase 6 |

---

## lib/api Migration Inventory

| File | Status |
| ---- | ------ |
| `apps/desktop/src/lib/api/spaces.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/gateway.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/clients.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/featureSets.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/workspaceBindings.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/serverManager.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/metaTools.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/builtinServers.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/logs.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/serverFeatures.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/featureMembers.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/clientInstall.ts` | `invoke` → `apiCall` needed |
| `apps/desktop/src/lib/api/app.ts` | ✅ Already uses `apiCall` |
| `apps/desktop/src/lib/api/settings.ts` | ✅ Already uses `apiCall` |
| `apps/desktop/src/lib/api/oauth.ts` | ✅ Already uses `apiCall` |
| `apps/desktop/src/lib/api/workspaceAppearances.ts` | ✅ Already uses `apiCall` |
| `apps/desktop/src/lib/api/serverClone.ts` | ✅ Already uses `apiCall` |
| `apps/desktop/src/lib/api/configExport.ts` | ✅ Already uses `apiCall` |

---

## Phases

### Phase 1 — dev delta: audit + cherry-pick (~half day)

Compare the 10 `dev`-only commits against `dev-rebased` HEAD, file by file. For each file touched in `dev`, diff the `dev` version against the `dev-rebased` version to determine what Phase 5/6 already ported vs what's genuinely missing.

**Meta-tools focus (commits `7e1ab44`, `de5ccb4`, `e428dcf`):**
- `feature_set_tools.rs` — `get_tool_schema` bare-name lookup via `feature_name`
- `tool_discovery_search.rs` — `prefilled_params`, `display_name` on hits, `inactive_preview` on zero results
- `discovery_rank.rs` — stopword filtering + query synonym expansion in lexical ranking
- `search_tools.rs` — synonyms, inactive preview gate
- `meta_tool_common.rs` — `display_name` on deny messages
- `invoke_tool.rs` — prefilled params on invoke deny
- `list_servers.rs` — `prefilled_params` field in list output
- `token_budget.rs` — minor fixes from `de5ccb4`
- Integration tests added by `e428dcf` and `7e1ab44` — cherry-pick if not already present

**Server-update probe focus (commits `c489692`, `92e340f`, `d0d0232`, `7414f75`, `115dc74`):**
- `package_version.rs` — stale badge clearing (npx cache lookup post-update), PEP 508 strip before PyPI lookup, bare npx/uvx `current_version` persistence
- `resolution.rs` — guards + `block_in_place` async hygiene for `run_subprocess_blocking`
- `write_runtime.rs` — `LiveGatewayWriteRuntime` implementing `update_server_package`; bulk probe concurrency cap (4 via `buffer_unordered`)
- `ServersPage.tsx` / `server-update-policy.helpers.ts` — headless auto-exclusion logic

**Outcome:** Every production-grade fix from `dev`'s `meta-surface-lean-core` merge is on `dev-rebased`. `pnpm test:rust` passes. `get_tool_schema` resolves bare names, `search_tools` returns synonym-expanded results with inactive preview, version probe correctly badges bare `npx`/`uvx` servers and clears stale badges after explicit updates.

---

### Phase 2 — lib/api invoke → apiCall migration (~half day)

Migrate all 12 remaining `lib/api/*` modules from raw `invoke()` to `apiCall()`.

**Reference pattern:** `app.ts`, `settings.ts`, or `workspaceAppearances.ts` — already migrated. The pattern is: replace `invoke<ReturnType>('command_name', args)` with `apiCall<ReturnType>('command_name', args)` and update the import from `transport.ts`.

For each file, also verify that the corresponding HTTP route exists in `apps/desktop/src/lib/backend/data/fetch-api.routes/`. If a command has no web admin route yet, add one. The route modules already cover servers, spaces, gateway, workspaces, catalog — check coverage against each migrated command.

**Files (in dependency order — `spaces.ts` first since `useDataSync` calls it on startup):**
1. `spaces.ts`
2. `gateway.ts`
3. `serverManager.ts`
4. `featureSets.ts`
5. `featureMembers.ts`
6. `clients.ts`
7. `workspaceBindings.ts`
8. `builtinServers.ts`
9. `serverFeatures.ts`
10. `metaTools.ts`
11. `logs.ts`
12. `clientInstall.ts`

**Outcome:** `pnpm dev:web:admin` — app loads, no `transformCallback` errors in console, spaces populate on startup, all nav routes render with live data. Desktop Tauri build is unaffected (`apiCall` transparently uses `invoke` in Tauri context). `pnpm validate` clean.

---

### Phase 3 — Feature-by-feature verification (~1 day)

Walk through every ported feature area against both the desktop Tauri app and web admin. Mark each item as pass/fail/regression.

**Dashboard**
- [ ] Route `/dashboard` resolves as default landing (Home removed by `5503fc0`)
- [ ] Stat cards show live server count, connection count, active spaces
- [ ] Server health section renders per-server status
- [ ] Recent activity / meta-tool audit entries populate
- [ ] Quick links navigate correctly (Servers, Spaces, Clients, Settings)
- [ ] Gateway status bar shows port + uptime

**i18n**
- [ ] All nav labels render — no missing-key warnings in console
- [ ] Spaces, Servers, Settings, Feature Sets, Workspaces, Clients page strings all keyed
- [ ] Nav label keys match the renamed superapp vocab (`myServers`, `search`, `bundles`, `projects`, `clients`) from `a1e672b`

**Spaces**
- [ ] List / create / edit / delete a space
- [ ] Base directories scope correctly to the space
- [ ] Space switcher shows accent colour
- [ ] Space panel shows server count + binding count

**Servers**
- [ ] Install a server (npm package and uvx)
- [ ] Enable / disable toggle
- [ ] Auth config saves (token / OAuth / none)
- [ ] Logs panel streams output
- [ ] Clone account — source spawns a named clone with independent config
- [ ] Display name override saves and renders in list
- [ ] Source badge renders on cloned servers
- [ ] Update policy badge (Auto / Notify / Pinned) appears
- [ ] Notify-mode server shows amber badge when newer version is available
- [ ] Explicit update on notify-mode server clears stale badge after update (`d0d0232` fix)
- [ ] Pinned server locks to specific version; `@latest` not injected
- [ ] Auto-mode server always resolves `@latest` on spawn

**Feature Sets (Bundles)**
- [ ] Create / rename / delete a feature set
- [ ] Add / remove tools
- [ ] `surfaced` flag toggles per member
- [ ] Starter protection — delete blocked with explanation

**Workspaces (Projects)**
- [ ] Folder → bundle binding create / delete
- [x] Project rename (`label`) persists via Tauri + admin — restored Jun 25
- [x] Custom icons (`icon`) persist on bindings; migration 032 backfills from appearances
- [ ] Workspace appearances: icon picker for unmapped live roots (mapped roots use binding.icon)
- [ ] Per-client binding scope (client-scope bindings sheet)

**Clients**
- [ ] List preset clients (Cursor, Claude, VS Code, Windsurf)
- [ ] OAuth grant flow completes
- [ ] Access key copy works
- [ ] Connect IDE flow finishes

**Registry / Discover**
- [ ] Browse catalog resolves (spaceId now populated after Phase 2 fix)
- [ ] Install from registry
- [ ] Search + filters work

**Builtin Servers**
- [ ] Enable / disable per space
- [ ] Enabled builtins appear in gateway tool list

**Settings**
- [ ] Gateway port config saves
- [ ] Build stamp visible (version + build date)
- [ ] Server updates section shows pending update list
- [ ] Stale build banner shows when applicable
- [ ] Analytics toggle persists

**Meta-tools** (test via MCP client or gateway harness)
- [ ] Cursor agents use `mcpmux_search_tools` → `mcpmux_invoke_tool` (not direct `user-mcpmux-<qualified_name>` unless surfaced)
- [ ] Direct `call_tool` on invokable-but-not-surfaced qualified name returns `use_invoke_tool` redirect
- [ ] Gateway `initialize` instructions document meta-tool-first backend invoke path
- [ ] `invoke_tool` resolves a bare tool name (no server prefix)
- [ ] `get_tool_schema` resolves bare name via `feature_name` (`7e1ab44` fix)
- [ ] `search_tools` expands synonyms in query
- [ ] `search_tools` returns `inactive_preview` on zero results (`e428dcf` fix)
- [ ] `list_servers` includes `prefilled_params` on each entry (`de5ccb4` feat)
- [ ] Invoke denial message includes `display_name` (`de5ccb4` feat)
- [ ] Approval dialog surfaces for gated tools
- [ ] Token budget check runs correctly

**Web admin specific**
- [ ] SSE event stream connects (`:45819/events`) — no auth errors locally
- [ ] CF Access JWT middleware does not break unauthenticated local dev access
- [ ] SPA 404 fallback routes correctly to the React app

**Outcome:** All 8 ported feature areas confirmed working across desktop Tauri + web admin. Any regressions documented as follow-up items with specific repro steps.

---

## Files to create / modify

| Phase | File | Action |
| ----- | ---- | ------ |
| 1 | `crates/mcpmux-gateway/src/services/meta_tools/feature_set_tools.rs` | Verify / patch `get_tool_schema` bare-name fix |
| 1 | `crates/mcpmux-gateway/src/services/tool_discovery_search.rs` | Verify / patch `prefilled_params`, `display_name`, `inactive_preview` |
| 1 | `crates/mcpmux-gateway/src/services/discovery_rank.rs` | Verify / patch stopword filter + synonym expansion |
| 1 | `crates/mcpmux-gateway/src/services/meta_tools/search_tools.rs` | Verify / patch synonyms + inactive preview |
| 1 | `crates/mcpmux-gateway/src/services/meta_tools/meta_tool_common.rs` | Verify / patch `display_name` on deny |
| 1 | `crates/mcpmux-gateway/src/services/meta_tools/invoke_tool.rs` | Verify / patch prefilled params on deny |
| 1 | `crates/mcpmux-gateway/src/services/meta_tools/list_servers.rs` | Verify / patch `prefilled_params` field |
| 1 | `crates/mcpmux-gateway/src/services/package_version.rs` | Verify / patch stale badge + PEP 508 strip + bare npx/uvx `current_version` persist |
| 1 | `crates/mcpmux-gateway/src/pool/transport/resolution.rs` | Verify / patch guards + `block_in_place` async hygiene |
| 1 | `crates/mcpmux-gateway/src/admin/write_runtime.rs` | Verify / patch `LiveGatewayWriteRuntime::update_server_package` |
| 1 | `apps/desktop/src/features/servers/ServersPage.tsx` | Verify / patch headless auto-exclusion UI |
| 1 | `apps/desktop/src/features/servers/server-update-policy.helpers.ts` | Verify / patch headless helper updates |
| 2 | `apps/desktop/src/lib/api/spaces.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/gateway.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/serverManager.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/featureSets.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/featureMembers.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/clients.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/workspaceBindings.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/builtinServers.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/serverFeatures.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/metaTools.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/logs.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/api/clientInstall.ts` | `invoke` → `apiCall` |
| 2 | `apps/desktop/src/lib/backend/data/fetch-api.routes/*` | Add missing routes for any unmapped commands |
| 3 | All above surfaces | Manual verification only — no file changes expected |
| — | `apps/desktop/src-tauri/src/commands/workspace_binding.rs` | ✅ Done — label/icon DTO + create/update round-trip |
| — | `crates/mcpmux-storage/src/migrations/032_workspace_icon_backfill.sql` | ✅ Done — backfill binding icons from appearances |
| — | `crates/mcpmux-gateway/src/mcp/handler.rs` | ✅ Done — meta-tool-first initialize instructions |
| — | `docs/guide/clients.mdx`, `docs/guide/tool-optimization.mdx` | ✅ Done — Cursor invoke path documented |

---

## Key files referenced

| File | Note |
| ---- | ---- |
| [`apps/desktop/src/lib/api/transport.ts`](../../apps/desktop/src/lib/api/transport.ts) | `apiCall` dispatcher — the Phase 2 migration target |
| [`apps/desktop/src/lib/backend/data/fetch-api.routes/`](../../apps/desktop/src/lib/backend/data/fetch-api.routes/) | HTTP route map for web admin — must have routes for every migrated command |
| [`apps/desktop/src/hooks/useDataSync.ts`](../../apps/desktop/src/hooks/useDataSync.ts) | Startup sync — primary breakage site (calls `spaces.ts` on mount) |
| [`crates/mcpmux-gateway/src/services/package_version.rs`](../../crates/mcpmux-gateway/src/services/package_version.rs) | Phase 1 target — stale badge, PEP 508, bare npx/uvx fixes |
| [`crates/mcpmux-gateway/src/services/meta_tools/feature_set_tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/feature_set_tools.rs) | Phase 1 target — `get_tool_schema` bare-name resolution |
| [`crates/mcpmux-gateway/src/services/tool_discovery_search.rs`](../../crates/mcpmux-gateway/src/services/tool_discovery_search.rs) | Phase 1 target — `prefilled_params`, `display_name`, `inactive_preview` |
| [`crates/mcpmux-gateway/src/services/discovery_rank.rs`](../../crates/mcpmux-gateway/src/services/discovery_rank.rs) | Phase 1 target — synonym expansion + stopword filtering |
| [`crates/mcpmux-gateway/src/admin/write_runtime.rs`](../../crates/mcpmux-gateway/src/admin/write_runtime.rs) | Phase 1 target — `LiveGatewayWriteRuntime` wiring |
| [`apps/desktop/src/stores/appStore.ts`](../../apps/desktop/src/stores/appStore.ts) | `viewSpace` null trace starting point for data load failures |
| [`apps/desktop/src-tauri/src/commands/workspace_binding.rs`](../../apps/desktop/src-tauri/src/commands/workspace_binding.rs) | Projects label/icon round-trip (Jun 25) |
| [`crates/mcpmux-storage/src/migrations/032_workspace_icon_backfill.sql`](../../crates/mcpmux-storage/src/migrations/032_workspace_icon_backfill.sql) | One-time icon backfill from appearances → bindings |

---

## Related documentation

- [`docs/planning/dev-to-main-port.md`](./dev-to-main-port.md) — the 8-phase port this doc completes
- [`docs/planning/server-update-policy.md`](./server-update-policy.md) — Phase 1 delta context: update probe spec
- [`docs/planning/server-update-policy-audit-and-fixes.md`](./server-update-policy-audit-and-fixes.md) — Phase 1 delta context: audit + fix history
- [`docs/planning/meta-surface-lean-core.md`](./meta-surface-lean-core.md) — Phase 1 delta context: meta-tool fixes source
- [`docs/frontend/technical/backend-facade.md`](../frontend/technical/backend-facade.md) — Phase 2: `apiCall` / `fetch-api` architecture
