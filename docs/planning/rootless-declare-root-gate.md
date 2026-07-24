# Rootless Declare-Before-Grant Gate

**Last Updated:** Jul 23, 2026
**Status:** Planning — ready to implement
**Branch:** `dev-rebased` (fork-only for now; no upstream port planned)
**Depends on:** [`deny-by-default-bindable-callers.md`](./deny-by-default-bindable-callers.md) (Tier 4 `Unbound`, `ClientGrant` Tier 3), [`per-device-machine-header.md`](./per-device-machine-header.md), [`workspace-machine-binding.md`](./workspace-machine-binding.md)
**Unblocks:** Cloud Agent / rootless clients get scoped-by-repo tools instead of a permanent blanket grant

---

## Problem

Origin: `mux-remote-cloud-agent-access` session (Jul 22–23, 2026). Cursor Cloud Agents are rootless and HTTP-proxied through Cursor's backend — the request never originates on the VM, so there's no per-session `roots` broadcast and no way to template per-repo headers on a dashboard MCP entry. The only mechanism that made cloud agents work (Jul 22) is a **"Default for rootless sessions" `client_grants` row** — a blanket, permanent grant with no identity check.

Verified in code (not assumed) during this session's dig:

- **Tier ordering** in `feature_set_resolver.rs`: Tier 2 (id `WorkspaceBinding`) → **Tier 3 (`ClientGrant`, lines 614–641)** → Tier 4 (`Unbound`, lines 644–653). Any client with a non-empty grant returns at Tier 3 and **never reaches** Tier 4. Deny-by-default (`Unbound`) is dead code for a client that has a grant.
- Tier 3 is **completely unconditional** on root/identity state today — a client gets its grant's full tool surface whether or not it has ever declared what repo/project it represents.
- `mcpmux_set_workspace_root` (existing meta tool) writes into the **same** `session_roots` registry Tier 1 reads (`session_roots.rs:182-209`). It is not a separate "declaration" channel — calling it makes the session look exactly like a roots-capable session that reported a root.
- **Tier 1b is a trap for this use case**: when a session has *any* root and no exact-path `WorkspaceBinding` match, it **hard-returns `Unbound` immediately** (`feature_set_resolver.rs:499-539`) — it does **not** fall through to Tier 2/3. So naively calling `mcpmux_set_workspace_root("/workspace/repo")` from a cloud VM (whose path will never exact-match a desktop binding at `/Users/joe/Desktop/Repos/...`) would make the cloud client **worse off** than today (zero tools instead of blanket tools).
- Path matching is **exact string equality only** (`find_exact_for_roots`, `workspace_binding_repository.rs:316-326`) — no basename/repo-name fuzzy matching exists anywhere in the resolver today.

**Net: there is no existing "declare identity before you get tools" gate.** Building one requires resolver changes, not a config toggle.

---

## Decisions

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | Gate scope | **Global** — applies to all rootless (`roots_capable == false`) clients, not an opt-in per-client flag | User's call: simpler mental model, one behavior for all rootless clients going forward |
| 2 | Declaration matching | **Repo-name-assisted, with fallback.** On declaration, first try to match the declared path's basename (or an explicit repo-name hint) against known repo/Bundle names; if a name match is found, route to that repo's scoped binding/Bundle. If no match, fall back to "any non-empty declaration satisfies the gate" and unlock the client's existing grant (blanket, as today) | User's call: best-effort precision now without requiring a full path-identity system; degrades gracefully to today's behavior when there's no match |
| 3 | Tier 1b behavior for rootless clients | **Change it.** For a session with `roots_capable == false` (true rootless, not a roots-capable client mid-probe), a declared-but-unmatched root must **fall through to Tier 3** instead of hard-denying. Tier 1b's existing hard-deny behavior for roots-*capable* sessions is unchanged | This is the actual bug fix that makes declare-then-grant possible — without it, declaring is strictly worse than not declaring |
| 4 | Gate mechanism | New pre-Tier-3 check: Tier 3 only returns a grant if `session_roots.get(session_id)` is non-empty (or a valid `X-Mcpmux-Workspace`/`X-Mcpmux-Machine-Id`-equivalent identity signal is present). Otherwise return a new/reused "awaiting declaration" state that still surfaces `CORE_META_TOOLS` (`mcpmux_set_workspace_root`) so the client can self-unblock | Mirrors existing `PendingRoots` prior art — same shape, no backend tools, meta tools available, re-resolve on declaration |
| 5 | `ResolutionSource` | Reuse `PendingRoots` for "awaiting declaration" rather than adding a new enum variant, unless UI/logging needs to distinguish "waiting on MCP roots probe" from "waiting on rootless client to call `set_workspace_root`" — revisit during implementation if telemetry needs differ | Keeps the enum small; only split if the two states need genuinely different UI copy |
| 6 | Upstream port | **Not now.** This stays fork-only on `dev-rebased`, same as the two Jul 22 bug fixes (settings key migration, restart race) | User's call (4a): don't block on porting to `main` yet |

