# Workspace Binding Popup Loop Fix

**Last Updated:** Jul 23, 2026
**Status:** Implemented (Jul 23, 2026)
**Depends on:** `workspace-machine-binding.md` (Tier 1 machine-scoped lookup), `per-device-machine-header.md` (`request_machine_id` signal)
**Unblocks:** Reliable machine-scoped resolution for native Cursor MCP clients since Cloud Agents were connected

---

## Problem

Since connecting Cursor Cloud Agents, the "NEW WORKSPACE DETECTED" binding panel kept popping up for the `jsg-tech-check` workspace regardless of dismissal, and re-saving the binding did not fix it.

Traced via DB + gateway log inspection:

- The client is `mcp_36740f70` — Cursor's native "Cursor" MCP client (OAuth/DCR, registered May 16, 2026, `redirect_uri: cursor://anysphere.cursor-mcp/oauth/callback`), reconnecting with a fresh session roughly every 5 minutes.
- Every reconnect reported `roots=["/Users/joe/Desktop/Repos/Personal/jsg-tech-check"]` and resolved `source=Unbound`.
- A `workspace_bindings` row already existed for that exact path scoped to `machine_id=Gondor` (`ec211deb-4fa7-489f-ba15-35f4577d7e71`) with 2 FeatureSets attached, and `mcp_36740f70`'s own `inbound_clients.machine_id` is also Gondor.

Two compounding issues turned one bug into a permanent nuisance:

1. **No dedup on `WorkspaceNeedsBinding`.** Every Unbound-resolving reconnect refired the event (`log_and_notify_resolution` in [`handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs)), and [`bindingPanelStore.ts`](../../apps/desktop/src/stores/bindingPanelStore.ts) had no dismiss memory.
2. **"Save binding" re-targeted a binding that already existed** for `(Gondor, jsg-tech-check)`. It could not fix a read-side mismatch.

Separately (unrelated root cause, same investigation): `mcp_36740f70` had `approved=0` in `inbound_clients`, and [`live_runtime.rs`](../../crates/mcpmux-gateway/src/admin/live_runtime.rs) `get_oauth_clients` filters the Connections list on `approved` — so this client was invisible in the UI.

---

## Root cause (actual)

Not a no-header Tier 1 miss. Live repro found a **stale / wrong `X-Mcpmux-Machine-Id`** on the native Cursor OAuth session (likely inherited from Cloud Agents config). The header branch found no binding for that machine, then `continue`d to the next root — skipping the client's registered Gondor machine and the existing Gondor binding.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Root-cause method | Temporary debug logging + live repro against `mcp_36740f70` reconnect | Static reading of the no-header path looked correct; mismatch only showed up with real request-time header values. |
| 2 | Resolver fix | When header machine ≠ OAuth client's registered machine, treat header as stale and fall through to client → local → global | Preserves tunnel isolation when header matches registered machine or caller is anonymous. |
| 3 | Popup suppression | Persist dismissals per `(client_id, workspace_root)` in SQLite | Survives gateway restarts; clear on binding create/update for that root. |
| 4 | `approved` backfill | One-time `UPDATE ... WHERE approved = 0 AND last_seen IS NOT NULL` | Real access was already granted; flag never caught up. Legacy auto-approve TODO left out of scope. |

---

## Scope

**In (shipped):**
- Tier 1 stale-header fallthrough + regression tests
- `workspace_binding_prompt_dismissals` table + gateway emit skip + Tauri/FE wiring
- Clearing dismissals when a binding is saved for that workspace root
- Backfill migration for stale `approved` flags

**Out (unchanged):**

| Item | Reason |
| ---- | ------ |
| Removing the `approved` filter from `get_oauth_clients` | Backfill fixes the symptom; filter is correct for genuinely-unapproved clients. |
| Fixing the legacy auto-approve TODO in `handlers.rs` `oauth_authorize` | Separate pre-existing gap. |
| Unifying `client_id` and `machine_id` scoping axes | Tracked in `workspace-machine-binding.md` Future TODOs. |

---

## What shipped

| Commit | Phase | Summary |
| ------ | ----- | ------- |
| `4001253` | 1 | Stale-header fallthrough in `find_binding_for_roots`; storage + integration regression tests; temp instrumentation removed |
| `8d752fb` | 2 | Migration 041 dismissals; repo CRUD; gateway skip; Tauri dismiss commands; panel close + WorkspacesPage auto-open wiring; clear on create/update |
| `b1680a5` | 3 | Migration 042 approved backfill |

### Autonomous decisions during implement

- Dismissal CRUD lives on `InboundClientRepository` (no new repo type).
- WorkspacesPage auto-open uses **root-only** dismissal check when `client_id` is unknown (page-load catch-up without session context). Event-driven opens still pass `clientId` and persist per-client dismissals.
- `clear_binding_prompt_dismissals_for_root` on create/update clears all client rows for that root so a later regression re-prompts everyone.

---

## Files created

| File | Purpose |
| ---- | ------- |
| [`crates/mcpmux-storage/src/migrations/041_workspace_binding_prompt_dismissals.sql`](../../crates/mcpmux-storage/src/migrations/041_workspace_binding_prompt_dismissals.sql) | `workspace_binding_prompt_dismissals` table, PK `(client_id, workspace_root)` |
| [`crates/mcpmux-storage/src/migrations/042_backfill_approved_clients.sql`](../../crates/mcpmux-storage/src/migrations/042_backfill_approved_clients.sql) | Backfill stale `approved=0` clients with real traffic |

## Files modified

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Stale-header fallthrough in `find_binding_for_roots` |
| [`crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs`](../../crates/mcpmux-storage/src/repositories/workspace_binding_repository.rs) | Regression unit test (canonical machine + registered client shape) |
| [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) | `wrong_request_machine_header_falls_back_to_client_machine_binding` |
| [`crates/mcpmux-storage/src/database.rs`](../../crates/mcpmux-storage/src/database.rs) | Register migrations 041 + 042; fork test expects version 42 |
| [`crates/mcpmux-storage/src/repositories/inbound_client_repository.rs`](../../crates/mcpmux-storage/src/repositories/inbound_client_repository.rs) | Dismissal check/insert/clear methods |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | Skip `WorkspaceNeedsBinding` emit when dismissal exists |
| [`apps/desktop/src-tauri/src/commands/workspace_binding.rs`](../../apps/desktop/src-tauri/src/commands/workspace_binding.rs) | Dismiss / is-dismissed commands; clear on create/update |
| [`apps/desktop/src/lib/api/workspaceBindings.ts`](../../apps/desktop/src/lib/api/workspaceBindings.ts) | TS wrappers for dismiss commands |
| [`apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx`](../../apps/desktop/src/features/workspaces/workspace-binding-panel.component.tsx) | Persist dismissal on close for `create-from-live` with `clientId` |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | Root-only dismissal check before independent auto-open |

---

## Related Documentation

- [`workspace-machine-binding.md`](./workspace-machine-binding.md) — machine catalog, `find_exact_for_machine`
- [`per-device-machine-header.md`](./per-device-machine-header.md) — header priority + stale-header fallthrough exception (updated Jul 23, 2026)
