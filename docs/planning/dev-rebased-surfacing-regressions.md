# dev-rebased Surfacing Regression Fix

**Last Updated:** Jun 24, 2026
**Status:** Phases 1–3 complete — Phase 4 handoff in progress (post-port verification pass done)
**Branch:** `dev-rebased` (HEAD: `6c4d6b7`)
**Depends on:** Working tree clean at `d2307d9` (docs planning commit)
**Unblocks:** Gateway surfaces ~5 meta tools instead of 2228; hard-cut model restored; data-integrity regressions resolved; phase hand-off to `dev-rebased-post-port-completion.md`

### Phase status

| Phase | Status | Commit |
| ----- | ------ | ------ |
| 1 — Surfacing: list_* paths | ✅ Complete | `126fa2f` (list_tools pre-fix in `93e6bef`) |
| 2 — Surfacing: call_* hard-cut guards | ✅ Complete | `b131a3f` |
| 3 — Data integrity regressions | ✅ Complete | `6c4d6b7` |
| 4 — Hand off to post-port completion | 🔄 In progress | See audit below |

### Phase 6 verification (Jun 24, 2026)

Automated gates on `6c4d6b7`:

| Gate | Result |
| ---- | ------ |
| `pnpm validate` | ✅ pass (fmt, clippy, check, eslint, typecheck) |
| `pnpm test:rust:unit` | ✅ 435 passed, 2 skipped |
| `pnpm test:ts` | ✅ 334 passed |

Surfacing code inspection:

| Check | Result |
| ----- | ------ |
| `CORE_META_TOOLS` = 5 entries | ✅ `mod.rs` lines 82–88; asserted in `token_budget.rs` + `registry_advertises_core_tools_read_only_in_list` int test |
| `list_as_tools()` filters to core only | ✅ `registry.rs:229` filters via `CORE_META_TOOLS.contains` |
| `get_advertised_*` wired in handler | ✅ tools L746, prompts L1041/L1101, resources L1189/L1257 |
| `call_tool` hard-cut guards | ✅ L844–937: advertised vs invokable check, `use_invoke_tool` / `bind_feature_set` errors, `list_inactive_discovery_tools` lookup |
| `structured_content` passthrough | ✅ L1011 |

---

## Problem

A full diff audit between `dev` (reference) and `dev-rebased` HEAD surfaced 7 confirmed regressions introduced during the rebase. The branches share no merge base, so the comparison is file-level. One regression (`list_tools` exposing all resolved tools instead of surfaced ones) was found and patched before the audit; 6 more were found during it.

**1. Surfacing model completely stripped.** `dev` uses `get_advertised_*` methods on `list_tools`, `list_prompts`, `list_resources` — these filter to only features in the `surfaced` flag set. `dev-rebased` replaced all three with unfiltered `get_*_for_grants` calls. `get_advertised_prompts_for_grants` and `get_advertised_resources_for_grants` don't even exist in `facade.rs` anymore.

**2. Hard-cut guards gone from call_* paths.** `dev` rejects direct calls to non-surfaced tools with a `use_invoke_tool` / `bind_feature_set` redirect hint and a `list_inactive_discovery_tools` lookup. `dev-rebased` dropped the entire block — any invokable tool can be called directly, bypassing the meta-tool-first UX intent. Same pattern applied to `get_prompt` (lost `format_direct_fetch_prompt_redirect`) and `read_resource` (lost surfaced-only gate).

**3. `structured_content` dropped from `call_tool` result.** `dev` passes `result.structured_content` through to the client. `dev-rebased` removed the assignment. MCP clients expecting typed structured output silently get nothing.

**4. `WorkspaceNeedsBinding` event field broken.** Backend changed the event shape from `collision_client_id: Option<String>` to `space_locked: bool`. The frontend (`WorkspaceBindingSheet.tsx`, `useWorkspaceEvents.ts`) still keys collision badge copy off `collision_client_id` — that entire UX path is silently dead.

**5. Credential key migration removed.** `crates/mcpmux-storage/src/key_migration.rs` and its startup call in `apps/desktop/src-tauri/src/state/mod.rs` were deleted. Users with OS-keychain-dismiss-fallback encrypted credentials lose access after a keychain recovery.

