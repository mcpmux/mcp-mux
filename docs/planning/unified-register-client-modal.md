# Unified Register Client Modal — Cursor Preset + Generic

**Last Updated:** Jul 23, 2026
**Status:** Planning — not started
**Branch:** `dev-rebased`
**Depends on:** `upstream-client-mapping-reconciliation.md` Phase 1 (`mcpk_` API-key auth, `register_api_key_client`/`create_client_api_key`), `cursor-workspace-routing-bridge.md` Phase 2 (`buildCursorBridgeMcpJson`, the Cursor bridge concept this consolidates)
**Unblocks:** A single, less confusing Connections entry point for minting API-key clients — no backend changes required

---

## Problem

The Connections page currently exposes two API-key registration surfaces that both call the same backend (`register_api_key_client` in [`oauth.rs`](../../apps/desktop/src-tauri/src/commands/oauth.rs)) but look unrelated to a first-time user:

1. A big always-visible **"Global Cursor setup (no per-repo files)"** card ([`CursorBridgeSection.tsx`](../../apps/desktop/src/features/clients/CursorBridgeSection.tsx)) with its own "Generate global config" button
2. A **"Register client (API key)"** button in the page header that opens a separate generic modal ([`RegisterApiKeyClientModal.tsx`](../../apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx))

Dig research (`dig-and-ask` session, Jul 23) confirmed these were built in separate planning tracks weeks apart — `upstream-client-mapping-reconciliation.md` shipped the generic modal as the general inbound-auth primitive; `cursor-workspace-routing-bridge.md` added the Cursor card as a follow-on that reuses the same primitive to assemble a paste-ready `~/.cursor/mcp.json` snippet instead of showing a bare key. The bridge planning doc even flagged this ambiguity itself: its file table lists `CursorBridgeSection.tsx` "(or fold into `ClientsPage.tsx`)" — the fold-in was never done.

There's no in-app guidance distinguishing the three ways to connect Cursor (OAuth deep link on the empty state, the global bridge card, per-repo Workspaces install), and the generic modal's Space/Machine pickers are plain `<select>` elements while other parts of the app (e.g. `SpaceSwitcher.tsx`) use a nicer custom dropdown pattern.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Surface count | Consolidate into **one** modal with client-type presets (Cursor / Generic), reachable only from the header "Register client" button | User explicitly asked for this over keeping two surfaces; removes the redundant always-visible card. |
| 2 | Default preset | **Cursor tab is the default** view when the modal opens | Matches current de-facto behavior (the big card is the first thing users see today) — don't regress the primary path's prominence just because it moved into a modal. |
| 3 | Path education | Add a collapsible **"Which should I use?"** comparison table directly above the submit button row, not a separate doc link | User wants the explainer inline, not requiring a context switch to docs. |
| 4 | Regenerate semantics | User picks **rotate key on existing client** vs **create new client** at regenerate time | Today's Cursor card always mints a new `cursor-global-bridge` client, silently accumulating duplicates in the Connections list. Existing `create_client_api_key` (rotation) already exists and is unused by this flow — wire it in as a choice, don't force one behavior. |
| 5 | Machine assignment for Cursor preset | **Add it** — Cursor preset gets the same machine picker the Generic preset already has | Today only the Generic modal exposes machine tagging; no technical reason the Cursor client can't be tagged too, and per-device machine header routing already supports it. |
| 6 | Dropdown styling | Build a shared **`SearchableSelect`** combobox (typeahead) from existing `DropdownMenu` + `SearchField` primitives in `@mcpmux/ui`, replacing the raw `<select>` Space/Machine pickers | No searchable combobox exists anywhere in the codebase today — this is new shared component work, not a reuse. Built from primitives already in `@mcpmux/ui` rather than a new dependency. |
| 7 | Backend changes | **None** | `register_api_key_client`, `create_client_api_key`, `setClientMachineId` already support everything the unified modal needs. This is a frontend-only consolidation. |

