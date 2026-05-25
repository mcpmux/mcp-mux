# Gateway Warm Pool Startup

**Last Updated:** May 25, 2026  
**Status:** Planned — decisions locked, not started  
**Branch:** TBD (suggest `feat/gateway-warm-pool` off `main`)  
**Base branch:** `main`  
**Issue:** TBD — file after planning review  
**Depends on:** [`meta-gateway-invoke.md`](./meta-gateway-invoke.md) (agent meta-tool entrypoint — warm pool makes invoke/search reliable at session start)  
**Parent initiative:** [`agent-mcp-session-readiness.md`](./agent-mcp-session-readiness.md) (umbrella — all four agent entrypoint pain points)  
**Unblocks:** Agent-first McpMux sessions without multi-minute CONNECTING stalls; post-restart binding/roots race where `total_invokable: 0` until backends connect

---

## Problem

Gateway auto-start is fast for HTTP (`Ready to accept connections` immediately), but **backend MCP pool warm-up is slow and binding-blind**. Agents hit empty search/invoke results while servers spin up.

Observed in May 2026 live QA (meta-gateway invoke retest):

| Symptom | Cause |
| ------- | ----- |
| All enabled servers show **CONNECTING** at once | Batch `set_connecting` pre-loop in `StartupOrchestrator` |
| github unavailable for minutes after dev restart | Sequential connect through full enabled fleet |
| `total_invokable: 0`, `server_available: false` on all tools | `mark_all_features_unavailable` until each server completes handshake + discovery |
| Binding-resolved FS ready before github connects | Startup connects **all** DB-`enabled` servers, not binding-priority |
| Agent session roots arrive after first meta-tool calls | No runtime warm on roots/binding change |

Current startup path (`GatewayServer::run_with_shutdown` → `StartupOrchestrator::auto_connect_enabled_servers`):

1. Mark all features unavailable  
2. Resolve prefixes (all spaces)  
3. **Sequential** connect every `InstalledServer` where `enabled == true` (all spaces)  
4. Pre-set **all** enabled servers to CONNECTING before any handshake completes  

`ARCHITECTURE.md` documents pool-based **eager** connect (intentional — routing requires a pool instance). It also claims **parallel** startup connects; implementation is **sequential**. Bindings / FeatureSets affect routing only — not startup selection.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Startup connect scope | **Tiered** — binding-referenced servers first (parallel), then remaining DB-enabled in background | Agent-visible servers ready quickly without abandoning eager pool model for power users |
| 2 | Hot-set derivation | **Union of server_ids from ALL workspace bindings** (any space) | Matches multi-workspace installs; any bound folder's servers get priority |
| 3 | Unbound enabled servers | **No deferral** — still connect at boot (background tier after hot) | Preserves current "I enabled it, it should connect" desktop expectation |
| 4 | Parallelism | **Single semaphore, 6 concurrent** (stdio + HTTP mixed) | Balance wall-clock vs CPU/IO spike on homelab installs |
| 5 | Runtime warm | **On `WorkspaceBindingChanged` + when MCP session roots first arrive** | Fixes post-restart agent race and binding edits without full gateway restart |
| 6 | Agent warming signal | **Meta tools** — expose pool warming state (not desktop-only) | Agents can distinguish ACL deny vs still-warming; retry search instead of misdiagnosing |
| 7 | v1 delivery | **Full scope** — tiered + parallel + runtime warm + meta signal + CONNECTING UX + unify connect paths + setting hook | Single initiative, phased for review |
| 8 | Lazy invoke-time connect | **Out of scope for v1** | ARCHITECTURE rejects lazy loading; routing requires pool instance today |
| 9 | Connect path unification | **Single orchestrator** — manual `connect_all_enabled_servers` delegates to `StartupOrchestrator` | Fixes duplicate logic + ServerManager status gap on manual start |

---

## The Model

### Tiered startup

```text
Gateway spawn
├─ HTTP listener up (immediate)
└─ background StartupOrchestrator
    ├─ mark_all_features_unavailable()
    ├─ resolve_server_prefixes()
    ├─ build_connect_plan()
    │   ├─ hot: enabled ∩ referenced_by_any_binding_FS
    │   └─ warm: enabled \ hot  (still connects — no defer)
    ├─ set_connecting(hot only)          ← UX fix: no fleet-wide CONNECTING burst
    ├─ connect_parallel(hot, max=6)
    ├─ set_connecting(warm batch)        ← optional: per-server as each starts
    └─ connect_parallel(warm, max=6)
```

### Hot-set resolution (new helper)

```text
for binding in binding_repo.list():
  for fs_id in binding.feature_set_ids:
    resolve FeatureSet members (recursive composition)
    collect server_id from each included tool feature
    if FeatureSetType::ServerAll → include fs.server_id
→ hot_server_keys: HashSet<(space_id, server_id)>
```

