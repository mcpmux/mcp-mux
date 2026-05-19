# Dynamic MCP Toggling via Meta Tools

**Last Updated:** May 19, 2026
**Status:** Phase 1 complete — SessionOverrideRegistry + list-path composition wired; Phases 2–5 pending
**Branch:** `feat/dynamic-mcp-toggle-meta-tools`
**Base branch:** `feat/workspace-root-routing` ([upstream PR #151](https://github.com/mcpmux/mcp-mux/pull/151))
**Issue:** TBD — file after planning review
**Depends on:** [PR #151](https://github.com/mcpmux/mcp-mux/pull/151) merging or being consumed via fork (provides the `mcpmux_*` namespace, `MetaToolRegistry`, `ApprovalBroker`, `FeatureSetResolverService`, per-peer `list_changed`, `SessionRootsRegistry`)
**Unblocks:** [`jsg-tech-check` homelab MCP strategy](../../../jsg-tech-check/docs/setup/home-lab-overview.md#mcp-strategy--current-state)

---

## Problem

The Cursor / Claude Code pre-McpMux workflow gave a per-project escape valve: each `.cursor/mcp.json` declared a subset of servers, so the client only loaded the tools that mattered for that project. Token budget stayed proportional to the project's actual needs.

Routing everything through McpMux collapses that signal. The gateway exposes one consolidated MCP endpoint; the client side sees a single `mcpmux` server entry that's either ON or OFF. All 35+ tools from every enabled backend land in the LLM context window the moment a session opens, regardless of what the project actually needs.

PR #151 partly addresses this with persistent `WorkspaceBinding`s: bind `~/code/personal/set-times-app` to `{core, browser, design, db-personal}` and the gateway serves exactly those tools when Cursor opens that folder. That works for stable, known scopes. It does not work for:

- **Discovery-driven work** — "use whichever MCPs you need for this task" with no pre-declared bundle.
- **One-off needs** — "I'm in `set-times-app` (which is bound to a bundle that excludes `firebase`) but I need `firebase` for the next 15 minutes."
- **Minimum-context defaults** — start a session with zero backend tools loaded, let the LLM pull in what it needs based on the manifest, drop tools when it's done.

The user-facing ask, stated as the original request:

> Instead of having the whole definitions of all my MCPs all the time, I'd just have 1 always-on tool that gives me a manifest and then mcp can be smart enough to turn itself on.

PR #151 ships four meta tools (`mcpmux_list_all_tools`, `mcpmux_list_feature_sets`, `mcpmux_create_feature_set`, `mcpmux_bind_current_workspace`). They cover the manifest + persist-a-new-bundle path. They don't cover the ephemeral toggle path — there's no way to turn a backend server on for "just this session" without writing a binding to the DB.

This doc extends the meta-tools surface with session-scoped enable/disable, plus a server-level (coarser than tool-level) `mcpmux_list_servers` manifest tool. The resolver gains a Tier 0 (`SessionOverride`) that composes additively over Tier 1's `WorkspaceBinding`.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Granularity | **Server-level** (`mcpmux_enable_server("github")`), not tool-level | Matches the user's mental model ("turn on github") and the existing `FeatureSetType::ServerAll`. Tool-level enable can be added later as a degenerate case if a real use case shows up. |
| 2 | Default scope | **Session** (default), with `scope: "workspace"` as an opt-in arg | Session is the low-risk default; ephemerality is the point. Workspace scope falls back to the existing `WorkspaceBinding` write path, reusing PR #151 plumbing. |
| 3 | Composition with bindings | **Additive over `WorkspaceBinding`**: `effective = (binding ∪ session_enabled) − session_disabled` | Lets users keep their stable per-project bundle AND opportunistically add a server for a single session. Subtractive disable lets them mute a noisy server temporarily without unbinding. |
| 4 | Override lifetime | **In-memory, dies with `mcp-session-id`** | Matches `SessionRootsRegistry` semantics introduced by PR #151. Restart of gateway or client = fresh start. No DB persistence; no migration. |
| 5 | Approval flow | Session enables auto-allow by default (configurable); workspace writes require approval (existing flow) | Session-scope is ephemeral and safer than persistent state. App setting `gateway.session_overrides_require_approval` (default `false`) lets paranoid users gate everything. |
| 6 | Audit | Every override emits a `DomainEvent::MetaToolInvoked` (existing path) | No new event variants; the audit log already renders meta-tool calls. The "decision" field gets `"session_override"` for auto-allowed session writes. |
| 7 | Manifest format | `mcpmux_list_servers` returns server roster with `{id, name, tool_count, status}` where status ∈ `enabled_via_binding \| enabled_via_session \| disabled_via_session \| inactive` | The LLM needs to see current state, not just availability — otherwise it can't reason about whether to call enable or just call the tool. |
| 8 | Tier-0 placement | New `SessionOverrideRegistry` consulted **inside** `FeatureService` materialization, not as a new resolver tier | Resolver already returns `(space, feature_set_ids)` cleanly. Layering at the materialization step keeps the resolver pure and concentrates the composition logic in one place (`FeatureService::get_tools_for_grants`). |

---

## The Model

### Override store

Per-session, two server-id sets. Both empty = no overrides, default routing applies.

```text
SessionOverrideRegistry {
    enabled : DashMap<SessionId, HashSet<ServerId>>,
    disabled: DashMap<SessionId, HashSet<ServerId>>,
}
```

GC mirrors `SessionRootsRegistry`: both maps drop on `MCPNotifier`'s session-reap pass.

### Composition rule

For a session resolving its effective server set:

```text
1. (space, feature_set_ids) ← FeatureSetResolverService::resolve(...)
2. binding_servers ← FeatureService::servers_for(space, feature_set_ids)
3. session_on     ← SessionOverrideRegistry.enabled[session_id]
4. session_off    ← SessionOverrideRegistry.disabled[session_id]
5. effective      ← (binding_servers ∪ session_on) − session_off
6. tools          ← every Tool feature whose server_id ∈ effective AND is_available
```

`session_on` and `session_off` are honored even when the resolver returned `Deny` (no binding match) — the session-override path is how a roots-capable client opts into tools without a binding. Empty override sets + `Deny` from resolver = no tools (existing behavior).

### Tool surface

Three new tools added to `build_default_registry`:

| Tool | Type | Approval (default) | Purpose |
| ---- | ---- | ------------------ | ------- |
| `mcpmux_list_servers` | read | none | Server-level manifest with status per server. Coarser than `mcpmux_list_all_tools`. |
| `mcpmux_enable_server` | write | session: auto-allow; workspace: approval | Adds `server_id` to session overrides (or writes a binding). |
| `mcpmux_disable_server` | write | session: auto-allow; workspace: approval | Adds `server_id` to session disable set (or removes from binding). |

Each write fires `tools/list_changed` per-peer via the existing `MCPNotifier::notify_peer_lists_changed` path so the calling LLM's tool list refreshes mid-conversation.

### What McpMux still stores

| Item | Storage | Persistence |
| ---- | ------- | ----------- |
| Session overrides (enabled + disabled sets) | `SessionOverrideRegistry` (in-memory `DashMap`) | Process-lifetime; dies with session reap |
| Workspace-scope writes | `workspace_bindings` table (existing) | Persistent (no schema change) |
| Audit trail | `DomainEvent::MetaToolInvoked` (existing) | Persistent via existing audit log |
| `gateway.session_overrides_require_approval` setting | `app_settings` table (existing) | Persistent |

---

## Architecture

```
              ┌──────────────────────────────────────────┐
              │  FeatureService::get_tools_for_grants    │
              │  (existing materialization chokepoint)   │
              │                                          │
              │  binding_servers   = resolver-derived    │
              │  + session_enabled ← Tier 0 overrides    │
              │  − session_disabled                      │
              └──────────────────────────────────────────┘
                              ▲
                              │
   ┌──────────────────────────┴──────────────────────────┐
   │                                                     │
   ▼                                                     ▼
┌─────────────────────────┐              ┌──────────────────────────────┐
│ FeatureSetResolverService│             │ SessionOverrideRegistry      │
│ (PR #151 — unchanged)    │             │ (new)                        │
│                          │             │                              │
│ Tier 1: WorkspaceBinding │             │ enabled : DashMap<sid, set>  │
│ Tier 2: ClientGrant      │             │ disabled: DashMap<sid, set>  │
│ Tier 3: Deny             │             └──────────────────────────────┘
└─────────────────────────┘                            ▲
                                                       │
                                          ┌────────────┴────────────┐
                                          │ Meta tool writes mutate │
                                          │ this registry directly. │
                                          │                         │
                                          │ mcpmux_enable_server    │
                                          │ mcpmux_disable_server   │
                                          └─────────────────────────┘
```

- `SessionOverrideRegistry` lives in `crates/mcpmux-gateway/src/services/`, sibling to `session_roots.rs`. Same `Arc<Self>` factory pattern, same GC contract.
- `FeatureService` is the only consumer that reads it. The resolver itself stays pure — no new tier, no new branch in `feature_set_resolver.rs`.
- Writes go through the existing `MetaToolRegistry` dispatch in `tools.rs` → `with_approval()` (session-scope short-circuits approval when the setting allows) → mutate the registry → emit `tools/list_changed` via the existing `emit_tools_list_changed` helper.

---

## Files to create

| File | Purpose |
| ---- | ------- |
| `crates/mcpmux-gateway/src/services/session_overrides.rs` | `SessionOverrideRegistry` — `DashMap`-backed enable/disable sets, GC hooks, query helpers (`is_enabled`, `is_disabled`, `effective_overlay`) |
| `tests/rust/tests/integration/meta_tools.rs` | Composition tests: deny bootstrap, disable, additive (Phase 1); meta-tool E2E (existing) |
| `docs/planning/dynamic-mcp-toggle-meta-tools.md` | This doc |

## Files to modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-gateway/src/services/mod.rs`](../../crates/mcpmux-gateway/src/services/mod.rs) | `pub mod session_overrides;` + re-export `SessionOverrideRegistry` |
| [`crates/mcpmux-gateway/src/services/meta_tools/mod.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/mod.rs) | Register `ListServersTool`, `EnableServerTool`, `DisableServerTool` in `build_default_registry`. Add `session_overrides: Arc<SessionOverrideRegistry>` to `MetaToolContext`. |
| [`crates/mcpmux-gateway/src/services/meta_tools/registry.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/registry.rs) | Extend `MetaToolContext` with `session_overrides`. Add `"session_override"` to the decision-string match in `MetaToolRegistry::call` so audit rows are distinguishable. |
| [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) | Implement `ListServersTool`, `EnableServerTool`, `DisableServerTool`. Session-scope short-circuits `with_approval` when `gateway.session_overrides_require_approval` is false. |
| [`crates/mcpmux-gateway/src/pool/features/facade.rs`](../../crates/mcpmux-gateway/src/pool/features/facade.rs) | `FeatureService::get_tools_for_grants` (and sibling `get_prompts_for_grants`, `get_resources_for_grants`) take `session_id: Option<&str>` and apply `SessionOverrideRegistry` composition before returning. |
| [`crates/mcpmux-gateway/src/mcp/handler.rs`](../../crates/mcpmux-gateway/src/mcp/handler.rs) | Pass `session_id` (already on `RequestContext`) into the new `get_*_for_grants` signatures. |
| [`crates/mcpmux-gateway/src/server/service_container.rs`](../../crates/mcpmux-gateway/src/server/service_container.rs) | Construct `Arc<SessionOverrideRegistry>` once; wire into `MetaToolContext`, `FeatureService`, and the session-reap path in `MCPNotifier`. |
| [`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) | In the session-reap pass, also call `SessionOverrideRegistry::remove(session_id)` alongside `SessionRootsRegistry::remove`. |
| [`crates/mcpmux-core/src/domain/event.rs`](../../crates/mcpmux-core/src/domain/event.rs) | No new variant — `MetaToolInvoked` already carries `decision: String`. Document `"session_override"` as a valid value in the doc comment. |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | New "Active session overrides" sub-panel under the live-session inspector: per-session list of enabled / disabled server_ids with a "clear" button. |
| [`apps/desktop/src-tauri/src/commands/workspace_binding.rs`](../../apps/desktop/src-tauri/src/commands/workspace_binding.rs) | New Tauri commands: `list_session_overrides(session_id)`, `clear_session_overrides(session_id)`. Read-only + clear; mutation happens via the MCP tool, not the UI. |
| [`apps/desktop/src/lib/api/workspaceBindings.ts`](../../apps/desktop/src/lib/api/workspaceBindings.ts) | TS wrappers for the two new commands. |

---

## Phasing

### Phase 1 — `SessionOverrideRegistry` + composition wiring ✅

**Effort:** 1 evening  
**Completed:** May 19, 2026

- [x] `crates/mcpmux-gateway/src/services/session_overrides.rs` — `DashMap`-backed registry with `enable`, `disable`, `clear`, `enabled_set`, `disabled_set`, `remove`, `list_all`
- [x] Plumb `Arc<SessionOverrideRegistry>` through `ServiceContainer` → `ServiceFactory` → `FeatureService` and `MCPNotifier`
- [x] `FeatureService::get_*_for_grants(..., session_id: Option<&str>)` applies server-level composition: `effective = (binding_servers ∪ enabled) − disabled`, then all available features per effective `server_id`
- [x] All callsites updated (`handler.rs`, `routing.rs`, `handlers.rs`, `meta_tools/diff.rs`, integration tests) — MCP handler passes real session id; others pass `None`
- [x] `MCPNotifier::reap_dead_sessions` drops override entries alongside session roots
- [x] Unit tests in `session_overrides.rs`; composition tests in `tests/rust/tests/integration/meta_tools.rs` (deny bootstrap, disable, additive)

**Outcome (verified):** Direct registry mutation changes the next `get_tools_for_grants` result. Meta-tools and UI unchanged. `RoutingService::call_tool` authorization deferred to Phase 3 (list-only in Phase 1).

**Implementation notes:**
- Server-level composition loads **all available** features for each effective `server_id` (not FS-partial tool subsets).
- Fixed pre-existing DashMap deadlock in `SessionRootsRegistry::record_resolution` (`get` guard must not overlap `insert` on the same map).

### Phase 2 — `mcpmux_list_servers` read tool

**Effort:** 1 evening

- Add `ListServersTool` unit struct + `MetaTool` impl in `meta_tools/tools.rs`.
- Implementation: load `ServerFeature::list_for_space(caller_space_id)`, group by `server_id`, compute `tool_count = features.iter().filter(|f| f.feature_type == Tool).count()`, derive `status` per server by checking `binding`, `session_overrides.enabled`, `session_overrides.disabled` in order.
- JSON schema: empty `properties` (no args).
- Register in `build_default_registry` alongside the existing reads.
- Integration test: connect a fake session, call `mcpmux_list_servers`, assert response shape includes `status` enum values for both bound and unbound servers.

**Outcome:** An LLM calling `mcpmux_list_servers` from any session receives a server roster like `[{id: "github", name: "GitHub", tool_count: 24, status: "enabled_via_binding"}, {id: "firebase", name: "Firebase", tool_count: 18, status: "inactive"}, ...]`. No state mutation yet.

### Phase 3 — `mcpmux_enable_server` / `mcpmux_disable_server` (session scope)

**Effort:** 1 day

- Add `EnableServerTool` + `DisableServerTool` to `meta_tools/tools.rs`.
- Args: `{ server_id: string, scope?: "session" | "workspace" (default "session") }`.
- Session-scope flow: validate `server_id` exists in caller's resolved Space → look up `gateway.session_overrides_require_approval` setting → if `false`, mutate registry directly; if `true`, route through `with_approval` first.
- Enable adds to `enabled` and removes from `disabled` (the two sets are mutually exclusive per server-id, last-write-wins).
- Disable mirror: adds to `disabled`, removes from `enabled`.
- After mutation: fire per-peer `tools/list_changed` via `notify_peer_lists_changed(client_id)`. Emit `MetaToolInvoked` with `decision: "session_override"` when auto-allowed, `"allow_once"` when approval was required.
- Reject `scope: "workspace"` with `MetaToolError::InvalidArgument("workspace scope not yet implemented; see Phase 4")` until Phase 4 lands.
- Integration tests: enable → tool appears in next `tools/list`, disable → tool disappears, both with the per-peer notify firing.

**Outcome:** From a fresh Cursor window (no binding, no overrides), an LLM calls `mcpmux_enable_server({"server_id": "github"})`. The GitHub tools appear in the next `tools/list`. The LLM uses them, then calls `mcpmux_disable_server({"server_id": "github"})` when done. Tools disappear. No DB writes; closing Cursor and reopening it = clean slate.

### Phase 4 — Workspace-scope variants

**Effort:** 1 day

- Extend `EnableServerTool` / `DisableServerTool` to handle `scope: "workspace"`.
- Enable + workspace: requires the caller to have reported MCP roots (reuse `caller_space_id` + `session_roots.get` pattern from `BindCurrentWorkspaceTool`). If no binding exists for the first reported root, return `MetaToolError::InvalidArgument("no binding exists for this workspace; create one with mcpmux_create_feature_set + mcpmux_bind_current_workspace first")`. If a binding exists, look up its FS, add a `ServerAll`-typed `FeatureSet` for `server_id`, append its id to the binding's `feature_set_ids` list.
- Disable + workspace: remove the matching `ServerAll` FS from the binding's `feature_set_ids` if present; if the server's tools come from a custom FS (not a `ServerAll` row), reject with a message pointing the user at the Workspaces UI.
- Always require approval for workspace scope (no auto-allow setting).
- Integration test: enable + workspace persists across a session restart; disable + workspace removes from binding row.

**Outcome:** An LLM in a bound workspace adds a `ServerAll` FS layer to its binding via `mcpmux_enable_server({"server_id": "firebase", "scope": "workspace"})`, approves in the desktop dialog, and the next time it opens that folder Firebase tools are there without re-enabling.

### Phase 5 — UI surface for session overrides

**Effort:** 1 day

- New "Active session overrides" sub-panel inside `WorkspacesPage.tsx`'s live-session inspector: lists per-session `enabled`/`disabled` server ids alongside the reported roots.
- "Clear all overrides" button per session — calls the new `clear_session_overrides` Tauri command. Useful when a session got into a weird state and the user wants a clean default-routing read.
- New Tauri commands: `list_session_overrides(session_id) -> { enabled: string[], disabled: string[] }`, `clear_session_overrides(session_id)`.
- Settings checkbox under Gateway settings: "Require approval for session-scope overrides" — wires to `gateway.session_overrides_require_approval`.
- README + CHANGELOG entries describing the new meta-tools and the manifest-driven workflow.

**Outcome:** From the Workspaces tab, a user can see at a glance "session abc123 has GitHub enabled (session) and Firebase disabled (session)" and clear them with one click. The new approval-required setting is discoverable in Gateway settings without reading docs.

---

## Out of scope

| Item | Reason |
| ---- | ------ |
| Tool-level granularity (`mcpmux_enable_tools(["github_create_issue"])`) | Server-level covers the user's stated use case. Adding tool-level later is additive — same approval flow, more specific `qualified_name` list. No real evidence yet that tool-level matters more than server-level for token budget. |
| Persistent session preferences across gateway restarts | Process-lifetime is the design — sessions die when the client reconnects. If a user wants stickiness, they should use a binding. Adding persistence here would duplicate the binding system poorly. |
| Auto-enable on tool-call hint ("LLM tried to call `github_create_issue` → silently enable github first") | Requires a "shadow tool list" mechanism in the handler (advertise more than is currently active). Possible follow-up, but design isn't obvious — silent enable defeats the audit trail. |
| Cross-client session sharing | `mcp-session-id` is per-MCP-session; two Cursor windows have two sessions and two override sets. By design — independent contexts. |
| Override expiry / TTL | Sessions are already ephemeral. A TTL would be a different concept and isn't asked for. |
| Tool-level disable inside an already-enabled server | Use `mcpmux_create_feature_set` + `mcpmux_bind_current_workspace` (PR #151's existing path) for fine-grained subsets. |

---

## Key files referenced

| File | Why |
| ---- | --- |
| [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) | Where the three new `MetaTool` impls land. Existing `with_approval` + `caller_space_id` patterns are the templates. |
| [`crates/mcpmux-gateway/src/services/meta_tools/mod.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/mod.rs) | `build_default_registry` factory — registration site for the new tools. `MetaToolContext` gains one new field. |
| [`crates/mcpmux-gateway/src/services/meta_tools/registry.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/registry.rs) | `MetaToolRegistry::call` dispatch + audit emission. Adds `"session_override"` decision string. |
| [`crates/mcpmux-gateway/src/services/meta_tools/approval.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/approval.rs) | `ApprovalBroker` — reused as-is for workspace-scope writes. Session-scope writes short-circuit when the setting allows. |
| [`crates/mcpmux-gateway/src/services/session_roots.rs`](../../crates/mcpmux-gateway/src/services/session_roots.rs) | Pattern reference for `SessionOverrideRegistry`. Same `Arc<Self>` + `DashMap` + GC contract. |
| [`crates/mcpmux-gateway/src/services/feature_set_resolver.rs`](../../crates/mcpmux-gateway/src/services/feature_set_resolver.rs) | Tier 1/2/3 resolver — stays untouched. Override composition happens in `FeatureService`, not here. |
| [`crates/mcpmux-gateway/src/pool/features/facade.rs`](../../crates/mcpmux-gateway/src/pool/features/facade.rs) | `FeatureService::get_tools_for_grants` is the materialization chokepoint where the override composition runs. |
| [`crates/mcpmux-gateway/src/consumers/mcp_notifier.rs`](../../crates/mcpmux-gateway/src/consumers/mcp_notifier.rs) | Session-reap pass — extend to also drop override entries. `notify_peer_lists_changed` is reused for the post-write list refresh. |
| [`apps/desktop/src/features/workspaces/WorkspacesPage.tsx`](../../apps/desktop/src/features/workspaces/WorkspacesPage.tsx) | New "Active session overrides" sub-panel slots into the existing live-session inspector. |

---

## Related work

- [mcpmux/mcp-mux PR #151](https://github.com/mcpmux/mcp-mux/pull/151) — workspace-root-driven FeatureSet routing + the `mcpmux_*` meta-tool namespace this PR builds on. Must merge (or be consumed via fork) first.
- [`docs/planning/issue-52-secret-text-input-syntax.md`](./issue-52-secret-text-input-syntax.md) — sibling planning doc; same conventions used here. Independent feature, no functional overlap.
- [`jsg-tech-check` homelab plan](../../../jsg-tech-check/docs/setup/home-lab-overview.md#mcp-strategy--current-state) — the consuming use case. The "Personal vs Work" Spaces + bundled `set-times-app` / `sync2hire-platform` model leans on bindings; this doc adds the "no, actually just enable this one MCP for the next 10 minutes" escape valve.
- [MCP spec — Tools `list_changed`](https://modelcontextprotocol.io/specification/2025-11-25/server/tools#list-changed-notification) — the protocol mechanism that makes the post-write tool-list refresh observable mid-conversation. Already wired by PR #151.

---

## Reconciliation

This doc is the source of truth for what gets built. When implementation completes, update the **Status** field at the top and reconcile any deviations (extra files, dropped phases, scope changes) per [`update-planning-md`](~/.cursor/commands/update-planning-md.md).
