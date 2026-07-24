# Custom Server Panel + Full Manifest Modal Overhaul

**Last Updated:** Jul 23, 2026
**Status:** Planning — ready to implement
**Depends on:** None (pure frontend UX work on top of shipped `ConfigEditorModal`/`AddServerMenu`)
**Unblocks:** A guided way to add one custom server without hand-writing JSON, and a genuinely full-screen manifest editor with working search

---

## Problem

Today's "+ Add Server" dropdown (`AddServerMenu.tsx`) has two options: **Discover from registry** and **Add custom server**. The latter opens `ConfigEditorModal` — a centered `80vh`/`max-w-4xl` modal that Monaco-edits the *entire* space JSON file. Its "Insert Server" toolbar button (`handleInsertCustomServer` → `addCustomServerDraft()`) splices a blank stdio-only stub (`{ name, command: '', args: [], env: {} }`) directly into the JSON under a `custom-server` key — the user then hand-edits that stub in the raw text.

This has three rough edges the user wants fixed:

1. **The modal isn't full height/width** — `80vh`/`max-w-4xl` leaves margin on all sides for what's meant to be a serious JSON editing surface.
2. **No visible search.** Monaco actually ships built-in Cmd+F find, and nothing in `ConfigEditorModal` disables it — but it's undocumented (toolbar hints only mention `Ctrl+S`/`Ctrl+Shift+F`) and has two real bugs: the window-level keydown handler checks `e.ctrlKey` only (not `metaKey`), so **Cmd+S/Cmd+Shift+F don't work on Mac**, and **Escape closes the entire modal** with no check for whether Monaco's find widget is open first — so hitting Escape to dismiss a find search closes the whole editor instead.
3. **Insert Server is a bad primitive.** Splicing a stdio-only stub into raw JSON and expecting the user to hand-fill it is exactly the kind of task the app should have a guided form for — especially since a very similar guided form already exists (the per-server **Configure modal** in `ServersPage.tsx`, ~540 lines of JSX rendering dynamic inputs/env/headers/args/update-policy fields) but it's wired to *installed* servers saving runtime overrides to SQLite, not to authoring a brand-new definition into the space JSON file.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Manifest modal sizing | **Near-full** — `95vh`/`95vw` with a thin margin, still reads as a modal (not true edge-to-edge) | User's preference; keeps a visible backdrop/overlay so it's still clearly a modal, not a page |
| 2 | New panel width | **Standard drawer width in Form mode** (`min-w-[420px] max-w-[480px]`, matching `SpacePanel`/`FeatureSetPanel`), **wider in JSON mode** (`min-w-[600px] max-w-[720px]`) | Raw JSON editing benefits from horizontal room; the guided form doesn't need it |
| 3 | Search | **Both** — fix the two Monaco find bugs (metaKey support, Escape-vs-find-widget conflict) *and* add an explicit toolbar search button, since relying on undocumented Cmd+F is easy to miss | User selected both; the bug fixes are needed regardless since they block Cmd+F from working correctly on Mac at all |
| 4 | New panel shell | **Model the CSS/structural shell after `WorkspaceBindingPanel`** (backdrop, sticky header/footer, `CollapsibleSection` body, slide-in-from-right) — but build a server-specific header, not reuse `PanelIdentityHeader` (that component's machine-badge/workspace-icon logic is workspace-binding-specific and doesn't apply here) | User's pick for "canonical" shell; `PanelIdentityHeader` itself is not a generic component worth forcing onto an unrelated domain |
| 5 | JSON ↔ Form switching | **One panel, segmented control/tabs** to toggle between JSON and Form mode, not separate entry points | User's pick; keeps a single component and a single save path regardless of which mode was used |
| 6 | Guided form scope | **Full field mirror** of Configure modal's set (display name, dynamic inputs, env, headers, args, update-policy-adjacent fields) **adapted for creation**, with fields explicitly marked **Required vs Optional** (see Architecture) | User's pick, with the required/optional split spelled out explicitly since creation needs fields Configure never asks for (transport type, server ID key, command-or-url) while some Configure fields don't apply pre-install (update policy, pinned version — those are meaningless until the server is actually installed) |
| 7 | Panel state ownership | **Local `useState` in `ServersPage`**, not a new global Zustand store like `bindingPanelStore` | `WorkspaceBindingPanel` needs a global store because it's triggered from live gateway events from anywhere; this panel is only ever opened from `ServersPage` (via `AddServerMenu` or from inside the manifest modal) — a global store would be unused complexity |
| 8 | Insert Server button's new behavior | Stays in the manifest modal, but instead of splicing a stub inline it **opens the same `CustomServerPanel`**, docked over the modal (higher z-index) | This is what "side panel *of the modal*" means literally — the panel isn't only reachable from the dropdown, it's also reachable from inside the full-manifest view, and both paths share one component and one save path |

