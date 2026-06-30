# Deny by Default — Bindable Callers & Canonical Bindings

**Last Updated:** Jun 29, 2026 (rev 3 — implementation details + Q&A decisions added after dig)
**Status:** Planning — ready to implement
**Branch:** `feat/deny-by-default-routing` (off the current workspace-binding lineage)
**Depends on:** `projects-grouped-machine-cards.md` + `workspace-binding-project-adopt.md` merged (one-card-per-path `Entry.bindings[]`, `EntryKind`, `machine_id` on bindings, 3-tier resolver)
**Unblocks:** A folder/client gets **zero** backend tools until it has an explicit representation on the Projects/Clients page — and the whole feature stops depending on the now-deprecated MCP `roots` primitive

---

## Problem

Three coupled problems, all introduced or sharpened by PR #175 (`7fc50a0`).

**1. The default is "everything," not "nothing."** PR #175 flipped three resolver tiers (1b unmapped-with-roots, 1c post-grace, 3 no-roots/no-grants) from `ResolutionSource::Deny` to `SpaceDefault` → the default Space's **Starter FeatureSet**. The Starter is the "edit/empty it to control the default" knob, but a populated Starter (the real-world case — ~2,832 members locally) means **every unmapped folder silently gets every tool**. The code comments say "empty the Starter to grant nothing," but the *source* stays `SpaceDefault`, so the system can't tell "intentional deny" from "Starter happens to be empty."

**2. Callers can invoke without representation.** A folder (or a rootless client) that has no card on the Projects/Clients page can still call backend tools, because the resolver hands it the Starter. There's no gate tying "can invoke" to "has an explicit binding."

