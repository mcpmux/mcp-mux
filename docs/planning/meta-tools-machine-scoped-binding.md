# Meta-Tools Machine-Scoped Binding

**Last Updated:** Jun 30, 2026
**Status:** Implemented (Jun 30, 2026)
**Branch:** `feat/workspace-machine-binding` (continuation)
**Depends on:** [`workspace-machine-binding.md`](./workspace-machine-binding.md), [`per-device-machine-header.md`](./per-device-machine-header.md)
**Unblocks:** Reliable `mcpmux_bind_current_workspace` from tunneled devices (Rohan → Gondor) for brand-new projects

---

## Problem

`per-device-machine-header.md` shipped today and made `tools/list` / `tools/call` machine-aware via `X-Mcpmux-Machine-Id`, but explicitly left meta-tools out — its files-modified table says `meta_tool_common.rs` and `set_workspace_root.rs` "pass `None` for header (meta tools have no HTTP context)".

That's not actually a hard limitation — `oauth_ctx.request_machine_id` is extracted before the meta-tool intercept in `handler.rs`, it just was never threaded into `MetaToolCall`. But leaving it unwired exposed a real bug, confirmed from a Rohan session against Gondor's gateway:

`mcpmux_bind_current_workspace` always writes `client_id = Some(caller), machine_id = None` (`bind_workspace.rs`). Once a gateway has `local_machine_id` set — true for Gondor since today — `FeatureSetResolverService::find_binding_for_roots` only matches machine-scoped or fully-global (`client_id IS NULL AND machine_id IS NULL`) bindings:

```264:269:crates/mcpmux-gateway/src/services/feature_set_resolver.rs
// ponytail: when this install has no machine identity, preserve the
// pre-machine exact match (includes client-scoped bindings). Once
// local_machine_id is set, only machine + global canonical bindings apply.
if self.local_machine_id.read().await.is_none() {
    return self.binding_repo.find_exact_for_roots(roots).await;
}
Ok(None)
```

A client-scoped row matches neither tier. The bind write is a dead write: `already_bound: true` fires truthfully on retry (the dedup check re-reads the same client-scoped row it wrote), but the resolver will never surface it. Setting `X-Mcpmux-Machine-Id` on the caller's device alone does not fix this — the header only changes what the resolver *reads*; the bind tool still writes the wrong scope.