Intersect with `installed_servers.filter(|s| s.enabled)`.

### Runtime warm triggers

| Event | Action |
| ----- | ------ |
| `WorkspaceBindingChanged` | Compute newly referenced server keys; connect enabled missing instances (same `connect_server` path, respect semaphore) |
| MCP session roots first stored (`SessionRootsRegistry`) | Resolve binding for roots; connect hot set for that binding if not connected |
| User `enable_server_v2` | Unchanged — immediate connect for that server |

### Agent warming signal (meta tools)

Extend `mcpmux_list_servers` response (or add top-level fields on read meta tools) with:

```json
{
  "gateway_warming": true,
  "pool": {
    "connected": 2,
    "connecting": 4,
    "expected_enabled": 12,
    "hot_pending": 1
  }
}
```

`gateway_warming: true` when any **hot** server is still connecting or any enabled server referenced by current session's resolved binding is not `server_available`. Agents should retry `search_tools` after warming clears.

---

## Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│  GatewayServer::run_with_shutdown                               │
│    tokio::spawn → StartupOrchestrator                           │
└────────────────────────────┬────────────────────────────────────┘
                             │
         ┌───────────────────┼───────────────────┐
         ▼                   ▼                   ▼
  ConnectPlanBuilder   ParallelConnectPool   ServerManager
  (bindings + FS)        (Semaphore=6)         (set_connecting per tier)
         │                   │
         └─────────┬─────────┘
                   ▼
            PoolService::connect_server
                   │
                   ▼
         FeatureService::discover_and_cache
                   │
                   ▼
         DomainEvent → MCP list_changed

Runtime hooks:
  MCPNotifier / handler on WorkspaceBindingChanged → WarmPoolService::connect_missing
  handler on roots stored → WarmPoolService::connect_for_binding
