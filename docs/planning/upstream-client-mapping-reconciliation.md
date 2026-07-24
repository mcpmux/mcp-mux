# Upstream Client Mapping Reconciliation (`upstream/main` #201–#206 → `dev-rebased`)

**Last Updated:** Jul 16, 2026
**Status:** Planning — not started
**Branch:** N/A — each phase is a separate feature branch off `dev-rebased`
**Base branch:** `dev-rebased` (fork lineage, currently at migration `035_inbound_client_machine.sql`)
**Depends on:** `dev-to-main-port.md` migration-renumbering precedent (020–031 map); this doc extends that pattern for a second wave
**Unblocks:** Headless/remote MCP client support (`mcpk_` API keys) landing on the fork without regressing deny-by-default or machine-scoped routing

---

## Problem

Upstream shipped a 3-PR stacked series (#201 P1, #202 P2, #203 P3) plus two gateway fixes (#205, #206) on Jul 15, 2026 — all after this fork diverged. The series adds:

- **API-key inbound auth** (`mcpk_…` Bearer tokens) for headless/remote clients that can't complete the `mcpmux://` OAuth consent deep link
- **Generalized client → Space/FeatureSet mappings** with a `binding_type` (`path` | `id`) column, plus **lock-confine** (`inbound_clients.locked_space_id`) that pins an API-key client to one Space
- **Nav rename** (Apps → Clients, Workspaces → Mapping) and a non-localhost consent warning

This fork already solved a related but different problem: **deny-by-default routing** (`docs/planning/deny-by-default-bindable-callers.md`) replaced upstream's old `SpaceDefault`/Starter silent fallback with `Unbound` (zero tools until explicitly bound), and **machine-scoped bindings** (`docs/planning/workspace-machine-binding.md`) added a `client_id`/`machine_id` dual-scope axis to `WorkspaceBinding` that upstream's simpler clientId-only model doesn't have.

Porting upstream's series verbatim would reintroduce the exact `SpaceDefault` fallback this fork removed in migration `006_collapse_feature_sets.sql`, and would collide with fork migrations `020`–`035` (upstream's new migrations also start at `020`).