---

## Scope

**In:**
- `ConfigEditorModal` ("View full server manifest" after the rewire): near-full sizing, metaKey shortcut support, Escape-vs-find-widget fix, explicit search button, Insert Server now opens the panel instead of splicing a stub
- New `CustomServerPanel` component: JSON/Form segmented toggle, JSON mode (single-entry Monaco editor with template bones + scoped schema validation), Form mode (guided creation fields), shared save path that merges the built entry into the space JSON file
- `AddServerMenu` dropdown: 3 options (Discover / Add custom server → panel / View full server manifest → existing modal, renamed)
- i18n updates for all new/changed copy in `en/servers.json`

**Out:**

| Item | Reason |
| ---- | ------ |
| Changing Configure modal's existing install-time save path (`save_server_inputs` → SQLite) | Untouched — this plan only adds a *new* creation-time flow, it doesn't touch how installed-server overrides are edited |
| Fixing `schemas/user-space.schema.json`'s `inputDef.type` enum (`text`/`password` only) vs. the richer runtime `InputDefinition` type (boolean/number/url/select/file_path/directory_path/text/secret per Configure modal) | Pre-existing schema/type drift noticed during research; unrelated to this UI work — flagging for a separate pass |
| Updating `tests/e2e/specs/server-config.spec.ts` (already stale — looks for "Add Custom Server" button text and modal title that don't match current copy, let alone the copy this plan introduces) | Per repo convention, tests aren't updated as part of feature work unless explicitly requested |
| Multi-locale i18n | Only `en` exists in this repo today |
| A global Zustand store for panel state | See Decision #7 |

---

## Architecture

### Manifest modal fixes (`ConfigEditorModal.tsx`)

Sizing (Decision #1):

```tsx
// Before
<div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
  <div className="flex h-[80vh] w-full max-w-4xl flex-col rounded-xl ...">

// After
<div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6 backdrop-blur-sm">
  <div className="flex h-[95vh] w-[95vw] flex-col rounded-xl ...">
```

Keyboard shortcuts — add `metaKey` alongside `ctrlKey`, and don't intercept Escape when Monaco's find widget is open:

```tsx
const handleKeyDown = (e: KeyboardEvent) => {
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.shiftKey && e.key === 'F') { e.preventDefault(); handleFormat(); }
  if (mod && e.key === 's') { e.preventDefault(); handleSave(); }
  if (e.key === 'Escape' && !isFindWidgetOpen()) { onClose(); }
};

function isFindWidgetOpen(): boolean {
  // Monaco's find widget renders a `.find-widget.visible` node inside the
  // editor's DOM when open — cheapest reliable check without reaching into
  // FindController internals across monaco-editor versions.
  return !!editorRef.current?.getDomNode()?.querySelector('.find-widget.visible');
}
```

Toolbar gets an explicit search button next to Format, calling Monaco's own find action so users don't have to know the shortcut:

```tsx
<button onClick={() => editorRef.current?.getAction('actions.find')?.run()} title={t('configEditorModal.searchTitle')}>
  <Search className="h-4 w-4" />
  {t('configEditorModal.search')}
</button>
```

`handleInsertCustomServer` / `addCustomServerDraft` / `nextCustomServerKey` / `CUSTOM_SERVER_BASE_KEY` are removed from this file — the Insert Server button now just opens `CustomServerPanel` (`setShowCustomServerPanel(true)`), passing the modal's `spaceId` and an `onSaved` that calls `loadConfig()` so the newly-added entry appears in the JSON view immediately.

### `CustomServerPanel` — shared shell (Decisions #4, #5, #7, #8)

CSS shell mirrors `WorkspaceBindingPanel`'s structure (backdrop + slide-in panel + sticky header/footer + scrollable body), width responsive to mode per Decision #2:

```tsx
<div className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-[55] animate-in fade-in duration-200" onClick={onClose} />
<div className={cn(
  'fixed right-0 top-0 bottom-0 bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl',
  'flex flex-col animate-in slide-in-from-right duration-300 z-[60]',
  mode === 'json' ? 'w-full max-w-[720px] min-w-[600px]' : 'w-full max-w-[480px] min-w-[420px]',
)}>
  {/* header: icon + "Add Custom Server" + close button */}
  {/* segmented control: [ Form ] [ JSON ] */}
  {/* body: flex-1 overflow-y-auto — CollapsibleSection fields (Form) or MonacoJsonEditor (JSON) */}
  {/* sticky footer: Cancel / Save */}
</div>
```

`z-[55]`/`z-[60]` deliberately sit above `ConfigEditorModal`'s `z-50` — when opened via the modal's Insert Server button, the panel needs to visually dock over the still-open manifest modal, matching "side panel *of the modal*" literally (Decision #8). When opened standalone from `AddServerMenu`, the same z-index just has nothing else to stack over.

Rendered from two call sites, differing only in the `onSaved` callback:
- `ServersPage` (standalone, via `AddServerMenu`'s "Add custom server"): `onSaved` reruns `loadData()`
- `ConfigEditorModal` (nested, via Insert Server): `onSaved` reruns `loadConfig()`

Both save through the same helper — read `readSpaceConfig`, merge the built entry into `mcpServers` at the chosen key, `saveSpaceConfig` — so there's exactly one save path regardless of entry point or mode.

### JSON mode

Single-entry Monaco editor (not the whole-file editor `ConfigEditorModal` uses) — template bones default to a stdio stub (matching today's default), with an editable "Server ID" field above the editor driving the `mcpServers` dictionary key:

```json
{
  "name": "New Custom Server",
  "command": "",
  "args": [],
  "env": {}
}
```

Schema validation reuses `schemas/user-space.schema.json`'s `$defs.serverConfig` (and its `stdioServer`/`httpServer`/`metadata`/`inputDef` sub-defs) registered directly as the root schema for this single-entry editor, rather than the whole-file schema `ConfigEditorModal` uses (which requires a top-level `mcpServers` wrapper).

### Form mode (Decision #6)

**Required:**
- Server ID (the `mcpServers` dictionary key) — collision-checked against the current config via the same `nextCustomServerKey`-style logic `ConfigEditorModal` used, now moved into the shared helpers file
- Display name
- Transport type (`stdio` | `http`) — new; Configure modal never asks this because an installed server's transport is already fixed
- Command (if stdio) or URL (if http)

**Optional:**
- Description
- Args (stdio only, newline-delimited like Configure's `argsAppend`)
- Env vars (key/value rows, like Configure's `envOverrides`)
- HTTP headers (key/value rows, http only, like Configure's `extraHeaders`)
- Input definitions builder — new; Configure modal renders *values* for inputs a server's definition already declares, but a brand-new custom server has no existing input schema to fill in. This needs a small "define your inputs" row editor: id / label / type (`text`/`password` per the JSON schema) / required checkbox / secret checkbox, appended to the entry's `metadata.inputs` array
- Default params JSON (like Configure's `defaultParamsJson`)

Not carried over from Configure modal: **update policy / pinned version** — both are meaningless before the server is actually installed (they govern update behavior on an existing SQLite row), so they're excluded entirely rather than shown disabled.

On save, Form mode assembles the same entry shape JSON mode produces (`buildServerEntryFromForm()` → the `stdioServer`/`httpServer` shape from the schema) and goes through the identical merge-and-save helper.

### Shared helpers (`custom-server-entry.helpers.ts`)

```ts
export function nextCustomServerKey(servers: Record<string, unknown>, base?: string): string { ... }
export function buildServerEntryFromForm(form: CustomServerFormState): Record<string, unknown> { ... }
export function upsertServerEntry(
  config: SpaceConfigJson,
  key: string,
  entry: Record<string, unknown>,
): SpaceConfigJson { ... }
export const SINGLE_SERVER_ENTRY_SCHEMA = { /* derived from user-space.schema.json $defs.serverConfig */ };
```

`nextCustomServerKey` generalizes today's `ConfigEditorModal`-local version (same collision-suffix logic, `-2`, `-3`, …). `upsertServerEntry` generalizes `addCustomServerDraft` to accept an already-built entry instead of always inserting a blank stub.

---

## Files to Create

| File | Purpose |
| ---- | ------- |
| [`apps/desktop/src/features/servers/CustomServerPanel.tsx`](../../apps/desktop/src/features/servers/CustomServerPanel.tsx) | New shared panel — JSON/Form toggle, both save through one path. Rendered from `ServersPage` and from `ConfigEditorModal` |
| [`apps/desktop/src/features/servers/custom-server-entry.helpers.ts`](../../apps/desktop/src/features/servers/custom-server-entry.helpers.ts) | `nextCustomServerKey`, `buildServerEntryFromForm`, `upsertServerEntry`, single-entry JSON schema fragment |

## Files to Modify

| File | Change |
| ---- | ------ |
| [`apps/desktop/src/components/ConfigEditorModal.tsx`](../../apps/desktop/src/components/ConfigEditorModal.tsx) | Near-full sizing (L288-295); `metaKey` support + Escape/find-widget fix (L260-280); new search toolbar button; remove `handleInsertCustomServer`/`addCustomServerDraft`/`nextCustomServerKey`/`CUSTOM_SERVER_BASE_KEY` (L26-57, L174-186) in favor of opening `CustomServerPanel` |
| [`apps/desktop/src/features/servers/AddServerMenu.tsx`](../../apps/desktop/src/features/servers/AddServerMenu.tsx) | Third `DropdownMenuItem` ("View full server manifest"); props become `onDiscover` / `onCustom` (→ panel) / `onViewManifest` (→ existing modal) |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | New `customServerPanelOpen` state alongside `editConfigSpace` (L397); wire `AddServerMenu`'s three callbacks at both mount points (L1663-1666, L1703-1706); render `CustomServerPanel` |
| [`apps/desktop/src/locales/en/servers.json`](../../apps/desktop/src/locales/en/servers.json) | `addMenu.viewManifest`/`viewManifestDesc`; updated `addMenu.custom`/`customDesc` copy (moves from "Edit your Space JSON..." to "guided setup" framing); new `customServerPanel.*` namespace; `configEditorModal.search`/`searchTitle`; updated `configEditorModal.keyboardHints` |

---

## Phases

### Phase 1 — Manifest modal: fullscreen sizing + search fixes (~half day)

- Resize `ConfigEditorModal` per Decision #1 (`95vh`/`95vw`, adjusted overlay padding)
- Add `metaKey` support to the window keydown handler alongside `ctrlKey`
- Add `isFindWidgetOpen()` check so Escape doesn't close the modal while Monaco's find widget is open
- Add explicit search toolbar button wired to `actions.find`
- Update `configEditorModal.keyboardHints` copy

**Outcome:** Opening "Add custom server" (still the current entry point at this phase) shows a near-fullscreen modal; Cmd+F/Cmd+S/Cmd+Shift+F all work on Mac; pressing Escape while a find search is active dismisses the search, not the modal; a visible search button exists in the toolbar. Ships independently of the panel work below.

### Phase 2 — Shared helpers + panel shell + JSON mode (~1 day)

- `custom-server-entry.helpers.ts`: `nextCustomServerKey`, `upsertServerEntry`, single-entry JSON schema fragment (ported from `ConfigEditorModal`'s inline logic + `user-space.schema.json`'s `$defs`)
- `CustomServerPanel.tsx`: shell (backdrop, sticky header/footer, segmented Form/JSON control, responsive width per mode), JSON mode fully functional (template bones, single-entry schema-validated Monaco editor, editable Server ID field, save via `upsertServerEntry`)
- Form mode renders as a placeholder/stub at this phase (segmented control switches, but fields land in Phase 3)
- Not wired to any entry point yet — build and verify standalone (e.g. temporarily mounted behind a dev-only toggle) or wire directly to `ServersPage`'s "Add custom server" as a working draft ahead of Phase 4's full rewire

**Outcome:** JSON mode end-to-end: opening the panel, editing a single-entry JSON stub (stdio bones by default), and saving correctly merges it into the space's `mcpServers` under the chosen key without touching other entries.

### Phase 3 — Panel Form mode (~1 day)

- Required fields: Server ID, display name, transport type picker, command-or-url (conditional on transport)
- Optional fields: description, args (stdio), env vars, HTTP headers (http), default params JSON
- Input-definitions builder: add/remove rows (id, label, type, required, secret) appended to `metadata.inputs`
- `buildServerEntryFromForm()` assembles the same entry shape JSON mode produces; save path is identical

**Outcome:** Switching to Form mode and filling in a stdio or HTTP server (with or without custom input definitions) produces the same on-disk JSON shape as hand-writing it in JSON mode would — both modes are interchangeable views onto one entry.

### Phase 4 — Wire entry points (~half day)

- `AddServerMenu`: add third option, retarget `onCustom` to open `CustomServerPanel` directly, add `onViewManifest` → existing `ConfigEditorModal` behavior
- `ConfigEditorModal`: Insert Server button opens `CustomServerPanel` nested (higher z-index), `onSaved` reruns `loadConfig()`
- `ServersPage`: wire all three `AddServerMenu` callbacks at both mount points (toolbar + empty-state)
- i18n: finalize all new/changed copy

**Outcome:** The dropdown has exactly the three options described in the request — Discover (unchanged), Add custom server (opens the panel directly), View full server manifest (opens the fullscreen modal from Phase 1) — and the manifest modal's Insert Server button opens the same panel docked over it, saving back into the currently-open manifest view.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`apps/desktop/src/components/ConfigEditorModal.tsx`](../../apps/desktop/src/components/ConfigEditorModal.tsx) | Current sizing (L288-295), keyboard handler (L260-280), Insert Server logic to remove (L26-57, L174-186), Monaco mount/fallback |
| [`apps/desktop/src/components/monaco-json-editor.component.tsx`](../../apps/desktop/src/components/monaco-json-editor.component.tsx) | Shared Monaco wrapper — `BASE_JSON_OPTIONS` has no find-related overrides, confirms built-in Cmd+F isn't disabled; `automaticLayout: false` + `ResizeObserver` layout pattern the panel's JSON mode should reuse |
| [`schemas/user-space.schema.json`](../../schemas/user-space.schema.json) | `$defs.serverConfig`/`stdioServer`/`httpServer`/`metadata`/`inputDef` — source for both the whole-file schema (`ConfigEditorModal`) and the new single-entry schema (`CustomServerPanel` JSON mode) |
| [`apps/desktop/src/components/ServerDefinitionModal.tsx`](../../apps/desktop/src/components/ServerDefinitionModal.tsx) | `buildEditableEntry()` (L48-70) — reference shape for a single server's editable JSON, same stdio/http branching the panel's JSON mode template follows |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | Configure modal (`ConfigModalState` L301, `configModal` render L2190-2728) — full field-by-field reference for Form mode's dynamic inputs/env/headers/args rendering; `editConfigSpace` state (L397) and `AddServerMenu` mount points (L1663-1666, L1703-1706) |
| [`apps/desktop/src/features/servers/AddServerMenu.tsx`](../../apps/desktop/src/features/servers/AddServerMenu.tsx) | Current 2-option dropdown, `DropdownMenuItem` icon/label/description pattern to extend to 3 |
| [`apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx) | Shell reference (L910-920 backdrop+panel wrapper, L921-965 sticky header, L967 scrollable body, L1263-1304 sticky footer) — `PanelIdentityHeader` (L148-228) itself is NOT reused, only the outer structural pattern |
| [`docs/planning/sidesheet-panel-identity-header.md`](./sidesheet-panel-identity-header.md) | Documents the completed sidesheet refactor `WorkspaceBindingPanel` is the result of |
| [`apps/desktop/src/stores/bindingPanelStore.ts`](../../apps/desktop/src/stores/bindingPanelStore.ts) | Global panel-store pattern considered and rejected for this feature (Decision #7) — reference for why it's overkill here |
| [`apps/desktop/src/locales/en/servers.json`](../../apps/desktop/src/locales/en/servers.json) | `addMenu.*` and `configEditorModal.*` string conventions (short label + `*Desc` suffix) to extend |
| [`tests/e2e/specs/server-config.spec.ts`](../../tests/e2e/specs/server-config.spec.ts) | Already stale before this plan (wrong button text/modal title) — flagged, not fixed, per Scope/Out |

---

## Related Documentation

- [`sidesheet-panel-identity-header.md`](./sidesheet-panel-identity-header.md) — the sidesheet refactor this plan's panel shell borrows structure from
- [`clone-auth-header-config-editing.md`](./clone-auth-header-config-editing.md) — separate in-flight work on Configure modal's `extra_headers`/`env_overrides` editors; this plan's Form mode mirrors those same field patterns for creation rather than editing an installed server