**3. The whole feature rides a deprecated primitive.** MCP `roots` was formally deprecated in protocol `2026-07-28` via [SEP-2577](https://modelcontextprotocol.io/seps/2577-deprecate-roots-sampling-and-logging) (low adoption, informational-only semantics, better alternatives). It stays functional ~12 months. Routing keyed primarily on roots is building on sand — and clients vary wildly in roots support (Claude Desktop/Code yes; Cursor/VS Code inconsistent; browser clients structurally can't).

**4. Root reporting in practice — resolved no-action.** Live log analysis (`mcpmux.2026-06-29.log`, client `mcp_36740f70` = Cursor) shows the probe works correctly (180 non-empty root reports, 14 empty, 93% success today). The three "exhausted retries" sessions at 17:17 UTC were caused by Cursor silently hanging on `list_roots()` for >5 minutes until the RMCP keepalive (300s) closed the transport — our retry loop handled them correctly and the sessions were already dead. The `roots=[]` sessions from 17:34 onward are legitimate: the user had moved to the mcp-mux window and Cursor correctly reports no roots for that context. There is no probe bug to fix. The `roots=[]` → `SpaceDefault` → Starter leak is sealed by Phase 1 (`Unbound` replaces `SpaceDefault`), not by fixing the probe.

**3. The whole feature rides a deprecated primitive.** MCP `roots` was formally deprecated in protocol `2026-07-28` via [SEP-2577](https://modelcontextprotocol.io/seps/2577-deprecate-roots-sampling-and-logging) (low adoption, informational-only semantics, better alternatives). It stays functional ~12 months. Routing keyed primarily on roots is building on sand — and clients vary wildly in roots support (Claude Desktop/Code yes; Cursor/VS Code inconsistent; browser clients structurally can't).

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Deny mechanism | New explicit **`ResolutionSource::Unbound`** (empty `feature_set_ids`), distinct from defensive `Deny` | Makes deny-by-default a first-class, inspectable concept. Logs/events/UI can tell "no binding yet, expected — here's the CTA" from "system misconfig" (`Deny`). |
| 2 | Source of truth | The **`WorkspaceBinding` is canonical**; reported root, client identity, and machine are *ranked match signals*, none mandatory. Absence of all → `Unbound`. | Decouples the feature from `roots` without ripping out roots support. The deprecation just removes one signal from the priority list someday — not a rewrite. |
| 3 | Invocation gate | A caller with no matching binding (`Unbound`) gets **zero backend tools**, including via `mcpmux_invoke_tool`. Meta management/discovery/bind tools always work. | "No calls without a representation on the page." The `mcpmux_*` tools that let the LLM/user *create* a representation (search, get_schema, list_servers, set_workspace_root, bind) stay callable so you can always dig out. |
| 4 | Rootless callers | A rootless client with no grant resolves to `Unbound` and surfaces as a **bindable card keyed on client identity** (Option C). Every caller — folder or client — has a card and must be bound. | Anchors trust on mcpmux's existing per-client access keys (reliable even when roots aren't). Unifies the two surfaces under one rule. Matches the industry virtual-key pattern. |
| Bound-elsewhere | A path bound only on another machine resolves to `Unbound` on this machine (`deny_here`). | Consistent with deny-by-default. The user adds a machine-scoped (or adopts via `workspace-binding-project-adopt`) binding for this machine to enable tools. Tunneled multi-device: set `X-Mcpmux-Machine-Id` on each remote client — see [`per-device-machine-header.md`](./per-device-machine-header.md). |
| 6 | Unmapped card visibility | A detected-but-unbound folder **still appears as a card**, in an explicit "denied / bind to enable tools" state. The card *is* how you create the representation. | The CTA and the representation are the same surface — hiding it would make the deny a dead-end. |
| 7 | Card aggregation | **One card per project path**, per-machine breakdown inside it. Badges shift relative to the viewer; the card's *content* does not change with the machine dropdown. | Matches "used from Gondor but not Rohan." The bottom routing table (`EntryCardRoutingTable`) is the bones for the per-machine *binding* axis. Per-machine *usage* attribution is a data gap (see Architecture). |
| 8 | Starter fate | Keep Starter as an **opt-in default bundle** the user can explicitly bind, surfaced during onboarding — never the silent fallback. | The user should see, at setup, that default = nothing, and be offered the Starter bundle as a deliberate choice. |
| 9 | `mcpmux_bind_current_workspace` discoverability | Add to **`CORE_META_TOOLS`** (always visible in `tools/list`). Currently callable only via invoke-denial hints — insufficient for `Unbound` sessions. | LLM must be able to discover and call bind without first calling a backend tool and receiving a denial. |
| 10 | `unmapped-live` card copy | **Explicit denied state**: replace amber "Unmapped" pill with `UNBOUND` badge + primary "Bind to enable tools" CTA button on card body. `card.badgeLiveUnbound` key already exists (unused). | Card carries real meaning post-Phase-1: zero tools, calls blocked. "Unmapped" was informational; the card now needs to communicate a consequence. |
| 11 | Effective features in panel when unbound | **Empty state**: "No tools on this session yet. Bind this folder to enable access." Replace Starter FS preview entirely. | After Phase 1 the backend returns empty ids. Showing the Starter as a "preview" would be misleading — it's no longer the fallback. |
| 12 | Phase 3 scope | **Deferred** to its own dig + plan. Core mechanism (`ClientGrant` as rootless binding anchor) already works in Rust; needs fresh frontend surface on Connections page. | No infrastructure for bindable identity cards exists today — building it alongside Phases 1+2 would bloat the batch. |

---

## Scope

**In:**
- `ResolutionSource::Unbound` variant; `Deny` reserved for true degenerate cases (no default space / no Starter)
- Resolver: Tier 1b, Tier 1c-post-grace, and Tier 3 return `Unbound` (empty ids) instead of the Starter fallback
- Resolver doc rewrite: binding-canonical + ranked-signal framing (root → client grant → machine, none mandatory)
- `WorkspaceNeedsBinding` continues to fire on `Unbound` (keeps the auto-pop CTA)
- Confirm `mcpmux_invoke_tool` stays grant-gated under `Unbound` (blocks backend invoke); ensure the self-bind meta path (`mcpmux_bind_current_workspace`) is reachable/discoverable from an unbound caller
- Rootless client with no grant → `Unbound`; surface that client identity as a bindable card with a deny CTA
- Projects card: `unmapped-live` rendered as explicit deny + bind CTA; one-card-per-path with per-machine binding rows; local-machine usage attribution for the "used here" axis
- Onboarding surface: communicate default = none and offer the Starter as an opt-in bundle to bind
- Docs reconcile: `docs/guide/spaces.mdx` "active-Space FeatureSet for unmapped sessions" language is stale

- Investigate and fix the `peer.list_roots()` probe flakiness (transport-closed after 6 retries, falling back to `roots=[]`) — this is the direct cause of the "no card, calls go through" symptom today

**Out:**

| Item | Reason |
| ---- | ------ |
| Cross-install machine usage sync ("Rohan reported this folder" visible on Gondor) | No cloud sync exists — a single install only observes its own machine. Local-machine usage attribution ships; the cross-machine view is populated only as shared storage gains sync. Deferred. |
| Full merge of Projects + Clients pages into one surface | Option C's end state. MVP gives both surfaces the same deny-default + bind CTA; the literal page merge is a follow-up. |
| Removing `roots` support / migrating to a non-roots signal | Roots stays a (now non-mandatory) match signal during the ~12mo deprecation window. Decision 2 makes its eventual removal a config change, not a rewrite. |
| `collision_client_id` resurrection | Field is wired through events/UI but the resolver never populates it. Triaged in Phase 5 — remove the dead UX unless collision detection is explicitly wanted. |
| Per-client rate limiting / tool denylists | Industry gateways layer this on identity; out of scope for the deny-by-default cut. |

---

## Architecture

### `ResolutionSource` — add `Unbound`

```rust
pub enum ResolutionSource {
    WorkspaceBinding, // a binding matched a signal
    PendingRoots,     // roots-capable, roots in flight (transient empty, unchanged)
    ClientGrant,      // rootless-by-design with a per-client grant
    Unbound,          // NEW — no binding matched; deny by default (empty feature_set_ids)
    Deny,             // defensive only: no default space, or space has no Starter
}
```

`Unbound` carries the resolved `space_id` (for base-dir context + the CTA) but **empty** `feature_set_ids`. `fingerprint()` already returns `None` for empty ids, so per-peer `list_changed` change-detection keeps working.

### Resolver — binding canonical, signals ranked (Decision 2)

The resolver answers one question: *which binding (if any) matches the signals this caller presents?* Signals, in priority order:

```text
1. reported root      → exact WorkspaceBinding match (machine: client → local → global)
2. client identity    → per-client grant in the default space  (ClientGrant)
3. machine            → scopes (1) and (2); bound-elsewhere → no match on this machine
   else               → Unbound  (deny by default)
```

`roots` is signal #1, not a precondition. When SEP-2577 retires it, signal #1 drops and the rest stand. The old `default_fallback()` (Starter) is replaced by an `unbound()` helper:

```rust
// was: default_fallback(space_id) -> SpaceDefault + Starter id
fn unbound(&self, space_id: Option<Uuid>) -> ResolvedFeatureSet {
    ResolvedFeatureSet {
        feature_set_ids: vec![],
        space_id,
        source: ResolutionSource::Unbound,
        collision_client_id: None,
    }
}
```

Tier 1b / 1c-post-grace / 3 call `unbound(..)` instead of selecting the Starter. `PendingRoots` (grace window) is unchanged — it's already a transient empty.

### Invocation gate (Decision 3)

No new gate is needed in the handler — it falls out of `Unbound`:

- `list_tools` / `list_prompts` / `list_resources`: empty `feature_set_ids` → empty backend lists. `CORE_META_TOOLS` are still appended unconditionally.
- `call_tool` (non-meta): empty grants → already returns "not invokable" / redirect to `mcpmux_invoke_tool`.
- `mcpmux_invoke_tool`: already checks `get_invokable_tools_for_grants` against the resolved FS — under `Unbound` it has nothing to invoke. **This is the gate.** Confirm + test it.
- **Self-bind path:** ensure `mcpmux_bind_current_workspace` is discoverable from an `Unbound` caller (it's registered but not in `CORE_META_TOOLS`) so the LLM can create the representation that unblocks itself.

### Card model (Decisions 6, 7)

One `Entry` per path with `bindings[]` already exists (`projects-grouped-machine-cards.md`). Two axes:

- **Binding axis (have the bones):** `EntryCardRoutingTable` renders per-machine rows (machine → FS → space) from `entry.bindings`. Ghost rows already cover an unconfigured current machine.
- **Usage axis (data gap):** "used from Gondor not Rohan" needs machine-attributed *presence*. `reportedRoots` comes from `list_all_roots()` with **no machine attribution**. Add a per-`(path, machine)` last-seen record stamped with this install's `local_machine_id`. One install only ever sees its own machine's usage (cross-install sync is Out).

`unmapped-live` becomes the explicit deny state: amber, "No tools until you bind this folder," primary **Bind** CTA.

### Resolution → card-state map

| Resolver source | Projects/Clients card state |
| --------------- | --------------------------- |
| `WorkspaceBinding` (this machine / global) | LIVE (emerald) — routes to bound FS |
| `WorkspaceBinding` (other machine only) | BOUND ELSEWHERE (violet) — `Unbound` here, ghost row CTA |
| `ClientGrant` | bound client identity card |
| `Unbound` (folder) | UNMAPPED / DENIED (amber) — bind CTA, zero backend tools |
| `Unbound` (rootless client) | client card — "needs binding," zero backend tools |
| `PendingRoots` | transient — no card state change |
| `Deny` | degenerate/misconfig — surfaced as an error, not a CTA |

---

## Files to Modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Add `Unbound` variant to `ResolutionSource`; write `unbound()` helper; replace `default_fallback()` at Tier 1b (~L409), Tier 1c-post-grace (~L462), Tier 3 (~L506); keep `SpaceDefault` in enum (serde compat) but stop producing it; rewrite module doc |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | Extend `matches!` arm in `log_and_notify_resolution` (~L142) to include `Unbound`; fix stale comment at ~L608 |
| [`crates/mcpmux-gateway/src/services/meta_tools/mod.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/mod.rs) | Add `mcpmux_bind_current_workspace` to `CORE_META_TOOLS` array (~L82) |
| [`crates/mcpmux-gateway/src/admin/command_bridge/read.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/read.rs) | `get_workspace_effective_features` (~L432): when resolver returns `Unbound`, return `feature_set_ids: []` — stop attaching Starter |
| [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) | ~12 tests: flip `SpaceDefault` → `Unbound`, assert `feature_set_ids` is empty |
| [`tests/rust/tests/integration/effective_features.rs`](../../tests/rust/tests/integration/effective_features.rs) | Rename + rewrite `unbound_session_falls_back_to_starter_fs`; update `empty_starter_grants_nothing_to_unbound_session` source assertion |
| [`tests/rust/tests/integration/workspace_binding_events.rs`](../../tests/rust/tests/integration/workspace_binding_events.rs) | 4 pre-bind state assertions: `SpaceDefault` → `Unbound` |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `unmapped-live` card: swap "Unmapped" pill for `card.badgeLiveUnbound`, add primary Bind CTA button; `EffectiveFeaturesContent`: replace Starter list with empty state when `source === 'unbound'` |
| [`apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx) | `sheet.descNew` copy: remove Starter-as-default framing; no-tools banner for `create-from-live` mode |
| [`apps/desktop/src/locales/en/workspaces.json`](../../apps/desktop/src/locales/en/workspaces.json) | Add `card.deniedCta`, `card.deniedTooltip`; fix stale strings: `subtitle`, `confirm.removeMessage`, `sheet.descNew`, `effective.sourceUnboundTooltip` |
| [`apps/desktop/src/lib/api/workspaceBindings.ts`](../../apps/desktop/src/lib/api/workspaceBindings.ts) | Update JSDoc on `source: 'unbound'` to reflect deny-by-default (not Starter preview) |
| [`docs/guide/spaces.mdx`](../guide/spaces.mdx) | Replace stale "active-Space FeatureSet for unmapped sessions" with deny-by-default + opt-in Starter bundle |

---

## Phases

### Phase 0 — Root probe investigation (RESOLVED — no code changes needed)

> **Dig complete.** The probe is working correctly (180 non-empty root reports vs 14 empty today, 93% success rate). The three sessions at 17:17 UTC that "exhausted retries" were cases where Cursor silently dropped the `list_roots()` request for ~6 minutes until the RMCP keepalive timeout (300s) closed the transport. Our retry logic handled them correctly (sessions were "left unresolved" then dead). The `roots=[]` sessions after 17:34 UTC are legitimate — the user had switched to the mcp-mux window, and Cursor correctly reports no roots for that context. No probe bug.

**Findings:**
- Cursor reports correct roots for the active project when it responds (~93% of the time)
- The keepalive-timeout hang is rare (3 sessions today), and those sessions are dead anyway — `Unbound` after Phase 1 is the correct outcome, whether or not roots were ever set
- `roots=[]` is a valid Cursor response when no folder is the active workspace (background/idle connections, or the chat window is focused on a different folder than the one being worked in)
- There is no bug to fix here; Phase 1 seals this path by making `Unbound` the explicit result for all these cases

**`set_workspace_root` as a pin:** Still worth ensuring it's advertised from an `Unbound` session (Phase 2). Not load-bearing but a useful escape hatch for edge cases. No separate phase needed.

---

### Phase 1 — Resolver: deny by default (`Unbound`) (~half day)

**`feature_set_resolver.rs`:**
- Add `Unbound` to `ResolutionSource` enum (after `ClientGrant`, before `Deny`). Keep `SpaceDefault` for serde compat — it's serialized in `set_workspace_root` responses — but stop producing it.
- Write `unbound()` helper next to `default_fallback()`:
  ```rust
  fn unbound(&self, space_id: Option<Uuid>) -> ResolvedFeatureSet {
      ResolvedFeatureSet { feature_set_ids: vec![], space_id, source: ResolutionSource::Unbound, collision_client_id: None }
  }
  ```
- Replace the three `default_fallback()` call sites:
  - **Tier 1b** (~L409): has-roots, no binding match → `self.unbound(target_space)`
  - **Tier 1c post-grace** (~L462): roots-capable, grace elapsed, no roots arrived → `self.unbound(default_space_id)`
  - **Tier 3** (~L506): no roots, no grants → `self.unbound(default_space_id)`
- Rewrite module doc to binding-canonical + ranked-signal framing (root → client grant → machine, none mandatory; roots is a deprecated non-required signal)

**Tests (`tests/rust/tests/integration/`):**
- `feature_set_resolver.rs`: ~12 tests assert `SpaceDefault` — flip to `Unbound`, assert `feature_set_ids.is_empty()`
- `effective_features.rs`: rename `unbound_session_falls_back_to_starter_fs` → assert `Unbound` + empty ids + zero tools; update `empty_starter_grants_nothing_to_unbound_session` source assertion
- `workspace_binding_events.rs`: 4 pre-bind assertions `SpaceDefault` → `Unbound`

**Outcome:** Unmapped folder → zero backend tools, `source = Unbound`. `cargo nextest run -p tests` green.

---

### Phase 2 — Invocation gate + self-bind escape hatch (~quarter day)

**`handler.rs`:**
- `log_and_notify_resolution` (~L142): extend `matches!` arm to include `Unbound`:
  ```rust
  matches!(resolved.source, ResolutionSource::Deny | ResolutionSource::SpaceDefault | ResolutionSource::Unbound)
  ```
- Fix stale comment at ~L608 (`source = Deny` → should say fires on `Deny | Unbound`)

**`meta_tools/mod.rs`:**
- Add `mcpmux_bind_current_workspace` to `CORE_META_TOOLS` array (~L82). It's already registered in `build_default_registry()` — just needs to be surfaced unconditionally in `tools/list`.

**`admin/command_bridge/read.rs`:**
- `get_workspace_effective_features` (~L432): when resolver source is `Unbound`, return `feature_set_ids: []` instead of attaching the Starter. The admin preview must reflect real post-Phase-1 state.

**New integration tests:**
- `Unbound` session: `mcpmux_invoke_tool` on a backend tool returns the denial hint (not a result)
- `Unbound` session: `mcpmux_bind_current_workspace` appears in `list_tools` response

**Outcome:** From an unmapped session, `tools/list` shows `CORE_META_TOOLS` including bind. Backend invoke is denied. `WorkspaceNeedsBinding` fires → auto-pop panel opens.

---

### Phase 3 — Rootless callers as bindable identities (~half day)

> **Deferred — needs its own dig.** No frontend infrastructure for bindable identity cards exists on the Connections page today. The Rust side (`ClientGrant` as the binding anchor) already works. Deferring keeps the current batch focused.

When planned, the work is:
- Tier 2/3: rootless client with no grant → `Unbound` (was Starter fallback); `ClientGrant` is the bound path
- Surface an `Unbound` client identity as a bindable card on the Connections page with deny CTA (reuse `client_grants` write path)
- `RootlessGrantsSection` exists in the panel already; the gap is the card-level denied state

---

### Phase 4 — Projects cards: deny CTA + usage axis (~half day)

**`workspaces.json`:**
- Add `card.deniedCta` ("Bind to enable tools"), `card.deniedTooltip`
- Wire the unused `card.badgeLiveUnbound` ("UNBOUND") onto the `unmapped-live` card
- Fix stale Starter-fallback strings: `subtitle`, `confirm.removeMessage`, `sheet.descNew`, `effective.sourceUnboundTooltip`

**`WorkspacesPage.tsx`:**
- `unmapped-live` card: replace amber "Unmapped" pill with `card.badgeLiveUnbound`; add an explicit primary "Bind to enable tools" button in the card body (keep card `onClick` for panel too)
- `EffectiveFeaturesContent` (~L1450): when `data.source === 'unbound'`, replace the Starter FS list with an empty state — icon + `"No tools on this session yet. Bind this folder to enable access."`

**`workspace-binding-panel.component.tsx`:**
- `sheet.descNew` copy for `create-from-live` mode: replace "already configured with your default Starter tools… close to keep the Starter" with deny-by-default framing ("No backend tools are active on this session. Creating a binding enables tool access.")

**`workspaceBindings.ts`:**
- Update JSDoc on `source: 'unbound'` to reflect deny-by-default

**Outcome:** `unmapped-live` card communicates zero tools + primary bind CTA. Panel copy no longer references Starter as default. `pnpm typecheck && pnpm lint` clean.

---

### Phase 5 — Onboarding journey, Starter bundle & docs reconcile (~half day)

- Onboarding/setup surface: state plainly that the default is **no tools**, and offer the Starter as an **opt-in default bundle** the user can explicitly bind
- Update `docs/guide/spaces.mdx` to deny-by-default + opt-in Starter (remove stale "active-Space FeatureSet" language)
- Triage `collision_client_id`: remove the dead event/UI wiring unless collision detection is wanted (decision recorded here, executed in this phase)

**Outcome:** A first-run user understands default = none and can deliberately opt into the Starter bundle. Docs match behavior. No dead `collision_client_id` paths remain (or a follow-up ticket is filed if detection is kept).

---

## Validation

Run after each phase lands:

```bash
pnpm test:rust        # resolver + integration tests (Phases 1–2)
pnpm typecheck        # TypeScript (Phase 4)
pnpm lint             # ESLint + cargo clippy --workspace -- -D warnings
```

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Resolution tiers, `default_fallback`→`unbound`, `ResolutionSource`, machine-aware lookup — core of Phases 1–3 |
| [`crates/mcpmux-gateway/src/services/session_roots.rs`](../../crates/mcpmux-gateway/src/services/session_roots.rs) | Per-session roots + capability state + grace clock; probe lock + throttle; `list_all_roots()` is the un-attributed usage source (Phase 4 gap) |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | `log_and_notify_resolution`, list/call filtering, meta-tool intercept, `WorkspaceNeedsBinding` emission |
| [`crates/mcpmux-gateway/src/services/meta_tools/mod.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/mod.rs) | `CORE_META_TOOLS`, prefix routing, master switch |
| [`crates/mcpmux-gateway/src/services/meta_tools/invoke_tool.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/invoke_tool.rs) | `mcpmux_invoke_tool` grant check — the invocation gate |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | `Entry`/`EntryKind`, `EntryCardRoutingTable`, auto-pop effect, card states |
| [`apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx) | `workspace-needs-binding` subscription, prompt setting, create-from-live flow |
| [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) | Resolution behavior tests to update for `Unbound` |
| [`tests/rust/tests/integration/effective_features.rs`](../../tests/rust/tests/integration/effective_features.rs) | "empty Starter = zero tools" test → "any Starter = zero tools for unbound" |

---

## Related Documentation

- [`projects-grouped-machine-cards.md`](./projects-grouped-machine-cards.md) — one-card-per-path `Entry.bindings[]`, `EntryCardRoutingTable`, machine rows — the card bones this builds on
- [`workspace-binding-project-adopt.md`](./workspace-binding-project-adopt.md) — cross-machine adopt flow + `live-unbound` badge; the path to enabling a bound-elsewhere folder on this machine
- [`workspace-machine-binding.md`](./workspace-machine-binding.md) — machine CRUD, `machine_id` on bindings, resolver machine lookup
- [`per-device-machine-header.md`](./per-device-machine-header.md) — `X-Mcpmux-Machine-Id` for tunneled multi-device routing
- [SEP-2577 — Deprecate Roots, Sampling, Logging](https://modelcontextprotocol.io/seps/2577-deprecate-roots-sampling-and-logging) — why the binding (not roots) must be canonical
- [MCP client feature support matrix](https://modelcontextprotocol.info/docs/clients/) — which clients report roots (Claude Desktop/Code yes; Cursor/VS Code inconsistent; browser clients no)
