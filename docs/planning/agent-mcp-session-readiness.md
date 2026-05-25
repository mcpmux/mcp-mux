# Agent MCP Session Readiness

**Last Updated:** May 25, 2026  
**Status:** Planned — umbrella initiative; child doc [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md) has Phase 1 decisions locked  
**Branch:** TBD (multiple tracks; see phasing)  
**Base branch:** `main`  
**Issue:** TBD  
**Depends on:** [`meta-gateway-invoke.md`](./meta-gateway-invoke.md) (meta-tool entrypoint — must be reliable at session start)  
**Unblocks:** Using McpMux as primary agent MCP endpoint without multi-minute warm-up, empty ACL false alarms, or manual Surface reload guesswork

---

## Problem

Live QA on meta-gateway invoke (May 2026) exposed four friction points for **agent-first** McpMux use. Each has a different **control boundary** — some are fully fixable in-repo, some need client cooperation, one is intentional product design.

| # | Pain | Symptom | Control |
| - | ---- | ------- | ------- |
| 1 | **Cold start** | Dozens of backends `CONNECTING`; github unavailable for minutes after dev restart | **High** — gateway startup |
| 2 | **Binding / roots timing** | `total_invokable: 0` until workspace roots land; fragile across restart + Cursor reload | **Medium-high** — gateway + client timing |
| 3 | **Surface → reload** | Surfaced tools don't appear until Cursor → MCP → Reload tools | **Medium** — we emit `list_changed`; Cursor may ignore |
| 4 | **FeatureSet authoring** | QA FS missing GWorkspace; ACL curation is human/agent write work | **Medium** — reduce friction; cannot auto-grant all |

This doc is the **umbrella plan**: what to fix, what to accept, and **exactly where to dig** in the codebase for each controllable part. Implementation detail for cold start lives in [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md).

---

## Decisions (umbrella)

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Initiative structure | **Four tracks**, one umbrella doc; cold start is first ship | Tracks have different owners/risk; don't block warm pool on Cursor |
| 2 | Cold start | **Tiered hot/warm + parallel×6** — see child doc | Binding-union hot set; no defer of unbound enabled |
| 3 | Roots timing | **Runtime warm + meta `gateway_warming` signal** | Can't force Cursor to send roots earlier; can probe/warm/react |
| 4 | Surface reload | **Audit event path first**; doc + Cursor issue if `list_changed` already fires | Don't build workarounds until we confirm gateway gap vs client gap |
| 5 | FS authoring | **Improve agent + operator DX** — not auto-infer ACL from repo | Security model requires explicit grants |
| 6 | Lazy invoke connect | **Out of scope** | Pool-based routing requires connected instance ([`ARCHITECTURE.md`](../../crates/mcpmux-gateway/ARCHITECTURE.md)) |

---

## Control matrix

```text
                    McpMux fixes?    Client/user?        Track
Cold start              ████████░░        ██ (enable count)   A — warm pool
Roots timing            ███████░░░        ███ (roots timing)  A + B
Surface reload          █████░░░░░        █████ (Cursor UX)   C
FS authoring            ██████░░░░        ████ (ACL by design) D
```

---

## Track A — Cold start (warm pool)

**Control: ~80%** · **Child doc:** [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md)

### Symptom

Gateway HTTP is up immediately; backend pool takes minutes when many servers are DB-`enabled`. UI shows fleet-wide `CONNECTING`. Agents see `server_available: false` on all tools until sequential connect + feature discovery completes.

### Root cause (confirmed)

| Behavior | Where to dig |
| -------- | ------------ |
| Background auto-connect after HTTP bind | [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) — `run_with_shutdown()` ~L445–492 |
| Mark all features unavailable pre-connect | [`crates/mcpmux-gateway/src/server/startup.rs`](../../crates/mcpmux-gateway/src/server/startup.rs) — `mark_all_features_unavailable()` |
| **All** DB-enabled servers, **sequential** connect | [`crates/mcpmux-gateway/src/server/startup.rs`](../../crates/mcpmux-gateway/src/server/startup.rs) — `auto_connect_enabled_servers()` ~L128–204 |
| Batch pre-set CONNECTING for entire fleet | Same file ~L147–159 |
| Binding does **not** filter startup connect | Compare with [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) (routing only) |
| Eager pool rationale | [`crates/mcpmux-gateway/ARCHITECTURE.md`](../../crates/mcpmux-gateway/ARCHITECTURE.md) — Pool-Based Connection Management |
| Dead parallel scaffold (never called) | [`crates/mcpmux-gateway/src/pool/server_manager.rs`](../../crates/mcpmux-gateway/src/pool/server_manager.rs) — `startup_refresh()` |
| Duplicate manual connect path | [`apps/desktop/src-tauri/src/commands/gateway.rs`](../../apps/desktop/src-tauri/src/commands/gateway.rs) — `connect_all_enabled_servers` |
| Stale auto-start comment | [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) ~L510 (claims frontend auto-connect — wrong) |
| Tauri auto-start entry | [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) — setup spawn ~L357 |
| UI CONNECTING state | [`apps/desktop/src/hooks/useServerManager.ts`](../../apps/desktop/src/hooks/useServerManager.ts), [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) |