---

## Scope

**In:**

- New `SearchableSelect` component in `packages/ui/src/components/common/` (typeahead-filterable dropdown, built from `DropdownMenu` + `SearchField`)
- Reworked `RegisterApiKeyClientModal.tsx` with a Cursor/Generic tab switch, Cursor defaulting to selected
- Cursor tab: name (defaults to `cursor-global-bridge`, editable), Space lock, machine picker (new), snippet output via existing `buildCursorBridgeMcpJson()`
- Generic tab: unchanged behavior (name, Space lock, machine, raw key output), restyled with `SearchableSelect`
- Inline "Which should I use?" expander with a comparison table (Cursor bridge / Generic key / per-repo Workspaces install / OAuth deep link)
- Regenerate flow: explicit rotate-vs-new-client choice when a client for the current name/type already exists
- Removal of the always-visible `CursorBridgeSection` card from `ClientsPage.tsx`; deletion of `CursorBridgeSection.tsx`
- i18n updates to `clients.json` for the merged copy

**Out:**

| Item | Reason / Deferral |
| ---- | ------------------ |
| Backend changes to `register_api_key_client` or key rotation | Decision 7 — existing commands already cover every code path this modal needs. |
| Changes to OAuth deep-link onboarding (`ConnectIDEs`, empty-state) | Out of scope — that's a different auth mechanism (dynamic OAuth client, not `mcpk_`), only referenced in the comparison table for context. |
| Changes to per-repo Workspaces install (`workspace_install.rs`, `WorkspaceInstallPanel`) | Out of scope — stays as documented fallback per `cursor-workspace-routing-bridge.md` Decision 4; only referenced in the comparison table. |
| Deduplication/cleanup of already-existing duplicate `cursor-global-bridge` clients in users' live databases | One-time data cleanup, not a UI concern — the new rotate option only prevents *future* duplicates. |
| `SearchableSelect` adoption elsewhere in the app (e.g. `workspace-binding-form.component.tsx`'s `Picker`, `RegistryPage`'s `FilterDropdown`) | New component ships scoped to this modal only; broader adoption is a separate follow-up once it's proven here. |

---

## Architecture

### Before → after

```text
Before:
  ClientsPage
  ├── Header "Register client" button → RegisterApiKeyClientModal (generic only)
  └── Always-visible CursorBridgeSection card → its own "Generate global config" flow

After:
  ClientsPage
  └── Header "Register client" button → RegisterApiKeyClientModal
        ├── Tab: Cursor (default) → mints mcpk_ key → buildCursorBridgeMcpJson() snippet
        ├── Tab: Generic          → mints mcpk_ key → raw key + Bearer instructions
        └── "Which should I use?" expander (comparison table, above submit row)
```

Both tabs call the same `registerApiKeyClient()` / `createClientApiKey()` / `setClientMachineId()` frontend wrappers — no new Tauri commands.

### Comparison table content (for the in-modal expander)

| Path | Best for | Setup |
| ---- | -------- | ----- |
| Cursor bridge (this modal, Cursor tab) | Cursor, all repos, zero per-repo files | Paste one snippet into `~/.cursor/mcp.json` |
| Generic API key (this modal, Generic tab) | Headless/CI/remote clients that can't complete browser OAuth | Copy key into client config manually |
| Per-repo Workspaces install | Cursor users who don't want an `npx`/Node dependency | One-click install per folder in Workspaces |
| OAuth deep link | VS Code, Claude Code, other IDEs with reliable `roots` reporting | One click from the empty-state onboarding |

### `SearchableSelect` shape

```ts
interface SearchableSelectProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: Array<{ value: T; label: string; icon?: string }>;
  placeholder: string;
  onCreateNew?: () => void;   // e.g. "+ New machine…" affordance
  disabled?: boolean;
  testId?: string;
}
```