This is a second, narrower wave of the same problem `dev-to-main-port.md` already solved once — a new upstream migration range needs to slot in without disturbing the fork's routing semantics.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Unbound vs SpaceDefault | **Keep fork `Unbound` everywhere** — do not adopt upstream's `SpaceDefault`/Starter fallback, including for new API-key clients | Deny-by-default is a deliberate, already-shipped product decision (`deny-by-default-bindable-callers.md`). Reintroducing a silent Starter fallback for API-key clients would create two different default behaviors for the same session type depending on auth method. |
| 2 | `locked_space_id` | **Hybrid** — a client can be locked to one Space, but its FeatureSet within that Space still resolves through the normal binding/machine-scope tiers, not a second hardcoded Starter-style default | Space lock is a real, useful primitive (confine a remote API-key client to one project). But bypassing the resolver entirely (upstream's `resolve_locked()`) would let a locked client dodge deny-by-default. Lock narrows the *Space* search space; it does not grant tools by itself. |
| 3 | Port scope | **All of P1–P3, phased** — bring in API-key auth, mappings, and the nav/consent polish, but sequenced so routing-semantics changes (Decision 1/2) land before any UI that assumes upstream's original defaults | Splitting P1/P2/P3 further than upstream did avoids a half-ported state where API keys exist but mapping UI still assumes `SpaceDefault`. |
| 4 | Identity model | **Both, with an explicit precedence order** — upstream's `binding_type` (`path` \| `id`) coexists with the fork's `client_id`/`machine_id` scoping axes | Upstream's `id` binding type and the fork's clientId mapping solve overlapping cases (routing a rootless/API-key client by identity instead of path) using different schemas. Rather than pick one, both ship with one documented resolution order so machine-scoped homelab routing and simple clientId routing both work. |
| 5 | Nav labels | **Keep fork naming (Projects / Bundles / Clients)** — borrow "Mapping" terminology only in copy/tooltips where it clarifies the new id-binding concept, not as a route/nav rename | The fork's naming is already shipped and tested (`navigation.ts`, e2e testids). A full rename for a two-PR upstream change isn't worth the i18n/testid churn; the *concept* of "mapping" (client → Space/FeatureSet) is what's useful, not the label. |
| 6 | Migration renumbering | **Squash/rewrite into one reconciled migration set**, not a straight append or fork-renumber | Fork already owns `020`–`035`. Upstream's `020`–`022` (api keys, binding_type, locked_space) need new numbers *and* their column/table shapes need to be reconciled against the fork's existing `workspace_bindings.client_scope`/`machine_scope` columns and `inbound_clients` schema before they're written — a straight renumber would leave two overlapping-but-different scoping mechanisms in the schema with no unification. |

---

## Scope

**In:**

- `inbound_client_api_keys` table + `mcpk_` Bearer auth path (upstream #201), renumbered and landed after fork's `035`
- `gateway.auth_disabled` auto-start fix (upstream #205) — additive, no fork conflict
- `structuredContent`/`_meta` passthrough fix on `tools/call` (upstream #206) — additive, no fork conflict
- `binding_type` (`path` | `id`) column on `workspace_bindings`, reconciled against existing `client_scope`/`machine_scope` columns into one resolver precedence table
- `locked_space_id` on `inbound_clients`, implemented as a Space-level narrowing filter ahead of the existing resolver tiers — not a bypass
- API-key client registration UI (`RegisterApiKeyClientModal`, `ClientApiKeysSection`) with the "Lock to a Space" option
- Non-localhost OAuth consent note pointing remote users to API-key clients (upstream #203), gated on `network_bind`
- Documented precedence order for: header/roots path binding → id binding (client/machine-scoped) → locked-Space-scoped id binding → deny (`Unbound`)

**Out:**

| Item | Reason / Deferral |
| ---- | ----------------- |
| Upstream `SpaceDefault`/Starter fallback for any tier | Rejected per Decision 1 — conflicts with shipped deny-by-default behavior. |
| Upstream nav rename (Workspaces → Mapping, Apps → Clients) | Rejected per Decision 5 — fork naming stays; concept ships without the route/label churn. |
| `resolve_locked()` as an early-exit bypass of the roots/machine resolver tiers | Rejected per Decision 2 — would let locked clients skip deny-by-default. |
| Legacy in-memory `AccessKey`/`AccessKeyAuth` extractor in `crates/mcpmux-gateway/src/auth/mod.rs` | Stale, unwired dead code predating both upstream and fork API-key work. Delete in a follow-up cleanup ticket, not bundled into this port to keep the diff reviewable. |
| Unifying `client_id` on `workspace_bindings` with `inbound_clients.machine_id` into one caller-identity model | Flagged as a Future TODO in `workspace-machine-binding.md` already; out of scope here — this port adds a *third* identity axis (`binding_type: id`) on top without forcing that unification yet. |

---

## Architecture

### Resolver precedence (post-port)

The existing binding-canonical resolver (`feature_set_resolver.rs`) gains one new signal (`binding_type: id`) and one new narrowing filter (Space lock), inserted without disturbing the existing tiers:

```text
0. Space lock (if inbound_clients.locked_space_id is set)
   → narrows all subsequent tiers to bindings/grants within that Space only
   → no match within the locked Space → Unbound (NOT the locked Space's Starter)

1. reported root / X-Mcpmux-Workspace  → path-type WorkspaceBinding
                                          (machine: client → local → global, existing)
2. clientId (API-key or OAuth client)  → id-type WorkspaceBinding (NEW — upstream's binding_type=id)
3. client identity, rootless           → ClientGrant (existing, default Space only unless locked)
4. machine                             → scopes (1) and (2); bound-elsewhere → no match on this machine (existing)
   else                                → Unbound (deny by default, unchanged)
```

Tier 2 is new. It sits between the fork's existing path-binding tier and its `ClientGrant` tier, matching upstream's original ordering intent (header/roots binding beats clientId mapping beats grants) while preserving the fork's `Unbound` terminus instead of upstream's `SpaceDefault` terminus.

### Schema reconciliation

| Upstream column (their migration) | Fork equivalent already present | Reconciliation |
| --- | --- | --- |
| `workspace_bindings.binding_type` (`021_binding_type.sql`) | none — fork bindings are implicitly path-typed | Add as a new column in the squashed migration; existing rows backfill `'path'` |
| `inbound_clients.locked_space_id` (`022_inbound_client_locked_space.sql`) | none | Add as a new nullable FK column in the squashed migration |
| `inbound_client_api_keys` table (`020_inbound_client_api_keys.sql`) | none | Add table as-is; no fork equivalent to reconcile against |
| — | `workspace_bindings.client_id` (`027_workspace_binding_client_scope.sql`) | Kept distinct from `binding_type: id` rows — this is a *scope* on a path binding, not an identity-keyed binding. Both can match; Tier 1 (path+scope) still runs before Tier 2 (id-type). |
| — | `workspace_bindings.machine_id` (`034_workspace_binding_machine_scope.sql`) | Applies to id-type bindings too; an id-type binding can also be machine-scoped. |

### Files to create / modify

| Area | File cluster | Action |
| ---- | ------------- | ------ |
| Storage | `crates/mcpmux-storage/src/migrations/036_inbound_client_api_keys.sql` | Create — squashed/renumbered from upstream `020` |
| Storage | `crates/mcpmux-storage/src/migrations/037_workspace_binding_type.sql` | Create — squashed/renumbered from upstream `021`, adjusted to coexist with `client_scope`/`machine_scope` |
| Storage | `crates/mcpmux-storage/src/migrations/038_inbound_client_locked_space.sql` | Create — squashed/renumbered from upstream `022` |
| Storage | `crates/mcpmux-storage/src/database.rs` | Modify — register `036`–`038` |
| Storage | `crates/mcpmux-storage/src/repositories/inbound_client_repository.rs` | Modify — add `hash_token`/`validate_api_key`/`create_api_key`/`revoke_api_key`, `get_locked_space`/`set_locked_space` |
| Storage | `crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs` | Modify — add `find_by_id_key`, keep existing `find_exact_for_roots`/`find_exact_for_machine` untouched |
| Core | `crates/mcpmux-core/src/domain/workspace_binding.rs` | Modify — `BindingType` enum, `new_id()` constructor |
| Gateway | `crates/mcpmux-gateway/src/mcp/oauth_middleware.rs` | Modify — add API-key Bearer branch after existing JWT check, before `auth_disabled` anonymous fallback |
| Gateway | `crates/mcpmux-gateway/src/services/feature_set_resolver.rs` | Modify — insert Tier 2 (id-binding) and Tier 0 (Space lock narrowing); no fallback-to-Starter path added |
| Gateway | `crates/mcpmux-gateway/src/server/handlers.rs` | Modify — non-localhost consent note gated on `network_bind` (upstream #203, additive) |
| Gateway | `crates/mcpmux-gateway/src/mcp/handler.rs` | Modify — restore `structuredContent`/`_meta` passthrough on `tools/call` (upstream #206, additive) |
| Tauri | `apps/desktop/src-tauri/src/commands/oauth.rs` | Modify — `register_api_key_client`, `create_client_api_key`, `list_client_api_keys`, `revoke_client_api_key`, optional `locked_space_id` param |
| Tauri | `apps/desktop/src-tauri/src/commands/gateway.rs` | Modify — restore `auth_disabled` seeding on auto-start (upstream #205, additive) |
| UI | `apps/desktop/src/features/clients/RegisterApiKeyClientModal.tsx` | Create |
| UI | `apps/desktop/src/features/clients/ClientApiKeysSection.tsx` | Create |
| UI | `apps/desktop/src/features/clients/ClientsPage.tsx` | Modify — "Register client" entry point + API-key panel |
| UI | `apps/desktop/src/features/workspaces/WorkspaceSetupWizard.tsx` | Modify — Folder vs ID/label toggle at binding create time |
| UI | `apps/desktop/src/features/workspaces/WorkspacesPage.tsx` | Modify — id-type binding display alongside existing path/machine grouping |
| UI | `apps/desktop/src/lib/api/workspaceBindings.ts` | Modify — `binding_type` on types |
| Tests | `tests/rust/tests/streamable_http/api_key_auth.rs` | Create — live/unknown/revoked key auth cases |
| Tests | `tests/rust/tests/integration/feature_set_resolver.rs` | Modify — add id-binding + Space-lock-narrowing cases; assert `Unbound` (not `SpaceDefault`) as the deny terminus in all new tests |

---

## Phases

### Phase 1 — API-key inbound auth, no routing changes (~1 day)

Pure auth-layer addition. No resolver semantics change, so nothing here can regress deny-by-default.

- `036_inbound_client_api_keys.sql` migration + `inbound_client_repository.rs` API-key CRUD (hash/validate/create/revoke)
- `oauth_middleware.rs` — API-key Bearer branch, tried after JWT, before `auth_disabled` anonymous fallback
- Tauri commands: `register_api_key_client`, `create_client_api_key`, `list_client_api_keys`, `revoke_client_api_key`
- UI: `RegisterApiKeyClientModal`, `ClientApiKeysSection`, wire into `ClientsPage.tsx` (no "Lock to a Space" option yet — that's Phase 2)
- Registered API-key clients get **no implicit binding** — they resolve through the existing tiers exactly like any other client, which today means `Unbound` until a binding or grant exists
- `tests/rust/tests/streamable_http/api_key_auth.rs` — live/unknown/revoked key cases

**Outcome:** An `mcpk_` API key authenticates a request and resolves `client_id`, but grants zero tools until that client is bound or granted through the existing (unchanged) mechanisms. `pnpm test:rust` green; no existing resolver test needs to change.

---

### Phase 2 — id-type bindings + resolver Tier 2 (~1 day)

Introduces the new routing signal without touching the deny terminus.

- `037_workspace_binding_type.sql` — `binding_type` column, backfill existing rows to `'path'`
- `crates/mcpmux-core/src/domain/workspace_binding.rs` — `BindingType` enum, `new_id()`
- `workspace_binding_repository.rs` — `find_by_id_key()`
- `feature_set_resolver.rs` — insert Tier 2 (`mapping_binding_for_roots` equivalent: id-type lookup on `client_id`) between the existing path tier and `ClientGrant` tier; all non-matching paths still terminate at `unbound()`, never `default_fallback()`
- UI: `WorkspaceSetupWizard` gets the Folder vs ID toggle; `WorkspacesPage.tsx` displays id-type bindings
- `feature_set_resolver.rs` integration tests: new id-binding cases, explicitly asserting `Unbound` (not `SpaceDefault`) when no id binding matches

**Outcome:** A client can be routed by clientId instead of path (e.g. a rootless API-key client bound directly to a FeatureSet), while a client with neither a path nor an id binding still gets zero tools. `pnpm test:rust` green, all existing `Unbound`-asserting tests unchanged.

---

### Phase 3 — Space lock as a narrowing filter (~half day)

Adds the lock primitive as Decision 2 specifies: a filter, not a bypass.

- `038_inbound_client_locked_space.sql` — `locked_space_id` on `inbound_clients`
- `inbound_client_repository.rs` — `get_locked_space`/`set_locked_space`
- `feature_set_resolver.rs` — Tier 0: if a lock exists, restrict Tiers 1–4 to bindings/grants within `locked_space_id`; no match within the locked Space still falls through to `unbound()`, not a locked-Space Starter
- `RegisterApiKeyClientModal.tsx` — "Lock to a Space" dropdown at registration
- Integration tests: locked client + matching in-Space binding → resolves; locked client + binding in a *different* Space → `Unbound` (not the locked Space's fallback); locked client + no binding at all → `Unbound`

**Outcome:** An API-key client can be confined to one Space for security, but still needs an explicit binding or grant within that Space to get any tools — deny-by-default holds inside the lock boundary too. `pnpm test:rust` green.

---

### Phase 4 — Gateway fixes + consent polish (~quarter day)

The two standalone upstream fixes, both additive and independent of Phases 1–3.

- `gateway.rs` / `lib.rs` — restore `auth_disabled` seeding on gateway auto-start (upstream #205)
- `handler.rs` — restore `structuredContent`/`_meta` passthrough on `tools/call` forwarding (upstream #206)
- `handlers.rs` — non-localhost OAuth consent note, gated on `network_bind`, pointing remote users at API-key clients (upstream #203, UI-adjacent only)
- Regression tests for both fixes ported from upstream

**Outcome:** Gateway auto-start honors the persisted auth-disabled setting; downstream tool results with `outputSchema` keep their structured content through the gateway; remote users hitting the OAuth consent page over a non-loopback bind see guidance toward API keys instead of a dead-end browser flow. `pnpm validate` clean.

---

## Key files referenced

| File | Note |
| ---- | ---- |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Core of Phases 2–3; existing `Unbound` terminus must survive both new tiers |
| [`crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs`](../../crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs) | `client_scope`/`machine_scope` lookups this port must coexist with, not replace |
| [`crates/mcpmux-storage/src/database.rs`](../../crates/mcpmux-storage/src/database.rs) | Migration registration array — next free slot is `036` |
| [`crates/mcpmux-gateway/src/mcp/oauth_middleware.rs`](../../crates/mcpmux-gateway/src/mcp/oauth_middleware.rs) | JWT-then-API-key auth chain, `auth_disabled` anonymous fallback |
| [`docs/planning/deny-by-default-bindable-callers.md`](./deny-by-default-bindable-callers.md) | The shipped decision this port must not regress — `Unbound` vs `SpaceDefault` |
| [`docs/planning/workspace-machine-binding.md`](./workspace-machine-binding.md) | Existing `client_id`/`machine_id` scope axes this port's `binding_type: id` must slot in alongside |
| [`docs/planning/dev-to-main-port.md`](./dev-to-main-port.md) | Precedent for migration-renumbering-as-feature-branch-PRs approach reused here |

---

## Related documentation

- [`docs/planning/deny-by-default-bindable-callers.md`](./deny-by-default-bindable-callers.md) — why `Unbound` exists and must stay the deny terminus
- [`docs/planning/workspace-machine-binding.md`](./workspace-machine-binding.md) — existing dual client/machine scope axis on `WorkspaceBinding`
- [`docs/planning/per-device-machine-header.md`](./per-device-machine-header.md) — `X-Mcpmux-Machine-Id` routing that Tier 2's id-binding lookup must respect
- [`docs/planning/dev-to-main-port.md`](./dev-to-main-port.md) — migration-renumbering precedent and phase-ordering rationale reused in this doc
