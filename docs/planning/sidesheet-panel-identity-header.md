# Workspace Binding Panel — Identity Header Refactor

**Last Updated:** Jun 28, 2026
**Status:** Complete (including restoration pass + auto-close fix)
**Branch:** `feat/workspace-machine-binding`
**Depends on:** None — builds on current panel in `main`
**Unblocks:** Cleaner routing UX for multi-machine users; removes identity clutter from the Mapping form

---

## Problem

The `WorkspaceBindingPanel` puts label, icon, machine, workspace root, space, and feature set inside a single "Mapping" collapsible with no hierarchy. Three different concerns are mixed together:

- **Identity** — label and icon are cosmetic metadata; they have nothing to do with routing
- **Routing** — space and feature set are the critical config; they're what the binding actually does
- **Scope** — machine and workspace root determine when/where a binding applies

This means the space and feature set pickers — the two fields a user actually needs to change — sit below label and icon every time. There's also no visible indication in the panel of which machine binding you're currently editing until you scroll into the form.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Where label + icon live | Move into the panel header as inline-editable fields | Frees the form body to be routing-only. The header is the natural identity surface — you see it before any section. |
| 2 | Machine display in header | Prominent machine pill/badge in the header trailing area | The user needs to know immediately which machine's binding they're editing. A badge in the header achieves this without dedicating a form field row to it. |
| 3 | Machine edit in header | Header badge is display-only; changing machine stays in the Scope section | Keeps the header simple. Badge click scrolls/expands Scope, not a separate popover. |
| 4 | Form sections | "Mapping" becomes two sections: "Routing" (space + FS) and "Scope" (machine + root) | Routing is the high-signal decision; Scope is usually set once and rarely touched. Scope starts collapsed. |
| 5 | State location | Lift form state (label, icon, spaceId, fsIds, machineId, root) to `WorkspaceBindingPanel` | Splitting fields across two `CollapsibleSection` wrappers requires coordinated state. Lifting makes both sections contribute to the same autosave trigger and removes the need for two `BindingForm` instances. |
| 6 | BindingForm refactoring | `BindingForm` becomes two narrower components: `RoutingFields` and `ScopeFields` | Cleaner than a `visibleFields` prop, easier to test independently, and the components are small enough that this isn't over-engineering. |
| 7 | Icon edit in header | Emoji picker button + clear in the header icon slot; file upload stays in Scope | Upload button in a 44px header slot is too cramped. Emoji and clear cover the common case; power users can expand Scope to upload. |

---

## Scope

**In:**
- Panel header: inline label input (editable), editable icon slot (emoji picker + clear), machine pill showing current binding's machine name
- `WorkspaceBindingPanel`: lift label, icon, spaceId, fsIds, machineId, root state from `BindingForm`
- New `RoutingFields` component: space picker + feature set picker (multiselect)
- New `ScopeFields` component: machine picker + workspace root input + browse + icon upload/text
- Replace "Mapping" `CollapsibleSection` with "Routing" (defaultOpen, accent) + "Scope" (collapsed by default)
- Machine pill in header: shows machine name (or "Global" for null); clicking it expands Scope
- Sticky footer submit/cancel for create + create-from-live modes
- `Effective Features` section: unchanged
- Update `panel.*` i18n keys; add `panel.machineGlobal`, `panel.routing`, `panel.scope`, `panel.identityPlaceholder`

**Out:**

| Item | Reason |
| ---- | ------ |
| File upload icon in header | Too cramped in a 44px header row. Upload button remains in `ScopeFields`. |
| Machine badge as a dropdown (quick-switch) | Adds complexity without a clear user request. Full machine change goes through the Scope section. |
| Splitting `BindingForm` into separate files | `RoutingFields` and `ScopeFields` live in `workspace-binding-form.component.tsx` alongside `SaveStatusPill` and `FormField`. No new files unless the file exceeds 400 lines. |
| Autosave changes | Edit-mode autosave logic stays exactly as-is; it just triggers from lifted state instead of internal `BindingForm` state. No behavior change. |
| Create-from-live sheet (quick sheet) | `WorkspaceBindingSheet.tsx` was removed; live prompts use `WorkspaceBindingPanel` exclusively. |

---

## Architecture

### Lifted state in `WorkspaceBindingPanel`

```ts
const [label, setLabel] = useState(initial?.label ?? '');
const [icon, setIcon] = useState(initial?.icon ?? appearanceIcon ?? '');
const [spaceId, setSpaceId] = useState(initial?.space_id ?? defaultSpaceId);
const [fsIds, setFsIds] = useState(initial?.feature_set_ids ?? []);
const [machineId, setMachineId] = useState(initial?.machine_id ?? '');
const [machineIds, setMachineIds] = useState<string[]>([]); // create-mode multiselect
const [root, setRoot] = useState(initial?.workspace_root ?? prefillRoot ?? '');
```

