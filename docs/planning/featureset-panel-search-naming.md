# FeatureSet panel: search, names, bulk select

**Last Updated:** Nov 2, 2026
**Status:** Plan ready, not started
**Branch:** `bugfix/25-featureset-panel-search-naming`
**Issue:** [crimsonsunset/mcp-mux#25](https://github.com/crimsonsunset/mcp-mux/issues/25)
**Depends on:** `sync/upstream-2026-10` ([PR 24](https://github.com/crimsonsunset/mcp-mux/pull/24), open). The Auto-include card and tool-size warning only exist there. `main` has an 806-line panel without them.

## Flow state

| Field | Value |
|---|---|
| Gate | 2 (plan ready) |
| Ticket | crimsonsunset/mcp-mux#25 |
| Branch | bugfix/25-featureset-panel-search-naming |
| Repos | mcp-mux |
| Isolation | worktree |
| Worktrees | /Users/joe/Desktop/Repos/Personal/jsg-tech-check-wt/featureset-panel/mcp-mux |
| Base | origin/sync/upstream-2026-10 @ dd67cb34 |
| PR target | `sync/upstream-2026-10` (not `main`) |
| QA mode | browser-qa (UI change, one Rust mapping change covered by cargo tests) |
| Gates | all |
| Updated | 2026-11-02 |

---

## Problem

The FeatureSet detail panel (`FeatureSetPanel.tsx`, 947 lines) looks broken in three ways.

- **Search shows nothing.** Every server group starts collapsed (L132) and typing never expands one. The filter runs, but the matching tools stay hidden. It also skips the server ID, so "railway" returns "No features found".
- **Names are raw.** Server rows print `group.serverId`. Tool rows print the bare MCP name because only resources ever get a `display_name`. The header prints `ID: <uuid>`.
- **Bulk controls lie.** Select All and the per-server toggle ignore the search filter. The toggle looks like an on/off switch but bulk-selects. Features from unavailable servers are hidden but still counted, re-saved and dropped by Deselect All.

## Decisions

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | Where server names come from | Panel calls `listInstalledServers(spaceId)` and maps through `resolveInstalledDisplayName` | The helper already mirrors Rust `display_name()`. No change to the Tauri or admin response shapes. The Workspaces page keeps its own naming gap (see Out) |
| 2 | Tool and prompt titles | Rust `convert_to_feature` fills `display_name` from `title`, then `annotations.title` (tools only) | `display_name` is already in both response shapes and the UI already prefers it. Raw `feature_name` stays visible as muted text because agents call that name |
| 3 | Search expansion | While a query or filter is active, expand every group with a match, up to 150 matching rows total. Above that, groups stay collapsed and show their match counts plus a "narrow your search" hint | No virtualization library. 150 rows is cheap. The cap is the ceiling and virtualization is the upgrade path |
| 4 | Search fields | Feature name, display name, description, server display name, server ID. Trimmed, lowercased once | Typing a server name is the most natural query |
| 5 | Bulk scope | Select All and the per-server control act on the visible, available features. With a filter active the label reads "Select N matches" | Matches the usual expectation. No confirm: nothing persists until Save and the new close guard protects unsaved edits |
| 6 | Per-server control | Tri-state checkbox (`role="checkbox"`, `aria-checked` true, false or mixed). Click on mixed selects all visible | Same visual language as the feature rows and the only control that shows "some" honestly |
| 7 | Unavailable features | Load with `includeUnavailable`. Show greyed, no bulk actions touch them, header counts cover available features only, Save preserves their membership | Fixes the hidden-member count drift and the silent drop on Deselect All without a new status fetch |
| 8 | Selection filter | All / Selected / Unselected segmented control beside the search box | Lets you see what is in the set without opening each server |
| 9 | Header | Inline editable name, description and icon. One Save in the footer saves members and general info together. The accordion goes away | Removes the exclusive accordion and the second Save button. UUID moves to a tooltip on the type badge |
| 10 | Close guard | Panel owns its overlay and Escape. A dirty panel (selection edited or general info changed) asks before closing | Today overlay and X discard silently. Moving the overlay into the panel is the smallest way to let it see its own dirty state |
| 11 | Ordering | Servers by display name. Inside a server: tools, then prompts, then resources, each by name. Type pill stays, no subgroups | Tools are the bulk and should be first. Subgroups are a larger UI change with no request |
| 12 | Progress bars | Header and server bars render only when the selected count is above 0. The same change goes into the copy in `WorkspacesPage.tsx` | The empty track on every 0/N row is noise |
| 13 | File split | `FeatureSetPanel.tsx` becomes a shell. Pieces under 300 lines each in kebab-case files | Repo rule. Nearly every fix touches the same code, so the split goes first |
| 14 | Tests | No new tests. `FeatureSetPanelAuto.test.tsx` gets a `listInstalledServers` mock because the panel now calls it. Keep the test ids listed below | Standing rule: tests are not written unless asked. Existing ones must still pass |
| 15 | Dead code | Delete `isConfigurable` and its dead branches, the unused `_feature` argument and the stale comments | Always true since the sets became member-driven |

## Scope

**In:** everything in the table above, plus the Rust change in Decision 2 and the dead fallback below.

**Out:**

| Item | Reason |
|---|---|
| Server display names on the Workspaces page | It reads `server_alias ?? server_id` from its own payload. It gets tool titles for free from Decision 2. Names there need the Rust payload to carry a server name, which is a separate change: [#26](https://github.com/crimsonsunset/mcp-mux/issues/26) |
| Flagging clone servers that expose the same tools (gondor and rivendell) | Needs a product call on what "duplicate" means for clones |
| Consolidating the `cached_definition` JSON parse (already in `ServersPage`, `dashboard.helpers.ts`, `server-pending-updates.helpers.ts`) | This work reads the icon from it a fourth time through one small helper. Consolidation is a separate refactor: [#27](https://github.com/crimsonsunset/mcp-mux/issues/27) |
| Virtualizing the list | Decision 3 caps what auto-expansion renders |
| Verifying that uninstalling a server deletes its feature rows (`delete_by_server` exists) | Orphan rows fall back to the last segment of `server_id`. Not verified |

## Architecture

New files, all under `apps/desktop/src/features/featuresets/`:

| File | Responsibility |
|---|---|
| `FeatureSetPanel.tsx` | Shell: overlay, Escape, header, warning, footer, save. Composes the pieces |
| `feature-set-header.component.tsx` | Inline name, description, icon, type badge, close button |
| `feature-set-features.helpers.ts` | Pure functions: build server groups, label map, sort, filter, match, selection scope, counts. Uses `resolveInstalledDisplayName` |
| `feature-set-selection.hooks.ts` | `useFeatureSetSelection`: selected and surfaced ids, scoped toggles, dirty flag, seeding from members |
| `feature-set-features-toolbar.component.tsx` | Search box, All/Selected/Unselected control, Select All, match count |
| `feature-set-server-group.component.tsx` | Group row: chevron, icon, name, count badge, tri-state checkbox, bar |
| `feature-set-feature-row.component.tsx` | One feature row: checkbox, type icon, title, raw name, Surface button |
| `feature-set-features-section.component.tsx` | Added during the split. Owns search, filter and expansion state, composes toolbar and groups |
| `feature-set-catalog.hooks.ts` | Added. Loads features (with unavailable) and installed servers in parallel |
| `feature-set-general-info.hooks.ts` | Added. Name, description and icon values with a saved baseline for the dirty check |
| `feature-set-notices.component.tsx` | Added. Auto-include card and tool-count warning, moved out to keep the shell under 300 lines |
| `feature-set-footer.component.tsx` | Added. Delete, auto-exit note and the single Save |

Data flow: the panel loads `listServerFeatures(spaceId, true)` and `listInstalledServers(spaceId)` in parallel. The helpers join them into `ServerGroup { serverId, label, icon, features, availableCount }`. Search state lives in the shell and feeds `filterGroups`. The hook owns selection, so Select All and the per-server control receive the already-filtered visible ids.

Rust: `ToServerFeature` gets `display_title(&self) -> Option<String>` (`Tool`: `title` then `annotations.title`; `Prompt`: `title`). `convert_to_feature` calls `with_display_name` when it returns `Some`. Storage already keeps the value (`display_name = COALESCE(?6, display_name)` on upsert), so titles appear after a server's next discovery.

**Regression to remove with it:** `list_servers.rs:138` copies a tool's `display_name` into a server's fallback name when no installed record exists. It never fired because tools had no `display_name`. After this change it would label a server with a tool title. Delete that fallback and the `feature_display_name` tuple slot in the same phase.

## Files to modify

| File | Change |
|---|---|
| [`crates/mcpmux-gateway/src/pool/features/conversion.rs`](../../crates/mcpmux-gateway/src/pool/features/conversion.rs) | `display_title` on the trait, impls for `Tool` and `Prompt`, call in `convert_to_feature` |
| [`crates/mcpmux-gateway/src/services/meta_tools/list_servers.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/list_servers.rs) | Delete the tool-title fallback for server names |
| [`apps/desktop/src/lib/api/serverFeatures.ts`](../../apps/desktop/src/lib/api/serverFeatures.ts) | `listServerFeatures(spaceId, includeUnavailable?)`. The route in `catalog.routes.ts` already forwards the flag |
| [`apps/desktop/src/features/featuresets/FeatureSetPanel.tsx`](../../apps/desktop/src/features/featuresets/FeatureSetPanel.tsx) | Reduced to the shell |
| [`apps/desktop/src/features/featuresets/FeatureSetsPage.tsx`](../../apps/desktop/src/features/featuresets/FeatureSetsPage.tsx) | Drop its overlay and close handling now that the panel owns them. Keep `featureset-panel-overlay` |
| [`apps/desktop/src/locales/en/featuresets.json`](../../apps/desktop/src/locales/en/featuresets.json) | Add filter, match-count, select-matches, unavailable and discard-changes strings. Remove `generalInfo`, `enableAll`, `disableAll` if unused |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | Hide the per-server progress bar at 0 (L1783 to 1804) |
| [`tests/ts/components/FeatureSetPanelAuto.test.tsx`](../../tests/ts/components/FeatureSetPanelAuto.test.tsx) | Add the `listInstalledServers` mock |

## Test ids to keep

`featureset-select-all`, `featureset-server-group-*`, `featureset-panel-close`, `featureset-panel-overlay`, `featureset-panel-name`, `featureset-panel-description`, `featureset-panel-icon`, `featureset-panel-save-changes`, `featureset-save-leaves-auto`, `featureset-auto-card`, `featureset-auto-switch`, `featureset-panel-tools-warning`, `surface-toggle-*`. `featureset-panel-save-general` goes away with its button. Nothing outside the panel references it.

## Phasing

### Phase 1: Titles from the backend (about 1 hour)
- `display_title` on `ToServerFeature`, impls for `Tool` and `Prompt`, wired into `convert_to_feature`
- Delete the server-name fallback in `list_servers.rs`
- `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`, `cargo nextest run -p mcpmux-gateway --lib`

**Outcome:** A tool with an MCP `title` produces a `ServerFeature` whose `display_name` is that title, a tool with only `annotations.title` falls back to it, and `mcpmux_list_servers` no longer reads feature names for server labels.

### Phase 2: Split and friendly names (about half a day)
- Extract the seven files above with the current behavior and test ids
- Load installed servers, join names and icons, sort servers by display name
- Delete the dead code from Decision 15, memoize groups and filtered groups
- Header shows the description, UUID in a tooltip
- `listServerFeatures` gains `includeUnavailable`

**Outcome:** Opening the panel shows server rows labelled with their display names and icons in alphabetical order. Search and selection behave exactly as before. Every file is under 300 lines. `pnpm typecheck` and `FeatureSetPanelAuto.test.tsx` pass.

### Phase 3: Search and selection semantics (about half a day)
- Search over the five fields in Decision 4, auto-expand under the 150 row cap, hint above it
- All / Selected / Unselected control, match count under the search box
- Select All and the tri-state per-server control scoped to visible available features
- Unavailable features greyed and excluded from bulk actions, counts over available features, membership preserved on save

**Outcome:** Typing "railway" opens that server with its tools listed. Searching "list" with Select All selects only the visible matches and the header count moves by that number. A server with unavailable members keeps them after Save and after Deselect All.

### Phase 4: Header, layout and close guard (about half a day)
- Inline name, description and icon in the header, single footer Save, remove the accordion and the nested 500px scroller
- Tools first inside a server, progress bars hidden at 0 here and in `WorkspacesPage.tsx`
- Panel-owned overlay, Escape to close, confirm on close when dirty, `aria-label` on the close button
- Locale strings added and unused ones removed

**Outcome:** Editing the name then clicking the overlay asks before discarding. Escape closes a clean panel. Zero-selected servers show no bar. There is one scroll container and one Save button.

### Phase 5: Reconcile and validate (about 1 hour)
- Local CI mirror: `pnpm validate` (fmt, clippy, check, eslint, typecheck), `pnpm test:ts`, `cargo nextest run -p mcpmux-gateway --lib`
- Plan fidelity pass against the diff
- Browser QA is Stage 4: it needs `pnpm dev:web:admin` or `pnpm dev:admin`, which Joe starts

**Outcome:** All local jobs the PR workflow runs are green and this doc matches the diff.

## Key files referenced

| File | Note |
|---|---|
| `apps/desktop/src/features/featuresets/FeatureSetPanel.tsx` | The whole panel. Search L160, group label L754, toggle L786, select all L246 |
| `apps/desktop/src/features/servers/server-display-name.helpers.ts` | `resolveInstalledDisplayName`, mirrors Rust `display_name()` |
| `apps/desktop/src/types/registry.ts` | `InstalledServerState.cached_definition` carries the definition `icon` |
| `crates/mcpmux-gateway/src/pool/features/conversion.rs` | Only `resource_to_feature` sets a display name today |
| `crates/mcpmux-gateway/src/pool/routing.rs` | `RoutedResource.name` reads `display_name`. Resources only, unaffected |
| `crates/mcpmux-storage/src/repositories/server_feature_repository.rs` | Upsert keeps `display_name` via `COALESCE` |
| `apps/desktop/src/features/workspaces/WorkspacesPage.tsx` | Sibling grouped view. Copies the progress bar style |
| `tests/ts/components/FeatureSetPanelAuto.test.tsx` and `tests/e2e/specs/featureset.wdio.ts` | Test ids the split must keep |

## Related

- [Issue 25](https://github.com/crimsonsunset/mcp-mux/issues/25)
- [`upstream-sync-oct-2026.md`](./upstream-sync-oct-2026.md): the sync branch this builds on
- [`custom-server-panel-and-manifest-modal.md`](./custom-server-panel-and-manifest-modal.md): states the panel is 420 to 480px wide. The real panel is `max-w-[45%] min-w-[600px]`. Stale, left alone here