---

## Scope

**In:**
- New pre-Tier-3 gate in `feature_set_resolver.rs`: rootless client with no declared root/identity signal → do not return `ClientGrant`; return `PendingRoots`-shaped empty state instead (meta tools only, `mcpmux_set_workspace_root` reachable)
- Repo-name-assisted matching: on declaration, attempt to resolve the declared path to a known repo/Bundle by name before falling back to "any declaration unlocks the existing grant"
- Modify Tier 1b so `roots_capable == false` sessions with a declared-but-unmatched root fall through to the new gate/Tier 3 instead of hard-denying (`Unbound`)
- Integration tests mirroring the existing `PendingRoots`/`ClientGrant`/`Unbound` test suite for the new "declared but unmatched" and "declared and name-matched" paths
- Re-resolve + `tools/list_changed` on successful declaration (existing `mcpmux_set_workspace_root` behavior — confirm it still fires correctly under the new gate)

**Out:**
- Per-client opt-in flag (decision 1: global, not per-client) — no new `inbound_clients` column
- Full path-identity system (canonical repo registry, git-remote-based matching, etc.) — repo-name matching is best-effort basename comparison against existing Bundle/binding names, not a new subsystem
- Upstream port to `main` (decision 6)
- Any change to roots-*capable* client behavior (Claude Desktop/Code, native Cursor) — this only changes behavior for clients where `roots_capable == false`

---

## Architecture

### Where the gate lives

```text
Tier 2  id WorkspaceBinding           (unchanged)
   ↓ miss
Tier 3  ClientGrant                    ← NEW: gate added here
   ↓
   has session declared a root/identity signal?
     NO  → return PendingRoots-shaped empty state (meta tools only)
     YES → attempt repo-name match against known Bundles/bindings
             MATCH    → route to that binding's FeatureSet(s)
             NO MATCH → fall through to existing grant lookup (today's behavior)
   ↓ still miss
Tier 4  Unbound                        (unchanged fallback)
```

### Tier 1b change (rootless only)

```text
has_roots == true, roots_capable_known == Some(false), no exact-path binding match:
  BEFORE: return Unbound immediately
  AFTER:  fall through to the Tier 3 gate above (treat the declared root as
          the "identity signal" the gate is waiting for)

has_roots == true, roots_capable_known == Some(true) or None (roots-capable / probing):
  UNCHANGED: hard Unbound on no match — this is the existing, correct
  behavior for real MCP-roots clients and must not regress.
```

### Repo-name matching (Decision 2)

Best-effort, not a new subsystem:
1. Take the basename of the declared path (or an explicit repo-name argument if we extend `mcpmux_set_workspace_root`'s schema — revisit during implementation whether the tool needs a `repo_name` hint param vs. inferring from path)
2. Compare against known `WorkspaceBinding.workspace_root` basenames and/or Bundle display names for the client's default space
3. First match wins; ties/none → fall back to the client's existing grant (Decision 2's stated fallback)

---

## Files to Modify