Built on top of the existing `DropdownMenu`/`DropdownMenuTrigger`/`DropdownMenuContent` primitives ([`packages/ui/src/components/common/DropdownMenu.tsx`](../../packages/ui/src/components/common/DropdownMenu.tsx)) for the trigger/panel/click-outside/escape behavior, and `SearchField` ([`packages/ui/src/components/common/SearchField.tsx`](../../packages/ui/src/components/common/SearchField.tsx)) for the filter input rendered at the top of the panel.

### Regenerate decision flow

```
User clicks "Regenerate" on an existing Cursor-tab client
  → if a client named `cursor-global-bridge` (or the current custom name) already exists:
      show two options:
        - "Rotate key" → createClientApiKey(existingClientId) → new snippet with new key, same client
        - "New client" → registerApiKeyClient(name, ...) → new client, old one left as-is (user can revoke separately)
  → if no existing client: skip the choice, just registerApiKeyClient(name, ...) as today
```

---

## Files to create / modify

| Area | File | Action |
| ---- | ---- | ------ |
| Shared UI | `packages/ui/src/components/common/SearchableSelect.tsx` | Create — typeahead combobox built from `DropdownMenu` + `SearchField` |
| Shared UI | `packages/ui/src/index.ts` (or equivalent barrel) | Modify — export `SearchableSelect` |
| Desktop UI | [`apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx`](../../apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx) | Modify — add Cursor/Generic tabs, comparison expander, `SearchableSelect` pickers, rotate-vs-new-client regenerate choice |
| Desktop UI | [`apps/desktop/src/features/clients/CursorBridgeSection.tsx`](../../apps/desktop/src/features/clients/CursorBridgeSection.tsx) | Delete — logic absorbed into the unified modal |
| Desktop UI | [`apps/desktop/src/features/clients/cursor-bridge-config.helpers.ts`](../../apps/desktop/src/features/clients/cursor-bridge-config.helpers.ts) | Keep as-is — `buildCursorBridgeMcpJson()`/`CURSOR_BRIDGE_CLIENT_NAME` now imported into the modal instead of the deleted card |
| Desktop UI | [`apps/desktop/src/features/clients/ClientsPage.tsx`](../../apps/desktop/src/features/clients/ClientsPage.tsx) | Modify — remove the `CursorBridgeSection` render and its import; header button still opens the (now unified) modal |
| i18n | [`apps/desktop/src/locales/en/clients.json`](../../apps/desktop/src/locales/en/clients.json) | Modify — merge `cursorBridge.*` keys into modal-scoped keys, add comparison-table copy, drop unused card-specific keys |
| Tests | `tests/e2e/specs/**` (any spec referencing `cursor-bridge-*` or `register-api-key-*` testids) | Modify — update selectors for the unified modal |

---

## Phases

### Phase 1 — `SearchableSelect` component (~half day)

- Build `SearchableSelect.tsx` in `packages/ui` using `DropdownMenu` (trigger/content/click-outside/escape) + `SearchField` (filter input)
- Support `onCreateNew` for the "+ New machine…" affordance the modal already relies on
- No consumers wired yet in this phase — component ships standalone with its own unit test

**Outcome:** `SearchableSelect` renders, filters options by typed text, and calls `onChange`/`onCreateNew` correctly in isolation. `pnpm typecheck && pnpm lint` pass.

---

### Phase 2 — Unified modal with Cursor/Generic tabs (~1 day)

- Add tab state to `RegisterApiKeyClientModal.tsx`, Cursor selected by default
- Cursor tab: name field defaulting to `CURSOR_BRIDGE_CLIENT_NAME`, Space lock via `SearchableSelect`, machine picker via `SearchableSelect` (new for this tab), submit calls `registerApiKeyClient()` then `buildCursorBridgeMcpJson()` for snippet output (moved import from the deleted `CursorBridgeSection`)
- Generic tab: existing fields, restyled with `SearchableSelect` in place of the raw `<select>`s, output unchanged (raw key)
- Add the "Which should I use?" comparison-table expander directly above the submit button row