**6. OAuth refresh dedup singleton removed.** `gateway.ts` inlined the token refresh logic and dropped the module-level singleton promise that prevented duplicate startup refresh calls when `useDataSync` triggers more than once.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Surfacing model for list_* | Restore full `dev` model: `get_advertised_*` on all three list paths | Keeps `tools/list` lean (~5 meta + surfaced only); full set reachable via `mcpmux_invoke_tool`. Consistent with the product intent of FeatureSet surfacing. |
| 2 | Hard-cut guard model | Restore `dev`'s call_tool / get_prompt / read_resource hard-cut with redirect hints | "If it's not in your list, use the meta-tool path" enforces the UX contract. Guards are callable via `is_meta_tool()` which already exists. |
| 3 | WorkspaceNeedsBinding event | Keep both `collision_client_id` AND `space_locked` fields | `space_locked` encodes the new scoped-Space behavior; `collision_client_id` drives existing frontend collision UX. Both are independent semantics. |
| 4 | structured_content | Restore passthrough | MCP 2025-11-05+ clients expect structured output. Removing it breaks a spec-defined field. |
| 5 | Credential key migration | Restore both the file and the startup call | No data on how many users hit the fallback path; safe to assume some did. Migration is idempotent. |
| 6 | OAuth refresh dedup | Restore singleton promise guard | `useDataSync` fires on mount and on reconnect. Duplicate refreshes are wasted work and could race the token store. |

---

## Scope

**In:**
- Restore `get_advertised_prompts_for_grants` and `get_advertised_resources_for_grants` in `facade.rs`
- Wire `list_prompts`, `get_prompt`, `list_resources`, `read_resource` to use advertised methods
- Restore `call_tool` hard-cut guard + `list_inactive_discovery_tools` redirect flow
- Restore `get_prompt` and `read_resource` hard-cut redirect helpers
- Restore `structured_content` passthrough on `call_tool`
- Restore `collision_client_id` field on `WorkspaceNeedsBinding` domain event
- Update frontend to render collision messaging using the restored field
- Restore `key_migration.rs` + startup call
- Restore OAuth refresh singleton in `gateway.ts`
- Commit the already-applied `list_tools` fix
- `pnpm validate` clean after each phase

**Out:**

| Item | Reason |
| ---- | ------ |
| dev delta cherry-picks (10 commits) | Tracked in [`dev-rebased-post-port-completion.md`](./dev-rebased-post-port-completion.md) Phase 1 — begin after this doc's phases are complete |
| lib/api invoke → apiCall migration | Same — tracked in `dev-rebased-post-port-completion.md` Phase 2 |
| Feature-by-feature verification pass | Same — tracked in `dev-rebased-post-port-completion.md` Phase 3 |
| `should_prompt` on `SpaceDefault` reversal | Intentional improvement on `dev-rebased` — not a regression |
| Roots-capability probe ordering fix | Intentional improvement on `dev-rebased` — not a regression |
| New meta-tools (diagnose_view, etc.) | Not regressed — present on both branches |

---

## Architecture

### Surfacing model (restored)

```
tools/list, list_prompts, list_resources
  └─ get_advertised_*_for_grants()
       ├─ get_invokable_*_for_grants()   ← full grant-resolved set (invoke ACL)
       └─ resolve_surfaced_feature_ids() ← surfaced flag filter
            → intersection: only surfaced features hit the wire

call_tool / get_prompt / read_resource
  ├─ if name starts with mcpmux_ → meta tool dispatch (no filter)
  ├─ if name in advertised set    → route to backend (existing behavior)
  ├─ if name in invokable set     → hard-cut: return use_invoke_tool redirect hint
  └─ else                         → tool not found
```

The `list_inactive_discovery_tools` call in the hard-cut path lets the redirect hint name the bundle that contains the tool, so the agent can bind it if needed.

### WorkspaceNeedsBinding event shape (restored)

```rust
WorkspaceNeedsBinding {
    client_id: String,
    session_id: String,
    space_id: Uuid,
    workspace_root: String,
    collision_client_id: Option<String>,  // restored — existing client already bound this root
    space_locked: bool,                    // new — Space picker should be locked in binding sheet
}
```

---

## Phases

### Phase 1 — Surfacing: list_* paths (~2 hours)

- Commit the already-applied `list_tools` → `get_advertised_tools_for_grants` fix
- Add `get_advertised_prompts_for_grants` to `facade.rs` (mirror the tools method: `get_prompts_for_grants` + `resolve_surfaced_feature_ids` filter)
- Add `get_advertised_resources_for_grants` to `facade.rs` (same pattern)
- Wire `list_prompts` (~941) to `get_advertised_prompts_for_grants`
- Wire `get_prompt` (~998) to `get_advertised_prompts_for_grants` for the allow-list check
- Wire `list_resources` (~1055) to `get_advertised_resources_for_grants`
- Wire `read_resource` (~1109) to `get_advertised_resources_for_grants` for the allow-list check
- `cargo check -p mcpmux-gateway` clean

**Outcome:** `tools/list`, `prompts/list`, and `resources/list` all return only surfaced features. Cursor shows ~5 meta tools. A fresh MCP session connecting to the gateway no longer dumps thousands of entries.

