# Projects — Grouped Multi-Machine Cards

**Last Updated:** Jun 25, 2026
**Status:** Planning — not started
**Branch:** `feat/projects-grouped-machine-cards` (off `feat/workspace-machine-binding`)
**Depends on:** `feat/workspace-machine-binding` fully merged (migrations 033/034, machine CRUD, `machine_id` on bindings)
**Unblocks:** Full homelab multi-box workflow — same project path on Gondor and Rohan each routing to a different FeatureSet, both visible at a glance

> ⚠️ **Status/Branch stale as of Jul 1, 2026** — this shipped in-branch on `feat/workspace-machine-binding` (Jun 25 commits), not on a separate `feat/projects-grouped-machine-cards` branch. See [`workspace-machine-binding.md`](./workspace-machine-binding.md#future-todos-from-pr-8-review-jul-1-2026) Future TODOs.

---

## Problem

The Projects page builds one `Entry` per unique path using a `seen` set. When two bindings share the same `workspace_root` but differ by `machine_id` (e.g. `/repos/s2h` on Gondor and Rohan), only the first one reaches the grid — the second is silently dropped:

```ts
// entries useMemo — current
for (const b of bindings) {
  const key = b.workspace_root.toLowerCase();
  if (seen.has(key)) continue;  // ← Rohan binding never renders
  seen.add(key);
  list.push({ id: b.id, binding: b, ... });
}
```

`bindingsByRoot` compounds this: it maps `root → single WorkspaceBinding`, so even the live-root path only attaches one binding per path.

Two consequences:

**1. Rohan's binding for `/repos/s2h` is invisible.** There is no card for it, no way to edit or delete it from the UI, and no indication it exists.

**2. Machine indicator on `EntryCard` is a single optional string.** The component has no concept of a path existing on multiple machines with different routing configs.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Entry granularity | One `Entry` per unique `workspace_root` (not per binding) | Matches the mental model — a project is a folder, not a binding. Multiple machines are routing variants of the same project. |
| 2 | Primary binding | First global (`machine_id IS NULL`) binding; fallback to first machine-scoped; fallback null | Gives the card a stable "canonical" binding for icon, space, FS display, and inspector default — without choosing a machine arbitrarily. |
| 3 | Card anatomy | Machine byline for single-machine cards; machine row list in the footer area for multi-machine cards | Option 2 + Option 4 from the visual options canvas. Byline is low-noise for the common case; rows surface all machines when more than one exists. |
| 4 | Inspector selection | `selectedBindingId` state — top-level card click opens primary binding; machine row click opens that row's binding | Preserves the current click-to-inspect feel while making individual machine bindings editable independently. |
| 5 | Machine filter on grouped cards | Filter matches if any binding on the entry has the selected `machine_id` | Consistent with how other multi-value fields filter — "show me Rohan projects" should include s2h even though s2h also has a Gondor binding. |

---

## Scope

**In:**
- `Entry.binding: WorkspaceBinding | null` → `Entry.bindings: WorkspaceBinding[]`
- `bindingsByRoot: Map<string, WorkspaceBinding>` → `Map<string, WorkspaceBinding[]>`
- `entries` useMemo rewrite — group by root instead of deduplicate by root
- `primaryBinding(entry)` helper — selects the canonical binding for space/FS/icon resolution
- `EntryCard` refactor — machine byline (single machine) and machine row list (multi-machine)
- `selected` state extended with `selectedBindingId: string | null` for row-level inspector routing
- Inspector receives the specific clicked binding (not always the primary)
- Machine filter updated for multi-binding entries

**Out:**

| Item | Reason |
| ---- | ------ |
| "New binding for existing path" machine-scoping prompt | Useful but a follow-up UX refinement — creating a second binding for a path already works today via the machine picker in the inspector; no prompt needed for MVP. |
| Machine color-coding (left border per machine) | Option 3 from the visual options canvas — deferred until machine icons are designed. Byline + row labels are sufficient disambiguation. |
| Machine filter: collapse grouped card to matching rows only | Complex interaction — clicking a filter would hide some rows inside a card. Defer; full card show/hide is correct for now. |
| Drag-to-reorder machine rows | No ordering requirement exists. Display order is creation order (stable from `list_workspace_bindings`). |

---

## Architecture

### Revised Entry model

```ts
interface Entry {
  id: string;           // primaryBinding.id ?? `live:${root}` — stable per path
  kind: EntryKind;
  root: string;
  bindings: WorkspaceBinding[];  // all bindings for this root, any machine_id
  isLive: boolean;
}

// Derived helper — not stored on Entry
function primaryBinding(entry: Entry): WorkspaceBinding | null {
  return (
    entry.bindings.find((b) => b.machine_id == null) ??
    entry.bindings[0] ??
    null
  );
}
```

### bindingsByRoot rebuild

```ts
const bindingsByRoot = useMemo(() => {
  const m = new Map<string, WorkspaceBinding[]>();
  for (const b of bindings) {
    const key = b.workspace_root.toLowerCase();
    const list = m.get(key) ?? [];
    list.push(b);
    m.set(key, list);
  }
  return m;
}, [bindings]);
```

### entries useMemo rewrite

```
for root in reportedRoots:
  key = root.toLowerCase()
  seen.add(key)
  binds = bindingsByRoot.get(key) ?? []
  push Entry{ bindings: binds, kind: binds.length > 0 ? 'mapped-live' : 'unmapped-live', isLive: true }

for b in bindings:
  key = b.workspace_root.toLowerCase()
  if seen.has(key): continue     // root already has an entry (live); this binding is included via bindingsByRoot
  seen.add(key)
  binds = bindingsByRoot.get(key) ?? [b]
  push Entry{ bindings: binds, kind: 'mapped-offline', isLive: false }
```

### Card anatomy

```
┌─────────────────────────────────────────────┐
│ [icon]  [OFFLINE]  [GONDOR]                 │  ← single-machine: machine in pill row
│         s2h-platform                        │
│         /repos/sync2hire/platform           │
├─────────────────────────────────────────────┤
│  Routes to  [Dev Tools]  in  [All]          │
└─────────────────────────────────────────────┘

┌─────────────────────────────────────────────┐
│ [icon]  [OFFLINE]                           │  ← multi-machine: no machine pill
│         s2h-platform                        │
│         /repos/sync2hire/platform           │
├──[Gondor]───────────────────────────────────┤
│  Routes to  [Dev Tools]   in  [All]         │  ← clickable row → opens Gondor binding
├──[Rohan]────────────────────────────────────┤
│  Routes to  [Read-only]   in  [All]         │  ← clickable row → opens Rohan binding
└─────────────────────────────────────────────┘
```

Machine rows in the multi-binding case are a borderless list inside the card footer area. Each row shows the machine name as a small label + its FS and Space chips. Clicking a row sets `selectedBindingId` and opens the inspector on that binding.

### Inspector routing

```ts
// Selected state (extended)
type Selected =
  | { mode: 'new' }
  | { mode: 'entry'; id: string; bindingId?: string };  // bindingId = specific row click

// Resolving the active binding in the inspector
const activeBinding =
  selected?.bindingId
    ? selectedEntry?.bindings.find((b) => b.id === selected.bindingId) ?? primaryBinding(selectedEntry)
    : primaryBinding(selectedEntry);
```

---

## Files to Modify

| File | Change |
| ---- | ------ |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `Entry` type, `bindingsByRoot`, `entries` useMemo, `primaryBinding` helper, `filtered` machine clause, `EntryCard` props + anatomy, `Selected` type extension, `selectedBindingId` state, inspector active-binding resolution |
| [`apps/desktop/src/locales/en/workspaces.json`](../../apps/desktop/src/locales/en/workspaces.json) | Add `card.machineRow` label for machine row monitor icon aria-label |

---

## Phases

### Phase 1 — Multi-binding Entry model (~half day)

- Change `bindingsByRoot` from `Map<string, WorkspaceBinding>` to `Map<string, WorkspaceBinding[]>`
- Change `Entry.binding: WorkspaceBinding | null` to `Entry.bindings: WorkspaceBinding[]`
- Add `primaryBinding(entry)` helper; replace all `entry.binding?.X` call sites with `primaryBinding(entry)?.X`
- Rewrite `entries` useMemo to group by root — a live root collects all bindings for that path; offline loop only pushes roots not already seen
- Update machine filter: `entry.bindings.some(b => b.machine_id === machineFilter)` instead of `entry.binding?.machine_id !== machineFilter`
- Update `machinesWithBindings` derivation: iterate `entries` and collect distinct machines from `entry.bindings`
- Keep `EntryCard` props and `machineName` prop unchanged in this phase — pass `primaryBinding(entry)?.machine_id ? machinesById.get(...)?.name : undefined` as before

**Outcome:** Rohan's binding for `/repos/s2h` is no longer silently dropped. The entries array contains one entry for that path with `bindings.length === 2`. The card grid looks identical to before. `pnpm typecheck` and `cargo test --workspace` pass.

---

### Phase 2 — Grouped card UI (~half day)

- Replace `machineName?: string` on `EntryCard` with `bindings: WorkspaceBinding[]` + `machinesById: Map<string, Machine>` props
- Single binding with `machine_id` set: render machine byline (monitor SVG icon + machine name) between the path line and the footer separator
- Multiple bindings (`bindings.length > 1`): render a machine row list in the footer — each row shows a small machine label, FS chip, and Space chip; rows are `<button>` elements with an `onClick` for Phase 3 wiring (no-op for now)
- Global canonical single binding (`machine_id == null`): no machine indicator
- Emit `onMachineRowClick(bindingId: string)` prop on `EntryCard` (no-op until Phase 3)
- Add `card.machineRow` aria-label i18n key

**Outcome:** s2h with Gondor + Rohan bindings renders as one card with two machine rows in the footer, each showing its own FS. A singly-scoped binding shows a machine byline. A global canonical binding shows no machine indicator. `pnpm typecheck && pnpm lint` pass clean.

---

### Phase 3 — Inspector routing for grouped cards (~quarter day)

- Extend `Selected` type to `{ mode: 'entry'; id: string; bindingId?: string }`
- Add `selectedBindingId` derived from `selected.bindingId` — used to resolve the active binding inside the inspector
- Wire `onMachineRowClick` on `EntryCard` to `setSelected({ mode: 'entry', id: entry.id, bindingId })` instead of the top-level card `onClick`
- Pass `activeBinding` (resolved from `selectedBindingId ?? primaryBinding`) to `InspectorPanel` instead of `entry.binding`
- Top-level card click (not on a machine row) clears `bindingId` and selects the primary binding as before

**Outcome:** Clicking the Gondor row on a grouped s2h card opens the Gondor binding in the inspector; editing and saving updates only that binding. Clicking the Rohan row opens the Rohan binding independently. Top-level card click still works for primary-binding editing. `pnpm typecheck && pnpm lint` pass.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `Entry`, `bindingsByRoot`, `entries` useMemo, `EntryCard`, `InspectorPanel` — all changes live here |
| [`apps/desktop/src/lib/api/workspaceBindings.ts`](../../apps/desktop/src/lib/api/workspaceBindings.ts) | `WorkspaceBinding` interface with `machine_id: string | null` — read-only reference for types |
| [`apps/desktop/src/lib/api/machines.ts`](../../apps/desktop/src/lib/api/machines.ts) | `Machine` interface, `listMachines`, `getLocalMachineId` — already loaded in `WorkspacesPage` |
| [`apps/desktop/src/locales/en/workspaces.json`](../../apps/desktop/src/locales/en/workspaces.json) | Existing `card.*` and `machineIdentity.*` i18n keys |

---

## Related Documentation

- [`workspace-machine-binding.md`](./workspace-machine-binding.md) — the feature this builds on; migrations 033/034, machine CRUD, `machine_id` on bindings, resolver wiring
- Visual options canvas: `canvases/machine-association-options.canvas.tsx` — Options 2 and 4 are the basis for the card anatomy in Phase 2