| File | Change |
|------|--------|
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | New pre-Tier-3 gate; modify Tier 1b fall-through condition for `roots_capable == Some(false)`; repo-name match helper |
| [`crates/mcpmux-gateway/src/services/session_roots.rs`](../../crates/mcpmux-gateway/src/services/session_roots.rs) | Confirm `get()`/`set()` semantics are sufficient as the "declared" signal; no new storage expected unless repo-name hint needs a side channel |
| [`crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs) | Confirm re-resolve + `tools/list_changed` still fire correctly once the gate is in place; consider optional `repo_name` param |
| [`crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs`](../../crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs) | Add a basename/name-lookup helper alongside existing `find_exact_for_roots` (do not change exact-match semantics for roots-capable clients) |
| [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) | New tests: rootless + no declaration → `PendingRoots`-shaped empty; rootless + declared + name match → scoped grant; rootless + declared + no match → existing blanket grant; roots-capable Tier 1b behavior unchanged (regression guard) |

---

## Phases

### Phase 1 — Resolver gate + Tier 1b fall-through (~half day)
- Add the pre-Tier-3 gate (Decision 4)
- Modify Tier 1b for `roots_capable == Some(false)` only
- Regression tests confirming roots-capable Tier 1b hard-deny is untouched

**Outcome:** A rootless client with an active grant but no declared root gets `PendingRoots`-shaped empty tool lists (`tools/list` returns `CORE_META_TOOLS` only) instead of its blanket grant. Calling `mcpmux_set_workspace_root` re-resolves and unblocks it via the existing fallback-to-grant path. Roots-capable clients (native Cursor, Claude Desktop) show zero behavior change — verified by regression tests, not just "looks unchanged."

### Phase 2 — Repo-name matching (~half day)
- Basename/name-lookup helper
- Wire into the gate's "declared but unmatched" branch
- Tests for match / no-match / fallback-to-grant paths

**Outcome:** A rootless client that declares a root whose basename matches a known repo/Bundle name (e.g. `mcp-mux`) gets routed to that repo's scoped FeatureSet(s), not the blanket grant. A client that declares a root with no name match still falls back to its existing grant (today's behavior), proven by a test that declares an unrecognizable path and asserts the blanket grant still applies.

### Phase 3 — Validation (~1 hour)
```bash
pnpm test:rust
pnpm typecheck
pnpm lint
```
- Manual check: existing "Default for rootless sessions" cloud client still works end-to-end after declaring a root that doesn't match any known repo name (confirms the fallback path, not just the happy path)

**Outcome:** `pnpm test:rust`, `pnpm typecheck`, and `pnpm lint` all green. A live Cursor Cloud Agent session on `jsg-tech-check` shows `mcpmux_search_tools` returning zero backend tools until it calls `mcpmux_set_workspace_root`, then returns the correct (or blanket-fallback) tool set immediately after.

---

## Key Files Referenced

| File | Notes |
|------|-------|
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Tier 2/3/4 ordering confirmed by dig (lines 587–653); this is where the new gate and Tier 1b change both land |
| [`crates/mcpmux-gateway/src/services/session_roots.rs`](../../crates/mcpmux-gateway/src/services/session_roots.rs) | Confirmed `set()`/`get()` (lines 182–209) is the single registry both MCP-protocol roots and `mcpmux_set_workspace_root` write into — no separate "declaration" channel exists today |
| [`crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/set_workspace_root.rs) | Existing injection + re-resolve + `list_changed` logic (lines 73–91) this plan reuses as the declaration mechanism |
| [`crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs`](../../crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs) | `find_exact_for_roots` (lines 316–326) confirmed exact-string-only — basis for the new basename-matching helper in Phase 2 |
| [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) | Existing `PendingRoots`/`ClientGrant`/`Unbound` test patterns (lines 265–291, 548–565, 891–911) to mirror for the new gate tests |

---

## Related Documentation

- [`deny-by-default-bindable-callers.md`](./deny-by-default-bindable-callers.md) — `Unbound`/`ClientGrant`/Tier ordering this doc builds on
- [`per-device-machine-header.md`](./per-device-machine-header.md) — sibling identity-signal mechanism (`X-Mcpmux-Machine-Id`), same resolver layer
- [`workspace-machine-binding.md`](./workspace-machine-binding.md) — machine catalog and binding model
- [`workspace-binding-popup-loop-fix.md`](./workspace-binding-popup-loop-fix.md) — already reconciled (Status: Implemented, Jul 23, 2026); no action needed here

---

## Related Ops Cleanup (from `mux-remote-cloud-agent-access` session — action items, not part of this doc's phases)

- jsg-tech-check `docs/planning/mcp-cloud-agents-remote-gateway-plan.md` — still describes Space-lock; reality was rootless-default Bundle + the key-rename fix. Untracked, unreconciled.
- jsg-tech-check session thread `~/.cursor/sessions/jsg-tech-check/threads/mux-remote-cloud-agent-access.md` — never written.
- Mobile Cloud Agent MCP re-test — no evidence done.
- Stale client revoke (`mcp_3fff6e8b`, `mcp_a0d804c2`) — no evidence done.