```

**Rejected for v1:** Skip boot connect for unbound enabled servers (user chose no defer). Invoke-time lazy connect (architecture conflict).

---

## Files to create

| File | Purpose |
| ---- | ------- |
| `crates/mcpmux-gateway/src/server/connect_plan.rs` | `ConnectPlanBuilder` — hot/warm tiers from bindings + enabled installs |
| `crates/mcpmux-gateway/src/server/parallel_connect.rs` | Semaphore-limited parallel connect runner shared by startup + runtime warm |
| `crates/mcpmux-gateway/src/services/warm_pool.rs` | Runtime warm on binding/roots; exposes `PoolWarmthSnapshot` for meta tools |
| `tests/rust/tests/integration/gateway_warm_pool.rs` | Hot tier ordering, parallel connect mock, warmth snapshot, binding-change warm |

## Files to modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-gateway/src/server/startup.rs`](../../crates/mcpmux-gateway/src/server/startup.rs) | Replace sequential loop with tiered parallel connect; narrow CONNECTING pre-set |
| [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) | Wire `WarmPoolService`; pass to MCP handler / orchestrator |
| [`crates/mcpmux-gateway/src/server/service_container.rs`](../../crates/mcpmux-gateway/src/server/service_container.rs) | DI for connect plan + warm pool services |
| [`crates/mcpmux-gateway/ARCHITECTURE.md`](../../crates/mcpmux-gateway/ARCHITECTURE.md) | Document tiered eager connect; fix parallel claim |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Fix stale comment ("frontend auto-connect") |
| [`apps/desktop/src-tauri/src/commands/gateway.rs`](../../apps/desktop/src-tauri/src/commands/gateway.rs) | `connect_all_enabled_servers` → delegate to orchestrator |
| [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) | `list_servers` warmth fields; optional hint when warming |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | Trigger runtime warm when roots stored / after binding resolve |
| [`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) | Call warm pool on `WorkspaceBindingChanged` |
| [`crates/mcpmux-gateway/src/pool/server_manager.rs`](../../crates/mcpmux-gateway/src/pool/server_manager.rs) | Delete or wire dead `startup_refresh` / document replacement |
| [`apps/desktop/src/features/settings/SettingsPage.tsx`](../../apps/desktop/src/features/settings/SettingsPage.tsx) | (Phase 3) Optional setting: startup concurrency 4/6/8 |

---

## Phasing

### Phase 1 — Tiered plan + parallel startup

**Effort:** ~1–2 days

- [ ] `ConnectPlanBuilder`: load all bindings → union server_ids from FS members (+ ServerAll) → split hot/warm enabled installs
- [ ] `ParallelConnectPool`: `FuturesUnordered` + `Semaphore(6)`; reuse existing `StartupOrchestrator::connect_server`
- [ ] Replace sequential loop in `auto_connect_enabled_servers` with hot-then-warm parallel passes
- [ ] CONNECTING UX: pre-set **hot tier only** (not entire fleet)
- [ ] Unit tests: plan builder with nested FS composition, ServerAll, empty bindings → warm-only
- [ ] Integration test: hot servers complete before warm when hot is mocked slow/fast

**Outcome:** Dev restart connects binding-referenced servers (e.g. github) within seconds even when 10+ other enabled servers exist. UI shows CONNECTING for hot tier first, not 30 servers at once.

### Phase 2 — Runtime warm + agent signal

**Effort:** ~1–2 days

- [ ] `WarmPoolService::connect_missing(keys)` — idempotent, shares semaphore with startup
- [ ] Hook: `WorkspaceBindingChanged` → compute new hot keys → connect missing
- [ ] Hook: session roots first stored → resolve binding → connect missing hot keys
- [ ] `PoolWarmthSnapshot` + expose via `mcpmux_list_servers` (`gateway_warming`, counts)
- [ ] Meta tool description updates steering agents to retry search when warming
- [ ] Integration test: roots arrive after gateway boot → github becomes available without manual enable

**Outcome:** Cursor agent session after dev restart gets invokable tools once roots land, without user running `mcpmux_enable_server`. Meta tools report warming vs ready.

### Phase 3 — Unify paths, settings, cleanup

**Effort:** ~0.5–1 day

- [ ] `connect_all_enabled_servers` Tauri command delegates to `StartupOrchestrator` (ServerManager events consistent)
- [ ] Optional setting `gateway.startup_concurrency` (4 / 6 / 8), default 6
- [ ] Remove or wire dead code: `PoolService::reconnect_all_enabled`, unused `startup_refresh`
- [ ] Update `ARCHITECTURE.md` + `lib.rs` comment
- [ ] Manual QA: full dev restart with QA FeatureSet binding — time-to-invokable for github < 15s with 8+ enabled servers

**Outcome:** One connect code path; docs match behavior; power users can tune concurrency. Planning doc reconciliation updated.

---

## Pre-PR validation

| Step | Command | Purpose |
| ---- | ------- | ------- |
| Rust unit + int | `cargo nextest run -p tests -- gateway_warm_pool` + workspace lib tests | Connect plan + warm hooks |
| Full validate | `pnpm validate` | fmt, clippy, check |
| Manual smoke | Dev restart → `mcpmux_list_servers` → `mcpmux_search_tools` until `gateway_warming: false` | Agent UX |

---

## Out of scope

| Item | Reason |
| ---- | ------ |
| Lazy connect on `invoke_tool` failure | Conflicts with pool-based routing; separate initiative if ever needed |
| Defer unbound enabled servers at boot | Explicit decision: no defer — all enabled still connect (background tier) |
| OAuth token prefetch | Deprecated no-op; RMCP refreshes per-request |
| Connect only servers in **current** session binding at boot | Boot has no session; use hot union of all bindings instead |
| Faster stdio package install (uvx/npm cache) | Environment concern, not gateway orchestration |

---

## Key files referenced

| File | Why |
| ---- | --- |
| [`crates/mcpmux-gateway/src/server/startup.rs`](../../crates/mcpmux-gateway/src/server/startup.rs) | Current sequential auto-connect + CONNECTING batch |
| [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) | Gateway spawn + background task ordering |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Binding resolution at MCP request time (roots) |
| [`crates/mcpmux-gateway/src/pool/features/resolution.rs`](../../crates/mcpmux-gateway/src/pool/features/resolution.rs) | FS → feature/server resolution (reuse for hot set) |
| [`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) | `WorkspaceBindingChanged` consumer today |
| [`crates/mcpmux-gateway/ARCHITECTURE.md`](../../crates/mcpmux-gateway/ARCHITECTURE.md) | Eager pool rationale — must stay compatible |
| [`docs/planning/meta-gateway-invoke-retest.md`](./meta-gateway-invoke-retest.md) | Live QA evidence for warm-pool pain |

---

## Related documentation

- [`docs/planning/meta-gateway-invoke.md`](./meta-gateway-invoke.md) — agent meta-tool model (primary consumer of warm pool)
- [`docs/planning/meta-gateway-invoke-retest.md`](./meta-gateway-invoke-retest.md) — post-85113e7 QA sign-off
- [`docs/planning/dynamic-mcp-toggle-meta-tools.md`](./dynamic-mcp-toggle-meta-tools.md) — session enable semantics unchanged

---

## Reconciliation

Update this doc's **Status** and phase checkboxes as work lands. When implementation completes, add a short entry to [`meta-gateway-invoke.md`](./meta-gateway-invoke.md) **Related documentation** linking here. Run planning-doc reconciliation (status, file inventory, phase outcomes) before marking **Status: Complete**.

**Decision record (May 25, 2026):** Tiered hot/warm startup with binding-union hot set; flat concurrency 6; no defer of unbound enabled; runtime warm on binding + roots; agent warming via meta tools; full three-phase delivery.