Autosave effect and submit handler stay in `WorkspaceBindingPanel`; the form components are passed values + setters.

### Component split

```
WorkspaceBindingPanel
├── PanelIdentityHeader          ← inline label, emoji icon, machine badge
├── CollapsibleSection "Routing" (defaultOpen)
│   └── RoutingFields            ← space picker (space_locked disables) + FS multiselect
├── CollapsibleSection "Scope" (collapsed, ref.expand())
│   └── ScopeFields              ← icon upload/text, machine picker, workspace root
├── CollapsibleSection "Effective Features" (unchanged)
├── Sticky footer (create modes) ← submit + cancel
└── Delete footer (edit mode)
```

### `bindingPanelStore` payload extensions

- `spaceLocked?: boolean` — from `workspace-needs-binding` event; locks Routing space picker
- `appearanceIcon?: string | null` — seeds icon from card grid when opening create-from-live manually

---

## Files Modified

| File | Change |
| ---- | ------ |
| [`workspace-binding-panel.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx) | Lifted state, header, Routing/Scope sections, sticky create footer, duplicate root check, machine badge resolver, routing subtitle for create-from-live; removed erroneous `workspace-binding-changed` auto-close listener |
| [`workspace-binding-form.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-form.component.tsx) | `RoutingFields`, `ScopeFields`, icon upload in Scope, `bindingScopeConflicts`, duplicate error testid |
| [`bindingPanelStore.ts`](../../apps/desktop/src/stores/bindingPanelStore.ts) | `spaceLocked`, `appearanceIcon` on payload |
| [`WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | Pass `appearanceIcon` when opening create-from-live from cards |
| [`workspaces.json`](../../apps/desktop/src/locales/en/workspaces.json) | `panel.*`, `form.duplicateRoot`, `form.spaceLockedHint`, `panel.machineCount` |
| [`WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `CollapsibleSectionRef.expand()` via forwardRef |
| Test files | Retarget `WorkspaceBindingPrompt` to panel; App mock; WorkspacesPage test mocks |

---

## Phases (completed)

| Phase | Status | Commit area |
| ----- | ------ | ----------- |
| 1 — Lift form state to panel | Done | `da725d6` |
| 2 — Panel header identity + machine badge | Done | `a06c7ab` |
| 3 — Routing + Scope sections, badge wiring | Done | `ef6763f` |
| 4 — Restoration pass | Done | `c68bc55` |
| 5 — Auto-close on edit save fix | Done | uncommitted |

---

## Restoration pass (Jun 28, 2026)

Regressions found after Phase 3 and fixed:

| Issue | Fix |
| ----- | --- |
| Icon upload/text removed | Restored full icon UI in `ScopeFields` |
| Create submit hidden in collapsed Scope | Sticky footer submit/cancel for create modes |
| Machine badge stuck on "Global" in create | Lifted `machineIds` to panel; `resolveMachineBadgeLabel` |
| `space_locked` not wired | Added to store + event handler; disables Routing space picker |
| create-from-live prompt copy lost | Moved `sheet.descNew` / `descCollision` to Routing subtitle |
| Duplicate root error missing in UI | Client-side pre-check + `workspace-binding-duplicate-error` testid |
| Unmapped card icon not seeded | `appearanceIcon` on store payload from `WorkspacesPage` |
| Stale tests | Retargeted `WorkspaceBindingPrompt` to panel; fixed App/Workspaces mocks |

---

## Post-ship fixes (Jun 28, 2026)

### Edit panel closed on Clear / autosave

**Symptom:** Clicking icon **Clear** or changing machine scope to **No machine** while editing a binding dismissed the entire panel. The save often succeeded, but the UX looked broken.

**Cause:** A `workspace-binding-changed` listener in `WorkspaceBindingPanel` called `close()` whenever the event's `workspace_root` matched the open binding. Every in-panel edit save (icon persist, machine change, 1.5s autosave) emits that event — contradicting the autosave invariant ("no behavior change").

**Fix:** Removed the auto-close listener. Panel now closes only via explicit paths: backdrop / X / Escape, create submit success, delete, disable-prompt link, create footer Cancel.

**Tests:** `WorkspaceBindingPrompt.test.tsx` — edit mode stays open after `workspace-binding-changed` and after icon Clear persist.

**Tradeoff:** If a binding is deleted or mutated externally while the edit panel is open, stale data may show until the user closes manually. Acceptable; matches pre-refactor edit behavior.

---

## Related Documentation

- [`projects-grouped-machine-cards.md`](./projects-grouped-machine-cards.md)
- [`workspace-machine-binding.md`](./workspace-machine-binding.md)
- Visual options canvas: `canvases/sidesheet-layout-opts.canvas.tsx` — Option C is the basis for this plan