Decided via `propose-opts-brainstorm` (4 options evaluated): write machine-scoped bindings from meta-tools, using the exact same header → client-machine → local-machine priority the resolver already uses to read. This finishes what `per-device-machine-header.md` started instead of leaving meta-tools as the one gap.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Bind write scope | Machine-scoped (`machine_id = effective, client_id = None`) when any machine identity exists anywhere; client-scoped (today's shape) only when none exists at all | Symmetric with the resolver's own read tiers — whatever scope `resolve()` would find, `bind` writes to. Preserves current behavior untouched on fresh installs with no machine registered yet. |
| 2 | Machine priority for writes | Request header (`X-Mcpmux-Machine-Id`) → OAuth client's registered machine (`inbound_clients.machine_id`) → gateway's `local_machine_id` | Mirrors `FeatureSetResolverService::find_binding_for_roots` Tier 1 exactly (new shared `effective_machine_id()` helper) — one source of truth for both read and write instead of two logics that can drift apart again. |
| 3 | API shape for the plumbing | New `MetaToolRegistry::call_from_device(..., request_machine_id)`; existing `call()` delegates with `None` | ~50 existing test call sites use `call()` with 4 args. Adding a 5th required param would touch every one of them for no behavior change on tests that don't exercise device routing. `call_from_device` is the one real call site (`handler.rs`); `call()` keeps meaning "no device context," which is already what every existing test assumes. |
| 4 | Response honesty | After bind (both the fresh-write and the `already_bound` fast paths), re-resolve and return `active: bool` reflecting whether the FeatureSet the caller just bound is actually live for their session right now | `already_bound: true` currently asserts success even when the row it found is invisible to the resolver. A bind tool should never claim success it can't back up. |
| 5 | Already-bound notification | Keep no-op (no new `list_changed`) when the fast path finds nothing changed FS-set-wise; the `active` flag is the guardrail, not extra chatter | Nothing in DB state changes on the fast path — firing `list_changed` for a no-op mutation would be misleading in the other direction. |

---

## Scope

**In:**
- `FeatureSetResolverService::effective_machine_id()` — shared write/read machine-priority helper
- `MetaToolCall.request_machine_id` + `MetaToolRegistry::call_from_device()`
- `handler.rs` meta-tool intercept passes `oauth_ctx.request_machine_id` through
- `caller_resolution()` / `caller_space_id()` in `meta_tool_common.rs` use the real header instead of hardcoded `None`
- `WorkspaceBinding::new_machine_scoped_multi()` constructor
- `bind_workspace.rs` rewritten to branch on `effective_machine_id()`: machine-scoped read/write when any machine identity exists, legacy client-scoped path otherwise
- `bind_workspace.rs` response gains `active: bool` (post-write re-resolve check)
- `set_workspace_root.rs` passes `call.request_machine_id` into `resolve()` instead of `None`
- New/updated integration tests in `tests/rust/tests/integration/{feature_set_resolver,meta_tools}.rs` covering machine-scoped bind + the header/no-header/no-machine-at-all branches
- Reconciliation update to `per-device-machine-header.md` and `workspace-machine-binding.md` closing out the "meta tools have no HTTP context" note

**Out:**

| Item | Reason |
| ---- | ------ |
| Combined `(client_id, machine_id)` scoped writes from meta-tools | Same non-goal as `workspace-machine-binding.md` — three-dimensional scoping adds complexity with no current use case. |
| Migrating existing client-scoped rows written by old gateway versions | No backfill needed — they simply stay invisible once a gateway registers a machine, exactly like any other pre-machine-era client-scoped row; the Workspaces UI already lets a human re-point them. |
| `already_bound` firing `list_changed` | Explicitly decided against — see Decision 5. |

---

## Architecture

### Before (today, on a gateway with `local_machine_id` set)

```
mcpmux_bind_current_workspace
  → find_longest_prefix_match(space_id, client_id=Some(caller), roots)   [dedup check]
  → write WorkspaceBinding { client_id: Some(caller), machine_id: None }

FeatureSetResolverService::resolve
  → find_binding_for_roots(roots, client_id, request_machine_id)
  → local_machine_id is Some ⇒ only checks:
      find_exact_for_machine(machine_id, root, client_id)   [machine_id must match — never does]
      find_exact_global(root)                               [client_id must be NULL — never is]
  → Ok(None)  ← the bind's own row is invisible
```

### After

```
effective_machine_id(client_id, request_machine_id):
    request_machine_id.or(client's registered machine).or(local_machine_id)

mcpmux_bind_current_workspace:
    machine = effective_machine_id(caller, call.request_machine_id)
    match machine:
        Some(m)  → find_exact_for_machine(m, root, None) / write { machine_id: Some(m), client_id: None }
        None     → find_longest_prefix_match(..., client_id=Some(caller)) / write { client_id: Some(caller) }  (unchanged legacy path)
    re-resolve after write → active = fs_id ∈ resolved.feature_set_ids
```

Same `effective_machine_id()` tiering the resolver already uses for reads — a bind from Rohan (header set) writes `machine_id = rohan`; a bind from Gondor's local session (no header, no client machine override) writes `machine_id = gondor`; a bind on a fresh install with nothing registered anywhere falls back to today's client-scoped shape untouched.

---

## Files to Create

None — all changes land in existing files.

## Files to Modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Add `effective_machine_id()` |
| [`crates/mcpmux-gateway/src/services/meta_tools/registry.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/registry.rs) | `MetaToolCall.request_machine_id`; `call_from_device()` |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | Meta-tool intercept calls `call_from_device(..., oauth_ctx.request_machine_id)` |
| [`crates/mcpmux-gateway/src/services/meta_tools/meta_tool_common.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/meta_tool_common.rs) | `caller_resolution()` / `caller_space_id()` use `call.request_machine_id` |
| [`crates/mcpmux-core/src/domain/workspace_binding.rs`](../../crates/mcpmux-core/src/domain/workspace_binding.rs) | Add `new_machine_scoped_multi()`; correct stale `client_id` doc comment ("does not affect resolution yet" — it does, via `find_exact_for_machine`) |
| [`crates/mcpmux-gateway/src/services/meta_tools/bind_workspace.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/bind_workspace.rs) | Branch dedup + write on `effective_machine_id()`; add `active` to response |
| [`crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs) | Pass `call.request_machine_id` into `resolve()` |
| [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) | Test `effective_machine_id()` tiering |
| [`tests/rust/tests/integration/meta_tools.rs`](../../tests/rust/tests/integration/meta_tools.rs) | Test machine-scoped bind + `active` flag; existing ~50 `call()` sites stay green (no machine identity configured in `Fixture` by default) |
| [`docs/planning/per-device-machine-header.md`](./per-device-machine-header.md) | Close out the meta-tools row in files-modified — no longer "pass `None`" |
| [`docs/planning/workspace-machine-binding.md`](./workspace-machine-binding.md) | Append an "Update" section noting meta-tools are now machine-aware |

---

## Phases

### ~~Phase 1 — Thread `request_machine_id` through the meta-tool call path (~1 hour)~~ ✅ DONE

- ~~`FeatureSetResolverService::effective_machine_id(client_id, request_machine_id)` — header → client machine → local machine, `None` if nothing registered~~
- ~~`MetaToolCall.request_machine_id: Option<Uuid>` field~~
- ~~`MetaToolRegistry::call_from_device()` (new) + `call()` delegates with `None` so existing test call sites don't change~~
- ~~`handler.rs` meta-tool intercept calls `call_from_device(..., oauth_ctx.request_machine_id)` instead of `call()`~~
- ~~`caller_space_id()` in `meta_tool_common.rs` passes `call.request_machine_id` instead of hardcoded `None`~~

**Outcome:** ✅ The real device header reaches every meta tool's resolver call for space/FS resolution. `cargo check -p mcpmux-gateway` compiles clean with these four files changed (verified via `git diff --stat`: `handler.rs`, `feature_set_resolver.rs`, `meta_tool_common.rs`, `registry.rs`, 53 insertions).

**Remaining in this phase:** `caller_resolution()` in `meta_tool_common.rs` still has one more call site to update to `call.request_machine_id` (currently only `caller_space_id()` is done) — finish this as the first step of Phase 2 since `bind_workspace.rs` depends on it.

---

### Phase 2 — Machine-scoped binding writes in `bind_workspace.rs` (~2 hours)

- Finish `caller_resolution()` in `meta_tool_common.rs` (the one leftover call site from Phase 1)
- `WorkspaceBinding::new_machine_scoped_multi(workspace_root, space_id, machine_id: Uuid, feature_set_ids)` constructor in `domain/workspace_binding.rs`, alongside existing `new_scoped_multi`
- Correct the stale doc comment on `WorkspaceBinding.client_id` — it currently claims client scope "does not affect resolution yet," which stopped being true once `find_exact_for_machine` started accepting `client_id` as a secondary filter
- In `bind_workspace.rs`: compute `machine = call.ctx.resolver.effective_machine_id(Some(call.client_id), call.request_machine_id).await?` once, before the dedup check
- Dedup check + write branch on `machine`:
  - `Some(m)` → look up via `find_exact_for_machine(&m, &normalized, None)`; create/update with `machine_id: Some(m), client_id: None`
  - `None` → unchanged: `find_longest_prefix_match(..., Some(caller_client_id), ...)` / `new_scoped_multi(..., Some(caller_client_id), ...)`

**Outcome:** A bind from a session carrying `X-Mcpmux-Machine-Id` (or whose OAuth client has `inbound_clients.machine_id` set, or on a gateway with `local_machine_id` set) writes a binding the resolver can actually find on the very next `tools/list`. Existing `bind_current_workspace_*` integration tests still pass unmodified (their `Fixture` registers no machine identity, so they exercise the untouched `None` branch).

---

### Phase 3 — Honest bind response (~30 min)

- After the write (or the `already_bound` fast-path hit), re-resolve: `call.ctx.resolver.resolve(call.session_id, Some(call.client_id), call.request_machine_id)`
- Add `"active": bool` to the JSON response — `true` iff `fs_id` is in `resolved.feature_set_ids`
- When `active: false`, add a `"note"` field explaining why (e.g. "bound, but this gateway has a different machine identity active for this session — check Settings → Machine Identity")

**Outcome:** `mcpmux_bind_current_workspace` never again reports `already_bound: true` (or a fresh bind) while silently failing to activate. An LLM (or Rohan) gets a truthful, actionable signal in the same response instead of having to separately probe with `mcpmux_list_servers`.

---

### Phase 4 — `set_workspace_root.rs` machine-awareness (~15 min)

- Replace the hardcoded `None` in `resolver.resolve(Some(session_id), Some(call.client_id), None)` with `call.request_machine_id`

**Outcome:** The manual root-injection escape hatch resolves against the same machine scope a normal `tools/list` would use for that session, instead of always resolving as if no header were present.

---

### Phase 5 — Tests + validation (~1.5 hours)

- `feature_set_resolver.rs` integration tests: `effective_machine_id()` returns header first, then client machine, then local machine, then `None`
- `meta_tools.rs` integration tests: bind with a machine identity configured writes `machine_id` not `client_id`; bind is then visible to a second session reporting the same root with the same machine header; `active: false` case when a global/other-machine binding shadows it
- Run `pnpm test:rust:int`, `cargo clippy --workspace -- -D warnings`, `pnpm typecheck` (no TS surface touched, but part of `pnpm validate`)

**Outcome:** `cargo nextest run -p tests` green including new machine-scoped bind cases; clippy clean; no regression in the ~50 existing meta-tool test call sites.

---

### Phase 6 — Docs reconciliation (~15 min)

- `per-device-machine-header.md`: update the files-modified rows for `meta_tool_common.rs` / `set_workspace_root.rs` — no longer "pass `None` for header"
- `workspace-machine-binding.md`: append an "Update — [date]" section noting meta-tools now write machine-scoped bindings

**Outcome:** Both planning docs accurately describe the shipped state — no doc still claims meta-tools lack machine context.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`docs/planning/per-device-machine-header.md`](./per-device-machine-header.md) | Origin of the `request_machine_id` header this phase finishes wiring into meta-tools |
| [`docs/planning/workspace-machine-binding.md`](./workspace-machine-binding.md) | Machine catalog, partial unique index pattern, resolver tiering this mirrors |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | `find_binding_for_roots` Tier 1 — the read-side priority `effective_machine_id()` mirrors |
| [`crates/mcpmux-gateway/src/mcp/context.rs`](../../crates/mcpmux-gateway/src/mcp/context.rs) | `OAuthContext.request_machine_id` extraction from `X-Mcpmux-Machine-Id` |
| [`crates/mcpmux-gateway/src/services/meta_tools/bind_workspace.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/bind_workspace.rs) | Current client-scoped-only write path being replaced |
| [`crates/mcpmux-core/src/domain/workspace_binding.rs`](../../crates/mcpmux-core/src/domain/workspace_binding.rs) | `new_scoped_multi` precedent for the new `new_machine_scoped_multi` |
| [`crates/mcpmux-core/src/repository/mod.rs`](../../crates/mcpmux-core/src/repository/mod.rs) | `find_exact_for_machine` / `find_exact_global` / `find_longest_prefix_match` semantics |
| [`tests/rust/tests/integration/meta_tools.rs`](../../tests/rust/tests/integration/meta_tools.rs) | ~50 existing `registry.call()` sites that must stay green |

---

## Related Documentation

- [`per-device-machine-header.md`](./per-device-machine-header.md) — the feature this closes the last gap in
- [`workspace-machine-binding.md`](./workspace-machine-binding.md) — machine catalog and binding model this builds on
- Originating conversation: `propose-opts-brainstorm` evaluation of 4 options for the bind-write-scope decision (Option 2 chosen)
