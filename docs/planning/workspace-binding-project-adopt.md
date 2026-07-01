# Workspace Binding — Cross-Machine Project Adoption

**Last Updated:** Jun 26, 2026
**Status:** Planning — not started
**Branch:** `feat/workspace-machine-binding` (continuing off `caf9c7a`)
**Depends on:** `feat/workspace-machine-binding` fully merged (migrations 033–035, machine CRUD, `machine_id` on bindings, resolver 3-tier lookup)
**Unblocks:** Full multi-box workflow — opening a project on a new machine auto-offers the existing routing config instead of starting from scratch

---

## Problem

When a user opens a workspace path on a new machine that already has that project bound on another machine, the gateway finds no binding for the current machine context and emits `WorkspaceNeedsBinding`. The binding sheet fires and presents a blank Space + FeatureSet picker — as if this folder has never been configured before.

Two things are wrong:

**1. The sheet has no awareness of sibling bindings.** A binding for `/home/joe/repos/s2h` on Rohan is invisible to the sheet when Gondor opens `/Users/joe/Desktop/Repos/s2h`. The user must manually recreate the same Space + FS config with no hint that it already exists.

**2. The WorkspacesPage LIVE badge misreports state.** A path is shown as emerald "LIVE" if `bindings.length > 0` — even when every binding is scoped to a different machine and the current install will route to `SpaceDefault` for every session on that path.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Match existing bindings by | **Folder name** (last path segment) rather than full path | Absolute paths differ across machines (`/Users/joe/…` vs `/home/joe/…`). Folder name is stable: `s2h-platform` is the same project everywhere. |
| 2 | Data model | No schema change — keep flat `workspace_bindings` rows | `listWorkspaceBindings()` already returns everything; client-side filter by folder name is sufficient. A `Project` entity is a follow-on if cross-machine sync is ever needed. |
| 3 | Adopt UX in sheet | New `'adopt'` step between `'machine'` and `'binding'` — shows a table of folder-name-matched bindings | Explicit opt-in. User can see exactly what config they're copying before it's applied. "Start fresh" always available. |
| 4 | Adopt action | Clone as a new machine-scoped row for the current machine with the same `space_id` + `feature_set_ids` | Preserves per-machine override ability. The two bindings are independent after creation. No mutation of the source binding. |
| 5 | Table component | Inline component in the sheet — do NOT extract `EntryCardRoutingTable` from `WorkspacesPage.tsx` | Extraction requires significant refactoring of a 3k-line file for a visual component the sheet only needs in a simplified form. Mirror the style; don't share the code. |
| 6 | LIVE badge fix | Pass `localMachineId` to `EntryCard`; badge is amber "UNBOUND" (not green "LIVE") when the path is live but no binding matches the current machine | The current emerald badge is factually wrong for the current machine's routing state. Amber clearly signals "something is configured but not for you." |

---

## Scope

**In:**
- `WorkspaceBindingSheet`: folder-name match on `listWorkspaceBindings()` at sheet open; `'adopt'` step with a table of matching bindings; pre-fill space+FS on adopt; skip step when no matches
- `WorkspacesPage`: `localMachineId` passed into `EntryCard`; badge variant when live but not bound for current machine; new i18n key

**Out:**

| Item | Reason |
| ---- | ------ |
| Project entity / schema change | Deferred — current flat binding rows support the UX fix without any migration; revisit for cloud sync |
| "Promote to global" option on adopt | Adds product complexity with no user request. A second binding row per machine is the correct default given the per-machine override intent in `workspace-machine-binding.md`. |
| Extracting `EntryCardRoutingTable` as a shared component | Deferred — useful if a third consumer appears, but not worth the refactor for one callsite today |
| Machine filter visual collapse (hide non-matching rows within card) | Deferred per `projects-grouped-machine-cards.md` — full card show/hide is the current model |
| Full path match fallback when no folder-name matches found | YAGNI — folder-name is always available; if the folder name is non-unique the user just sees more options in the adopt table |

---

## Architecture

### Folder-name matching

```ts
function folderName(root: string): string {
  return root.replace(/\\/g, '/').replace(/\/$/, '').split('/').at(-1) ?? root;
}

// On sheet open, after listWorkspaceBindings():
const currentFolder = folderName(payload.workspace_root);
const siblingBindings = allBindings.filter(
  (b) => folderName(b.workspace_root) !== currentFolder &&
         folderName(b.workspace_root) === currentFolder  // same folder name, different full path
         // actually:
);
// Correct version:
const siblingBindings = allBindings.filter(
  (b) =>
    b.workspace_root.toLowerCase() !== payload.workspace_root.toLowerCase() &&
    folderName(b.workspace_root).toLowerCase() === currentFolder.toLowerCase(),
);
```

If `siblingBindings.length > 0` → inject `'adopt'` step before `'binding'`. Otherwise proceed straight to `'binding'` as today.

### Sheet step model (updated)

```
'machine'  →  'adopt'  →  'binding'
              (skipped if
              no siblings)
```

- `'machine'` step: unchanged — assign machine to client if not already set
- `'adopt'` step (new): table of sibling bindings; "Use this" pre-fills space + FS and advances to `'binding'`; "Start fresh" advances to `'binding'` with no pre-fill
- `'binding'` step: unchanged — space + FS pickers, save

### Adopt table row shape

```ts
interface AdoptRow {
  bindingId: string;
  machineName: string | null;   // null = "No machine" (global)
  workspaceRoot: string;        // shown as the "source path" for disambiguation
  spaceName: string;
  fsNames: string[];
}
```