### Fix (locked)

Implement [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md) Phases 1–3: `ConnectPlanBuilder` (binding-union hot set), parallel connect (semaphore 6), hot-then-warm tiers, narrow CONNECTING pre-set.

### Outcome when done

Dev restart with QA binding: github `server_available: true` within ~15s even with 8+ other enabled servers. Fleet UI no longer flashes 30× CONNECTING simultaneously.

---

## Track B — Binding / roots timing

**Control: ~70%** · **Overlaps Track A Phase 2**

### Symptom

Agent MCP session calls meta tools before roots arrive → resolver returns `Deny` / empty FS → `total_invokable: 0`. After roots land, binding resolves but backends may still be cold (Track A).

### Root cause (confirmed)

| Behavior | Where to dig |
| -------- | ------------ |
| Four-tier resolver (binding / pending roots / grant / deny) | [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) — full file; read module doc ~L1–26 |
| Tier 1c: roots-capable, roots pending → empty FS | Same file ~L186–199 |
| Tier 1b: roots but no binding → deny | Same file ~L176–183 |
| Session roots storage | [`crates/mcpmux-gateway/src/services/session_roots.rs`](../../crates/mcpmux-gateway/src/services/session_roots.rs) — `set_roots_capable`, `get`, probe throttle |
| On-demand roots probe before list/call | [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) — search `on-demand probe` ~L193–315, ~L660 |
| `list_tools` vs `call_tool` roots probe parity | Same file — ensure meta-tool path uses same probe as `list_tools` |
| Integration tests for PendingRoots | [`tests/rust/tests/integration/feature_set_resolver.rs`](../../tests/rust/tests/integration/feature_set_resolver.rs) |
| Meta tools resolve via `caller_resolution()` | [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) — `caller_resolution()` |
| Workspace binding entity | [`crates/mcpmux-core/src/domain/workspace_binding.rs`](../../crates/mcpmux-core/src/domain/workspace_binding.rs) |
| Binding repo longest-prefix match | [`crates/mcpmux-core/src/repository/mod.rs`](../../crates/mcpmux-core/src/repository/mod.rs) — `find_longest_prefix_match` |

### Fix