---

### Phase 2 — Surfacing: call_* hard-cut guards (~3 hours)

Restore the hard-cut logic that was in `dev`'s `call_tool`, `get_prompt`, and `read_resource` handlers. The call_tool guard structure on `dev` (lines ~773–865) is the canonical reference.

- Restore `call_tool` hard-cut block:
  - after advertised-set hit → route normally (unchanged)
  - if in invokable but not surfaced → `list_inactive_discovery_tools` lookup, return `use_invoke_tool` redirect hint with `bindable_feature_set_id`
  - else → tool not found error
- Restore `structured_content` assignment on `call_tool` result (`result.structured_content = tool_result.structured_content`)
- Restore `get_prompt` hard-cut: if prompt not in advertised set but in invokable → `format_direct_fetch_prompt_redirect` response
- Restore `read_resource` hard-cut: if resource not in advertised set but readable → `format_direct_read_resource_redirect` response (or equivalent)
- Verify `routing.rs` still has the `format_direct_*_redirect` helpers (subagent noted they're orphaned but still present)
- `cargo check -p mcpmux-gateway` clean; `pnpm test:rust:unit` passes

**Outcome:** Calling a non-surfaced tool directly returns a structured hint pointing to `mcpmux_invoke_tool` and the bindable FeatureSet. The hard-cut is testable by invoking a tool from a connected server that isn't in the active FeatureSet's surfaced list. `structured_content` passes through correctly on tools that return it.

---

### Phase 3 — Data integrity regressions (~1.5 hours)

Three changes that could silently corrupt state or break user data:

**Credential key migration:**
- Restore `crates/mcpmux-storage/src/key_migration.rs` from `dev` (`git show dev:crates/mcpmux-storage/src/key_migration.rs`)
- Restore the `migrate_file_key_encrypted_fields` startup call in `apps/desktop/src-tauri/src/state/mod.rs` at its original location (~line 90)
- Confirm migration is idempotent (no double-migration risk on a fresh install)

**WorkspaceNeedsBinding event:**
- Add `collision_client_id: Option<String>` back to `DomainEvent::WorkspaceNeedsBinding` in `crates/mcpmux-core/src/domain/event.rs`
- Restore population of `collision_client_id` in `handler.rs` at the `emit_domain_event` call site — requires looking up whether any existing session for this space already holds the root
- Update `WorkspaceBindingSheet.tsx` to render collision badge when `collision_client_id` is `Some` (existing logic, just re-wire)
- Update `useWorkspaceEvents.ts` to pass both `collision_client_id` and `space_locked` through to the sheet

**OAuth refresh dedup:**
- Restore module-level singleton promise in `apps/desktop/src/lib/api/gateway.ts` that coalesces concurrent `refreshOAuthTokensOnStartup` calls to a single in-flight promise

**Outcome:** Tauri `cargo check` and `pnpm typecheck` both pass. A simulated double-mount of `useDataSync` produces one OAuth refresh call in the network tab. Opening the app after a keychain recovery does not permanently lose credential access for users who previously hit the OS-keychain-dismiss fallback path.

---

### Phase 4 — Hand off to post-port completion doc (~ongoing)

After Phases 1–3 are confirmed working (desktop Tauri app loads, gateway serves ~5 meta tools, `pnpm validate` clean), pick up the remaining port work from the existing plan:

| Post-port phase | Status | Commit / notes |
| --------------- | ------ | -------------- |
| [`dev-rebased-post-port-completion.md`](./dev-rebased-post-port-completion.md) **Phase 1** — audit + cherry-pick 10 `dev`-only commits | ✅ Complete | `784cd41` |
| **Phase 2** — `lib/api` `invoke` → `apiCall` migration (12 remaining files) | ✅ Complete | `9747c71`; grep confirms zero raw `invoke()` in `apps/desktop/src/lib/api/` |
| **Phase 3** — feature-by-feature verification | 🔄 Automated pass done; manual QA open | Phase 6 verification pass (Jun 24); see manual checklist below |

**Manual QA still required** (cannot automate in CI — from post-port Phase 3 checklist):

- Dashboard: stat cards, health section, activity feed, quick links, gateway status bar
- i18n: nav labels, renamed superapp vocab (`myServers`, `search`, `bundles`, `projects`, `clients`)
- Spaces: CRUD, base dirs, switcher accent, panel counts
- Servers: install/enable/auth/logs/clone/display name/source badge/update policy badges and notify/auto/pinned modes
- Feature Sets: CRUD, tool add/remove, surfaced toggle, starter protection
- Workspaces: folder→bundle binding, appearances, per-client scope
- Clients: preset list, OAuth grant, access key copy, Connect IDE flow
- Registry/Discover: catalog browse, install, search/filters
- Builtin Servers: enable/disable per space, gateway tool list
- Settings: gateway port, build stamp, pending updates, stale build banner, analytics toggle
- Meta-tools (MCP client): bare-name invoke/schema, search synonyms + inactive preview, prefilled_params, display_name on deny, approval dialog, token budget
- Web admin: SSE `:45819/events`, CF Access JWT on local dev, SPA 404 fallback
- Surfacing smoke: fresh MCP session shows ~5 meta tools (not thousands); direct call to non-surfaced tool returns `use_invoke_tool` hint

**Outcome:** `dev-rebased` reaches full feature parity with `dev` tip. Web admin loads cleanly. All verification items in the post-port completion doc are checked off.

---

## Files to create / modify

| Phase | File | Action |
| ----- | ---- | ------ |
| 1 | `crates/mcpmux-gateway/src/mcp/handler.rs` | Commit list_tools fix; wire list_prompts, get_prompt, list_resources, read_resource to advertised methods |
| 1 | `crates/mcpmux-gateway/src/pool/features/facade.rs` | Add `get_advertised_prompts_for_grants`, `get_advertised_resources_for_grants` |
| 2 | `crates/mcpmux-gateway/src/mcp/handler.rs` | Restore call_tool hard-cut block, structured_content, get_prompt redirect, read_resource redirect |
| 2 | `crates/mcpmux-gateway/src/pool/routing.rs` | Verify format_direct_fetch_prompt_redirect / format_direct_read_resource_redirect present; restore if missing |
| 3 | `crates/mcpmux-storage/src/key_migration.rs` | Restore from `dev` (deleted on dev-rebased) |
| 3 | `crates/mcpmux-storage/src/lib.rs` | Re-export `key_migration` module |
| 3 | `apps/desktop/src-tauri/src/state/mod.rs` | Restore `migrate_file_key_encrypted_fields` startup call |
| 3 | `crates/mcpmux-core/src/domain/event.rs` | Add `collision_client_id: Option<String>` back to `WorkspaceNeedsBinding` |
| 3 | `crates/mcpmux-gateway/src/mcp/handler.rs` | Restore `collision_client_id` population at `emit_domain_event` site |
| 3 | `apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx` | Re-wire collision badge to `collision_client_id` |
| 3 | `apps/desktop/src/hooks/useWorkspaceEvents.ts` | Pass both `collision_client_id` and `space_locked` to sheet |
| 3 | `apps/desktop/src/lib/api/gateway.ts` | Restore singleton dedup promise for `refreshOAuthTokensOnStartup` |

---

## Key files referenced

| File | Note |
| ---- | ---- |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | Primary regression surface — all list/call/fetch MCP paths |
| [`crates/mcpmux-gateway/src/pool/features/facade.rs`](../../crates/mcpmux-gateway/src/pool/features/facade.rs) | Missing `get_advertised_prompts/resources_for_grants`; `get_advertised_tools_for_grants` already exists here |
| [`crates/mcpmux-gateway/src/pool/routing.rs`](../../crates/mcpmux-gateway/src/pool/routing.rs) | `format_direct_*_redirect` helpers — orphaned but still present; needed by Phase 2 |
| [`crates/mcpmux-core/src/domain/event.rs`](../../crates/mcpmux-core/src/domain/event.rs) | `WorkspaceNeedsBinding` shape — missing `collision_client_id` |
| [`apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx`](../../apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx) | Collision badge/copy wired to `collision_client_id` (~209–215) |
| [`apps/desktop/src/hooks/useWorkspaceEvents.ts`](../../apps/desktop/src/hooks/useWorkspaceEvents.ts) | Event handler that needs to forward both event fields (~36) |
| [`apps/desktop/src-tauri/src/state/mod.rs`](../../apps/desktop/src-tauri/src/state/mod.rs) | Startup init — migration call goes at ~line 90 |
| [`apps/desktop/src/lib/api/gateway.ts`](../../apps/desktop/src/lib/api/gateway.ts) | OAuth refresh dedup singleton — currently inlined without coalescing guard |

---

## Related documentation

- [`docs/planning/dev-rebased-post-port-completion.md`](./dev-rebased-post-port-completion.md) — Phase 4 of this doc hands off to Phases 1–3 of this doc
- [`docs/planning/dev-to-main-port.md`](./dev-to-main-port.md) — the 8-phase port that produced the dev-rebased branch
- [`docs/planning/web-admin-completion.md`](./web-admin-completion.md) — web admin gaps; unblocked after lib/api migration (post-port Phase 2)