Columns: MACHINE | PATH | SPACE | TOOL SET

### LIVE badge fix

```ts
// In WorkspacesPage, pass down to EntryCard:
const entryIsBoundForCurrentMachine = (entry: Entry) =>
  entry.bindings.some(
    (b) => b.machine_id == null || b.machine_id === localMachineId,
  );

// EntryCard kind derivation (live roots only):
const kind =
  isLive && bindings.length > 0 && entryIsBoundForCurrentMachine(entry)
    ? 'mapped-live'
    : isLive && bindings.length > 0
    ? 'live-unbound'       // ← new
    : isLive
    ? 'unmapped-live'
    : 'mapped-offline';
```

Badge rendering:

| `kind` | Badge | Color |
| ------ | ----- | ----- |
| `mapped-live` | LIVE | emerald |
| `live-unbound` | UNBOUND | amber |
| `unmapped-live` | UNMAPPED | blue/muted |
| `mapped-offline` | (none / offline indicator) | — |

---

## Files to Modify

| File | Change |
| ---- | ------ |
| [`apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx`](../../apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx) | Add `'adopt'` to step union; `siblingBindings` state; `folderName` helper; adopt table component; pre-fill logic on "Use this"; `listWorkspaceBindings` call at sheet open |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | Pass `localMachineId` to `EntryCard`; add `'live-unbound'` to `EntryKind`; update badge rendering for new state |
| [`apps/desktop/src/locales/en/workspaces.json`](../../apps/desktop/src/locales/en/workspaces.json) | `sheet.adopt*` keys (step title, desc, table headers, CTA labels); `card.badgeLiveUnbound` key |

---

## Phases

### Phase 1 — Sheet: sibling detection + adopt step (~half day)

- Add `folderName(root: string): string` helper (last path segment, cross-platform slash normalize)
- Add `siblingBindings` state + fetch via `listWorkspaceBindings()` on `workspace-needs-binding` event (parallel with existing `getClientMachineId` call)
- Extend step union: `'machine' | 'adopt' | 'binding'`; default to `'binding'`; route to `'adopt'` when `siblingBindings.length > 0` and the client already has a machine (or after machine step completes)
- `AdoptStep` sub-component: renders a table of sibling bindings (MACHINE | PATH | SPACE | TOOL SET); "Use this" button per row; "Start fresh" link below
- "Use this" → set `selectedSpaceId` + `selectedFsId` from the chosen binding, advance to `'binding'`
- "Start fresh" → advance to `'binding'` with no pre-fill (existing behavior)
- Resolve `spaceName` and `fsNames` from the same `listSpaces` + `listFeatureSetsBySpace` data already loaded; no extra API calls needed
- Add `sheet.adopt*` i18n strings

**Outcome:** Opening a workspace path on a machine where that folder name is already bound on a different machine shows the adopt step with the existing config(s) as selectable rows. Clicking "Use this" pre-fills the space + FS pickers. Clicking "Start fresh" or opening a path with no siblings goes straight to the binding step as before. `pnpm typecheck && pnpm lint` pass clean.

---

### Phase 2 — WorkspacesPage: LIVE badge fix (~quarter day)

- Load `localMachineId` from `getLocalMachineId()` in `WorkspacesPage` `loadData` (already fetched; confirm it's in scope or add if not)
- Add `'live-unbound'` to `EntryKind` union
- Update `entries` useMemo: derive `kind` for live roots using `entryIsBoundForCurrentMachine(entry, localMachineId)` helper — `mapped-live` only when at least one binding has `machine_id == null || machine_id === localMachineId`
- Update `EntryCard` badge rendering: amber "UNBOUND" pill for `live-unbound`; existing emerald "LIVE" for `mapped-live`; existing muted "UNMAPPED" for `unmapped-live`
- Add `card.badgeLiveUnbound` i18n key (`"UNBOUND"`)

**Outcome:** A path that is live (currently reported) but has only bindings scoped to other machines shows an amber "UNBOUND" badge instead of emerald "LIVE". A path that is live and has a global or current-machine-scoped binding continues to show green "LIVE". `pnpm typecheck && pnpm lint` pass clean.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx`](../../apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx) | Target file — existing step model (`'machine' \| 'binding'`), event handler, save flow |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `EntryCard`, `EntryCardRoutingTable`, `EntryKind`, `entries` useMemo, badge rendering |
| [`apps/desktop/src/lib/api/workspaceBindings.ts`](../../apps/desktop/src/lib/api/workspaceBindings.ts) | `WorkspaceBinding` type, `listWorkspaceBindings()`, `WorkspaceBindingInput` |
| [`apps/desktop/src/lib/api/machines.ts`](../../apps/desktop/src/lib/api/machines.ts) | `getLocalMachineId()`, `Machine` type |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | 3-tier lookup context — confirms why sheet fires on cross-machine paths |
| [`docs/planning/workspace-machine-binding.md`](./workspace-machine-binding.md) | Data model decisions (migrations 033–035, resolver tiers) this feature builds on |
| [`docs/planning/projects-grouped-machine-cards.md`](./projects-grouped-machine-cards.md) | `EntryCard` anatomy, `EntryKind`, `Entry.bindings[]` shape — badge fix builds on this |

---

## Related Documentation

- [`workspace-machine-binding.md`](./workspace-machine-binding.md) — machine CRUD, `machine_id` on bindings, 3-tier resolver this feature depends on
- [`projects-grouped-machine-cards.md`](./projects-grouped-machine-cards.md) — grouped card UI, `EntryKind`, `primaryBinding` — badge fix extends this work