**Outcome:** Opening "Register client" defaults to a Cursor-preset view producing the same `~/.cursor/mcp.json` snippet the old card produced; switching to Generic reproduces the old modal's behavior exactly. `pnpm typecheck && pnpm lint` pass.

---

### Phase 3 — Regenerate choice + `ClientsPage` cleanup (~half day)

- Wire the rotate-vs-new-client choice into the Cursor tab's regenerate action (`createClientApiKey` vs `registerApiKeyClient`)
- Remove `CursorBridgeSection` import/render from `ClientsPage.tsx`; delete `CursorBridgeSection.tsx`
- Update `clients.json`: merge `cursorBridge.*` into new modal keys, drop unused ones
- Sweep `tests/e2e/specs/**` for `cursor-bridge-*`/`register-api-key-*` testids and update selectors

**Outcome:** Connections page shows a single header "Register client" entry point; regenerating a Cursor config either rotates the existing client's key or creates a new one per user choice, with no silent duplicate accumulation. `pnpm test:ts && pnpm typecheck && pnpm lint` pass; any affected e2e specs updated.

---

## Key files referenced

| File | Notes |
| ---- | ----- |
| [`apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx`](../../apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx) | Generic modal being extended with tabs — current Space/Machine `<select>` fields are what `SearchableSelect` replaces |
| [`apps/desktop/src/features/clients/CursorBridgeSection.tsx`](../../apps/desktop/src/features/clients/CursorBridgeSection.tsx) | Card being deleted — its generate/regenerate logic moves into the modal's Cursor tab |
| [`apps/desktop/src/features/clients/cursor-bridge-config.helpers.ts`](../../apps/desktop/src/features/clients/cursor-bridge-config.helpers.ts) | `buildCursorBridgeMcpJson()`, `CURSOR_BRIDGE_CLIENT_NAME` — unchanged, re-imported into the modal |
| [`apps/desktop/src/features/clients/ClientsPage.tsx`](../../apps/desktop/src/features/clients/ClientsPage.tsx) | Header button + modal mount point; card removal happens here |
| [`apps/desktop/src/lib/api/gateway.ts`](../../apps/desktop/src/lib/api/gateway.ts) | `registerApiKeyClient()`, `createClientApiKey()` — existing wrappers, no changes needed |
| [`packages/ui/src/components/common/DropdownMenu.tsx`](../../packages/ui/src/components/common/DropdownMenu.tsx) | Primitive `SearchableSelect` is built on top of |
| [`packages/ui/src/components/common/SearchField.tsx`](../../packages/ui/src/components/common/SearchField.tsx) | Primitive providing the typeahead input |
| [`apps/desktop/src/components/SpaceSwitcher.tsx`](../../apps/desktop/src/components/SpaceSwitcher.tsx) | Reference pattern for the "nicer" custom dropdown styling the user pointed at |
| [`apps/desktop/src/features/workspaces/workspace-binding-form.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-form.component.tsx) | Has its own `Picker` (still a styled native `<select>`, not a true combobox) — left untouched per Scope/Out |

---

## Related documentation

- [`docs/planning/upstream-client-mapping-reconciliation.md`](./upstream-client-mapping-reconciliation.md) — origin of the generic `RegisterApiKeyClientModal` and `mcpk_` API-key auth this unification builds on
- [`docs/planning/cursor-workspace-routing-bridge.md`](./cursor-workspace-routing-bridge.md) — origin of the Cursor bridge concept and `buildCursorBridgeMcpJson()`, including the never-resolved "or fold into `ClientsPage.tsx`" note this doc finally addresses
- [`docs/manual/workspace-header-routing.md`](../manual/workspace-header-routing.md) — background on why Cursor needs a dedicated routing path at all