1. **Track A Phase 2:** `WarmPoolService` on roots stored + `WorkspaceBindingChanged` ([`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) ~L454 — today only `notify_all_list_changed`, no connect)
2. **`gateway_warming` in meta tools** — [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) `list_servers` (see child doc)
3. **Audit:** Confirm every meta-tool `call()` path runs roots probe when `PendingRoots` (handler may already; verify for CallMcpTool session id)

### Not fixable in McpMux alone

- Cursor opening MCP connection before reporting `roots` — we can probe/retry, not force
- User opening wrong folder (no binding match) — `WorkspaceNeedsBinding` UX, not gateway bug

### Outcome when done

After dev restart + Cursor reload: first `mcpmux_search_tools` either returns tools or explicit `{ gateway_warming: true }` with retry hint — not silent empty ACL.

---

## Track C — Surface changes need MCP reload

**Control: ~50%** · **Audit-first track**

### Symptom

User toggles **Surface** on a FeatureSet member, saves, but Cursor agent still sees only 10 `mcpmux_*` tools until manual **Reload tools**. Direct `github_list_issues` may work via gateway while Cursor descriptor cache is stale.

### What we already do (dig here first)

| Step | Where to dig |
| ---- | ------------ |
| Surface toggle UI | [`apps/desktop/src/features/featuresets/FeatureSetPanel.tsx`](../../apps/desktop/src/features/featuresets/FeatureSetPanel.tsx) — `toggleSurfaced`, `handleSave` → `setFeatureSetMembers` with `surfaced:` |
| Bulk member save + `surfaced` column | [`apps/desktop/src-tauri/src/commands/feature_set.rs`](../../apps/desktop/src-tauri/src/commands/feature_set.rs) — `set_feature_set_members` ~L448–525 |
| DB migration for `surfaced` | [`crates/mcpmux-storage/src/migrations/019_feature_set_member_surfaced.sql`](../../crates/mcpmux-storage/src/migrations/019_feature_set_member_surfaced.sql) |
| Event after save | `grant_service.notify_feature_set_modified()` in `set_feature_set_members` ~L516–522 |
| Grant service → domain event | [`crates/mcpmux-gateway/src/services/grant_service.rs`](../../crates/mcpmux-gateway/src/services/grant_service.rs) — `notify_feature_set_modified` |
| MCPNotifier → `tools/list_changed` | [`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) — `FeatureSetMembersChanged` ~L419–429, `force=true` |
| Advertised vs invokable split | [`crates/mcpmux-gateway/src/pool/features/facade.rs`](../../crates/mcpmux-gateway/src/pool/features/facade.rs) — `get_advertised_tools_for_grants` |
| Surfaced resolution | [`crates/mcpmux-gateway/src/pool/features/resolution.rs`](../../crates/mcpmux-gateway/src/pool/features/resolution.rs) — `resolve_surfaced_feature_ids` |
| Direct call gate + redirect | [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) — `call_tool` surfaced check |
| Meta-tool create FS with surfaced | [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) — `CreateFeatureSetTool` |
| QA runbook reminder | [`docs/planning/meta-gateway-invoke-qa.md`](./meta-gateway-invoke-qa.md), [`meta-gateway-invoke-retest.md`](./meta-gateway-invoke-retest.md) §9 |

### Investigation tasks (Phase C1)

- [ ] Save Surface in UI → confirm `FeatureSetMembersChanged` in gateway logs
- [ ] Confirm MCPNotifier sends `notifications/tools/list_changed` to Cursor session (wire log or integration test with mock peer)
- [ ] If event fires but Cursor ignores: **client limitation** — file Cursor issue; keep runbook "Reload tools after Surface"
- [ ] If event does **not** fire: fix grant_service / gateway-not-running path in `set_feature_set_members`

### Possible McpMux improvements (Phase C2 — only if audit finds gaps)

- [ ] Per-session `notify_peer_lists_changed` for roots-capable peers on Surface change (today space-wide broadcast — see `ClientGrantChanged` pattern ~L432–446 for narrower fanout)
- [ ] Desktop toast after save: "Reload MCP tools in your client to pick up surfaced tools"
- [ ] Meta tool `mcpmux_list_servers` includes `surfaced_tools: ["github_list_issues"]` so agents know what *should* be direct-callable

### Not fixable without client change

Cursor (and some MCP clients) not applying `tools/list_changed` to local tool descriptor cache — manual reload remains fallback.

### Outcome when done

Documented truth table: "gateway emits X → Cursor does Y." Either a confirmed gateway bug fixed, or explicit "reload required" UX with no false "broken gateway" impression.

---

## Track D — FeatureSet authoring friction

**Control: ~60% (reduce friction; ACL stays explicit)**

### Symptom

Agent QA needed GWorkspace in FeatureSet but QA FS only had 3 github tools. Operator must know Workspaces UI, checkbox vs Surface, binding vs session enable.

### Intentional constraint

FeatureSets define **invoke ACL** ([`meta-gateway-invoke.md`](./meta-gateway-invoke.md) decision #5). Auto-granting all installed tools defeats the model.

### Where to dig

| Capability | Where to dig |
| ---------- | ------------ |
| FeatureSet editor (checkbox vs Surface) | [`apps/desktop/src/features/featuresets/FeatureSetPanel.tsx`](../../apps/desktop/src/features/featuresets/FeatureSetPanel.tsx) |
| Workspace binding UI | [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx), [`WorkspaceBindingSheet.tsx`](../../apps/desktop/src/features/workspaces/WorkspaceBindingSheet.tsx) |
| Effective features inspector | Workspaces page — "Effective Features" panel (binding resolve preview) |
| Meta: list FS | [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) — `ListFeatureSetsTool` |
| Meta: create FS | Same — `CreateFeatureSetTool` (~L897+) with `tool_qualified_names`, `surfaced_tools` |
| Meta: bind workspace | Same — `BindCurrentWorkspaceTool` (triggers approval — avoid in routine QA) |
| Meta: diagnostic full catalog | Same — `ListAllToolsTool` with `invokable` / `total_invokable` (post-85113e7) |
| Meta: discovery for agents | Same — `SearchToolsTool` |
| Operator vs agent steering | `list_all_tools` `hint` field in same file |
| FS member storage | [`crates/mcpmux-core/src/domain/feature_set.rs`](../../crates/mcpmux-core/src/domain/feature_set.rs) |
| Bundles (starter FS) | FeatureSet bundles in DB; [`docs/planning/meta-gateway-invoke-qa.md`](./meta-gateway-invoke-qa.md) prep |

### Fix options (Phase D — pick 1–2 for v1)

| Option | Effort | Dig/start |
| ------ | ------ | --------- |
| **D1: FS authoring helper meta tool** — `mcpmux_suggest_feature_set_from_servers({ server_ids, surfaced? })` returns member list from `list_all_tools` | ~1 day | Extend [`tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) registry in [`mod.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/mod.rs) |
| **D2: Better empty-search errors** — "server X not in binding FS; add via Workspaces or mcpmux_create_feature_set" | ~0.5 day | [`invoke.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/invoke.rs), [`tool_discovery`](../../crates/mcpmux-gateway/src/services/tool_discovery/) |
| **D3: UI — clone FS / add server bundle to binding** | ~1–2 days | Workspaces + FeatureSet panels |
| **D4: QA template FS checked into docs** | ~0.5 day | Extend [`meta-gateway-invoke-qa.md`](./meta-gateway-invoke-qa.md) with canonical FS export |

### Out of scope

- Auto-infer FeatureSet from open repo / installed servers without explicit write
- Removing human approval for `bind_current_workspace` (space-wide effect)

### Outcome when done

Agent or operator can go from "I need github + gworkspace for this repo" to a bound FS in ≤3 meta calls or one UI flow, without hand-editing JSON.

---

## Umbrella phasing

Recommended ship order — each track is independently testable.

### Phase 1 — Warm pool (Track A)

**Effort:** ~2–3 days · **Doc:** [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md) Phases 1–3

Fixes pain **#1** and most of **#2** (backend availability).

### Phase 2 — Roots + warming signal (Track B)

**Effort:** ~1 day · **Doc:** child doc Phase 2 (may merge with Phase 1 if same PR)

Fixes remaining **#2** agent confusion (`gateway_warming`).

### Phase 3 — Surface list_changed audit (Track C)

**Effort:** ~0.5–1 day investigate; ~1 day fix if gateway gap

Fixes or documents **#3** with evidence.

### Phase 4 — FS authoring DX (Track D)

**Effort:** ~1–2 days for D1+D2

Reduces **#4**; does not eliminate ACL curation.

---

## Pre-PR validation (umbrella)

| Track | How to verify |
| ----- | ------------- |
| A | Dev restart → github invokable < 15s; logs show hot tier first |
| B | Reload Cursor → first search returns tools or `gateway_warming: true` |
| C | Toggle Surface → log `FeatureSetMembersChanged` + optional wire capture; document Cursor behavior |
| D | Agent creates FS from `list_all_tools` names → search finds new tools after bind |

---

## Out of scope (umbrella)

| Item | Reason |
| ---- | ------ |
| Faster uvx/npm installs | Environment / cache, not gateway |
| Cursor `list_changed` implementation | External client |
| Full auto-ACL from repo analysis | Conflicts with invoke security model |
| Disable eager pool / lazy connect on invoke | ARCHITECTURE rejection; large routing change |

---

## Key files referenced

| File | Track |
| ---- | ----- |
| [`crates/mcpmux-gateway/src/server/startup.rs`](../../crates/mcpmux-gateway/src/server/startup.rs) | A |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | B |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | B, C |
| [`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) | B, C |
| [`apps/desktop/src-tauri/src/commands/feature_set.rs`](../../apps/desktop/src-tauri/src/commands/feature_set.rs) | C |
| [`apps/desktop/src/features/featuresets/FeatureSetPanel.tsx`](../../apps/desktop/src/features/featuresets/FeatureSetPanel.tsx) | C, D |
| [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) | B, D |

---

## Related documentation

- [`docs/planning/gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md) — Track A/B implementation detail (decisions locked)
- [`docs/planning/meta-gateway-invoke.md`](./meta-gateway-invoke.md) — meta-tool entrypoint model
- [`docs/planning/meta-gateway-invoke-retest.md`](./meta-gateway-invoke-retest.md) — live QA evidence
- [`crates/mcpmux-gateway/ARCHITECTURE.md`](../../crates/mcpmux-gateway/ARCHITECTURE.md) — eager pool rationale

---

## Reconciliation

Update **Status** and track checkboxes as phases ship. When Track A completes, mark [`gateway-warm-pool-startup.md`](./gateway-warm-pool-startup.md) **Status: Complete** and link back here. Run planning-doc reconciliation before closing the umbrella initiative.

**Decision record (May 25, 2026):** Four-track umbrella for agent session readiness; cold start first via warm pool; Surface audit before building client workarounds; FS authoring improved via meta-tool DX not auto-ACL.
