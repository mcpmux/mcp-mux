# Clone Auth Header Config Editing

**Last Updated:** Jul 23, 2026
**Status:** Planning — ready to implement
**Depends on:** Clone lineage (`cloned_from`, migration 021) and `manual_entry` install source — both already shipped
**Unblocks:** Editing/fixing auth headers on any `manual_entry` clone through the UI instead of raw `sqlite3`

---

## Problem

`posthog-personal-mesh`, a clone of `posthog-personal`, silently queried the **parent's** PostHog project (`345911`, "set-times-app") instead of its own (`501917`, "Mesh") — no error, just wrong data every call. Trying to fix it through the McpMux UI's Definition editor returned:

```
Server 'posthog-personal-mesh' not found in config
```

Traced live (DB inspection + code reading, not guessed):

- `clone_server()` (`crates/mcpmux-core/src/application/server.rs:374-443`) copies the source's `cached_definition` wholesale into the new row, including the definition's embedded `source: ServerSource` field (`crates/mcpmux-core/src/domain/server.rs:38,80`). If the parent was `UserSpace`-sourced, the clone's `cached_definition` inherits that same `source` tag even though the clone's own row lives only in SQLite (`installation_source: ManualEntry`).
- The Definition editor gates edit vs. read-only purely on that inherited tag — `isEditable = server.source.type === 'UserSpace'` (`ServerDefinitionModal.tsx:80`), `canEditDefinition={server.source.type === 'UserSpace'}` (`ServersPage.tsx:1967`) — never checking `installation_source`. So the UI shows an "editable" definition, but Save calls `updateServerInConfig()` → `update_server_in_config` (`apps/desktop/src-tauri/src/commands/space.rs:322`, mirrored in `crates/mcpmux-gateway/src/admin/command_bridge/space.rs:182`), which only reads/writes `spaces/*.json`. The clone has no key there → `"not found in config"`. Every `manual_entry` clone hits this, not just this one server.
- Separately, `clone_server()` copies definition/lineage but **not** `extra_headers`, `input_values`, `env_overrides`, `args_append`, or credentials (`server.rs:413-420`) — the new row starts with `extra_headers: {}`. There is **no runtime fallback to the parent's `extra_headers`** — `build_transport_config()` (`crates/mcpmux-gateway/src/pool/transport/resolution.rs:47-127`) only reads the clone's own row and merges `installed.extra_headers` last (line 127). The wrong-project behavior came from the clone's copied `cached_definition` carrying the parent's baked-in header/template values forward while the clone's own override column stayed empty — not from any live parent lookup. Working sibling `posthog-personal-gait` has both `Authorization` and `x-posthog-project-id` set in its **own** `extra_headers`, proving the override mechanism works fine once populated.
- The Configure modal (`ServersPage.tsx`, `save_server_inputs` → `update_config()` at `server.rs:217-269`) already has key/value editors for `extra_headers` (HTTP transports) and does correctly persist per-clone overrides — the gap is that nothing seeds or prompts for them at clone time, and nothing warns when they're left empty.
- After patching `cached_definition` directly via SQL, a plain client-side reconnect/retry returned "Connection closed"; only killing and relaunching `McpMux.app` picked up the change. Root cause: `PoolService::connect_server` (`crates/mcpmux-gateway/src/pool/service.rs:267-297`) returns early (`reused: true`, line 291) whenever the existing instance `is_healthy()` (line 284) — it never re-reads the DB unless the instance is explicitly evicted first. `retry_connection` (`apps/desktop/src-tauri/src/commands/server_manager.rs:512-545`) is the one path that calls `remove_instance()` before reconnecting, and `ServersPage` already calls it after Configure saves (`ServersPage.tsx:1087,1184,1221,1446,1472`). But `ServerConfigUpdated` (`crates/mcpmux-core/src/domain/event.rs:200`) — emitted by both `update_config()` and the (to-be-added) definition update — is only bridged to the frontend for a toast (`apps/desktop/src-tauri/src/commands/gateway.rs:547`, `crates/mcpmux-gateway/src/admin/ui_events.rs:115`). Nothing in the gateway subscribes to it to evict the pool instance, and the frontend's own `server-changed` handler only reloads the list on `installed`/`uninstalled` (`ServersPage.tsx:522`), not `config_updated`. So a save that doesn't happen to route through Configure's explicit `retryConnectionV2` call has no invalidation path at all.
- Headless/web-admin `retry_connection` is unimplemented — returns `gateway_write_unavailable()` / `gateway_not_running()` in both `LiveGatewayWriteRuntime` and the stub runtime (`crates/mcpmux-gateway/src/admin/write_runtime.rs:181-182,407-409`). Desktop delegates to Tauri instead of this trait, so the gap only bites web-admin-only usage today, but it means the invalidation fix can't be uniform across both surfaces without also filling this in.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Edit path for `manual_entry` clones | **Both**: add a DB-backed definition save path for clones (writes `cached_definition` on the `installed_servers` row instead of `spaces/*.json`), and tighten Configure/clone-wizard so headers are visible up front | Fixes the false-affordance bug (editor claims editable, save 404s) *and* the discoverability gap (headers only reachable by digging into Configure after the fact) |
| 2 | Clone creation: auth seeding | **Copy parent's `extra_headers` / `input_values`** into the new clone as editable starting values | Directly prevents the exact bug — a header-auth'd clone no longer starts silently blank; user still must swap the project-specific value but isn't starting from nothing |
| 3 | Empty/inherited-auth footgun | **Warn**, don't block | A banner/toast when a clone with empty `extra_headers` is enabled while its parent/definition required auth headers. Fail-closed was rejected — some clones legitimately don't need headers (e.g. stdio env-only auth), and a hard block would break those |
| 4 | Pool invalidation on save | **Auto-evict + reconnect on `ServerConfigUpdated`**, not just Configure's existing explicit `retryConnectionV2` call | Closes the gap for the new definition-save path and for `UserSpaceSyncService`'s file-driven `update_cached_definition()` (`crates/mcpmux-core/src/application/user_space_sync.rs:154-162`), which today updates `cached_definition` with zero event emission and zero pool invalidation |
| 5 | Scope | **Desktop + web admin write-runtime/bridge parity in the same effort** | `LiveGatewayWriteRuntime::retry_connection` (`write_runtime.rs:181`) and the clone/display-name stubs in `command_bridge/write.rs` are already tracked as Phase 6 gaps (`dev-to-main-port.md`); doing the invalidation and definition-update work once across both runtimes avoids reopening this file again for web-admin parity later |
| 6 | Clone-time `source` rewrite | **Yes** — `clone_server()` rewrites the copied definition's embedded `source` to reflect the clone's own storage (not `UserSpace`) | Removes the root cause of the false editability signal at its origin, so the UI gate (`isEditable`/`canEditDefinition`) stays correct without needing to special-case `installation_source` everywhere it's checked |

---

## Scope

**In:**
- `clone_server()`: rewrite `definition.source` on the copy; copy `extra_headers`/`input_values` from the source row into the new row
- New DB-backed definition update path for `manual_entry` rows (Tauri command + admin bridge command), parallel to `update_server_in_config` but targeting `installed_servers.cached_definition`
- `ServerDefinitionModal` / `ServersPage`: route save to the new DB path when `installation_source === ManualEntry`; editability gate switches from `source.type` to `installation_source`
- Clone wizard / Configure: surface required auth header fields inline when cloning a header-auth'd server (using whatever fields the parent's own `extra_headers`/registry input schema names)
- Warning UI when a clone with auth-requiring parent/definition has empty `extra_headers` at enable/connect time
- `ServerConfigUpdated` gains a gateway-side subscriber that calls `remove_instance()` + reconnect for enabled servers (both Tauri desktop path and `LiveGatewayWriteRuntime`)
- Wire the same auto-evict trigger into `UserSpaceSyncService::update_cached_definition()` callers
- Implement `LiveGatewayWriteRuntime::retry_connection` (currently stubbed) so web-admin gets the same invalidation guarantee

**Out:**

| Item | Reason |
| ---- | ------ |
| Fail-closed enforcement (blocking enable/connect on missing headers) | Rejected in Decision 3 — would break legitimate no-auth clones; warn-only is the chosen behavior |
| Transport fingerprint check on `connect_server`'s healthy-reuse path | Not needed once `ServerConfigUpdated` reliably evicts on every config/definition write; revisit only if the event-driven path proves to miss cases in practice |
| Copying OAuth credentials / `credentials` table rows on clone | Separate concern — credentials are per-install by design (OAuth tokens shouldn't be shared across clones); only header/input overrides are in scope here |
| Fixing `docs/guide/gateway.mdx`'s outdated `server_id + sha256(config)` pooling description | Docs correction, unrelated to the actual code fix; flag separately |
| Un-stubbing the rest of `command_bridge/write.rs` (clone_server, set_server_display_name for web admin) | Tracked already as Phase 6 in `dev-to-main-port.md`; only `retry_connection` is pulled forward here because this fix depends on it |

---

## Architecture

### Clone-time fixes (`clone_server`)

```rust
// crates/mcpmux-core/src/application/server.rs, inside clone_server()
definition.id = new_server_id.clone();
definition.name = format!("{} ({})", source.display_name(), normalized_suffix);
definition.alias = Some(alias);
definition.source = ServerSource::ManualEntry; // new variant — add to enum at crates/mcpmux-core/src/domain/server.rs:80 (today only UserSpace/Bundled/Registry exist)

let server = InstalledServer::new(&space_id_str, &new_server_id)
    .with_definition(&definition)
    .with_source(InstallationSource::ManualEntry)
    .with_cloned_from(source_server_id)
    .with_display_name_override(display_name_override)
    .with_update_policy(source.update_policy)
    .with_pinned_version(source.pinned_version.clone())
    .with_extra_headers(source.extra_headers.clone())   // new — seed, still user-editable
    .with_input_values(source.input_values.clone())     // new — seed, still user-editable
    .with_enabled(false);
```

**Pre-flight confirmed:** `ServerSource` (`crates/mcpmux-core/src/domain/server.rs:80-91`) currently has only `UserSpace { space_id, file_path }`, `Bundled` (default), and `Registry { url, name }` — no `ManualEntry` variant. Add one (unit struct, no fields needed) mirroring `InstallationSource::ManualEntry`.

### Definition editability gate (frontend)

```typescript
// Before (ServerDefinitionModal.tsx:80, ServersPage.tsx:1967)
const isEditable = server.source.type === 'UserSpace';

// After — check installation storage, not definition provenance
// Field is snake_case on the view model (confirmed live usage: ServersPage.tsx:148,185,227,960,1331,1351,1766)
const isEditable =
  server.installation_source?.type === 'UserConfig' ||
  server.installation_source?.type === 'ManualEntry'; // manual_entry now routes to the new DB save path
```

Save dispatch branches on the same field: `UserConfig` → existing `updateServerInConfig()`; `ManualEntry` → new `updateClonedServerDefinition()`.

### New DB-backed definition save path

Mirrors the existing JSON path (`update_server_in_config`) but targets the row directly:

```rust
// New: crates/mcpmux-core/src/application/server.rs
pub async fn update_definition(
    &self,
    space_id: Uuid,
    server_id: &str,
    definition: ServerDefinition,
) -> Result<InstalledServer> {
    // require installation_source == ManualEntry — reject otherwise with a clear error
    // update cached_definition + server_name via existing update_cached_definition() or repo.update()
    // emit DomainEvent::ServerConfigUpdated
}
```

Exposed as a new Tauri command (desktop) and admin bridge command (web admin), parallel to `update_server_in_config` / `command_bridge/space.rs`.

### Pool invalidation on `ServerConfigUpdated`

Today `ServerConfigUpdated` only reaches UI toasts (`gateway.rs:547`, `ui_events.rs:115`) — no gateway consumer. New subscriber added alongside the existing consumers in `crates/mcpmux-gateway/src/consumers/` (`mcp_notifier.rs`, `OAuthEventHandler`), registered the same way they are in `crates/mcpmux-gateway/src/server/mod.rs:398-406` off the `domain_event_tx.subscribe()` receiver (`crates/mcpmux-gateway/src/server/state.rs:141`):

```rust
DomainEvent::ServerConfigUpdated { space_id, server_id } => {
    if server_is_enabled(space_id, &server_id) {
        pool_service.remove_instance(space_id, &server_id);
        // next request or an eager reconnect picks up build_transport_config() fresh from DB
    }
}
```

`UserSpaceSyncService::sync_from_file`'s `update_cached_definition()` calls (`user_space_sync.rs:154-162`) currently emit nothing — add the same `ServerConfigUpdated` emission there so file-driven definition changes get the same eviction, closing the second silent-cache path found during research.

### Auth-seeding + warning surfaces (frontend)

- Clone wizard step reads the parent's `extra_headers` keys (and any registry input schema requiring headers) and pre-fills them into the new clone's Configure state, editable before the clone is enabled.
- Enable/connect path checks: parent (or definition) declares required headers, clone's own `extra_headers` is empty for those keys → non-blocking warning banner ("This clone may be using the wrong credentials — review its headers in Configure").

---

## Files to Modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-core/src/application/server.rs`](../../crates/mcpmux-core/src/application/server.rs) | `clone_server()`: rewrite `definition.source`, seed `extra_headers`/`input_values` from source. New `update_definition()` method for `manual_entry` rows, emits `ServerConfigUpdated` |
| [`crates/mcpmux-core/src/domain/server.rs`](../../crates/mcpmux-core/src/domain/server.rs) | Add `ServerSource::ManualEntry` unit variant (currently only `UserSpace`/`Bundled`/`Registry`, L80-91) |
| [`crates/mcpmux-core/src/application/user_space_sync.rs`](../../crates/mcpmux-core/src/application/user_space_sync.rs) | Emit `ServerConfigUpdated` alongside existing `update_cached_definition()` calls so file-sync definition changes also trigger pool eviction |
| [`crates/mcpmux-gateway/src/consumers/`](../../crates/mcpmux-gateway/src/consumers/) (new module, alongside `mcp_notifier.rs`) | New `ServerConfigUpdated` subscriber: `remove_instance()` for enabled servers on config/definition change |
| [`crates/mcpmux-gateway/src/server/mod.rs`](../../crates/mcpmux-gateway/src/server/mod.rs) | Register the new consumer's `.subscribe()` loop alongside `OAuthEventHandler` (L398-406) |
| [`crates/mcpmux-gateway/src/admin/write_runtime.rs`](../../crates/mcpmux-gateway/src/admin/write_runtime.rs) | Implement `LiveGatewayWriteRuntime::retry_connection` (currently `gateway_write_unavailable()` at L181-182) |
| [`crates/mcpmux-gateway/src/admin/command_bridge/space.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/space.rs) | New bridge command for `update_definition()`, parallel to the existing `update_server_in_config` (L182 error site) |
| [`apps/desktop/src-tauri/src/commands/space.rs`](../../apps/desktop/src-tauri/src/commands/space.rs) | New Tauri command wrapping `update_definition()`, parallel to `update_server_in_config` (L322 error site) |
| [`apps/desktop/src-tauri/src/commands/server_clone.rs`](../../apps/desktop/src-tauri/src/commands/server_clone.rs) | Thread through any new clone-time auth-seeding params if the wizard needs them at create time rather than post-clone Configure |
| [`apps/desktop/src/components/ServerDefinitionModal.tsx`](../../apps/desktop/src/components/ServerDefinitionModal.tsx) | Editability gate switches from `source.type === 'UserSpace'` (L80) to `installation_source`-based check; save dispatch branches to new DB path for `manual_entry` |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | `canEditDefinition` prop (L1967) updated to match; `server-changed` handler (L522) extended to also reconnect on `config_updated`; warning banner for empty-header clones |
| [`apps/desktop/src/features/servers/CloneAccountModal.tsx`](../../apps/desktop/src/features/servers/CloneAccountModal.tsx) | Add header/input seeding step or pre-fill Configure with parent's values post-clone |
| [`apps/desktop/src/lib/api/spaces.ts`](../../apps/desktop/src/lib/api/spaces.ts) | New API shim for the DB-backed definition update command |
| [`apps/desktop/src/lib/backend/events/useDomainEvents.ts`](../../apps/desktop/src/lib/backend/events/useDomainEvents.ts) | Confirm `config_updated` payload shape is sufficient for the new reconnect-on-save handler |

---

## Phases

### Phase 1 — Clone-time fixes: source rewrite + auth seeding (~half day)

- `clone_server()`: rewrite `definition.source`; seed `extra_headers` and `input_values` from the source row (`server.rs:413-420`)
- Unit test: cloning a `UserSpace`-sourced, header-auth'd server produces a row with `installation_source: ManualEntry`, a non-`UserSpace` definition `source`, and non-empty `extra_headers` matching the parent's
- `cargo nextest run -p mcpmux-core` targeted on `server` / clone tests

**Outcome:** A freshly cloned `posthog-personal-mesh`-equivalent starts with the parent's `Authorization`/`x-posthog-project-id` values already in `extra_headers` (user still swaps the project id), and its Definition editor no longer falsely claims to be `UserSpace`-editable.

### Phase 2 — DB-backed definition edit path (~1 day)

- `update_definition()` on `ServerAppService`; reject if `installation_source != ManualEntry`
- New Tauri command + admin bridge command mirroring `update_server_in_config`
- `ServerDefinitionModal` / `ServersPage`: editability gate + save dispatch on `installation_source`
- `pnpm typecheck && pnpm lint`, `cargo clippy --workspace -- -D warnings`

**Outcome:** Opening the Definition editor on any `manual_entry` clone (not just PostHog ones) allows edit + save, persisting to `installed_servers.cached_definition` instead of 404ing on `spaces/*.json`.

### Phase 3 — Pool invalidation on config/definition change (~half day)

- `ServerConfigUpdated` subscriber in the gateway pool: evict enabled server's instance
- `UserSpaceSyncService`: emit `ServerConfigUpdated` alongside `update_cached_definition()`
- Implement `LiveGatewayWriteRuntime::retry_connection`
- Regression test: simulate a config update while an instance is pooled+healthy, assert next connect rebuilds transport from DB without a manual `retry_connection` call

**Outcome:** Saving a clone's headers (via Configure, the new Definition editor, or a file-sync adoption) takes effect on the next call without requiring an app relaunch — reproducing the original bug's fix without the raw-SQL + relaunch workaround.

### Phase 4 — Footgun warning UI (~half day)

- Enable/connect-time check: clone's `extra_headers` empty for keys the parent/definition declares as required, non-blocking banner
- Clone wizard: surface header fields inline at creation time if not already covered by Phase 1's seeding
- i18n strings for the warning

**Outcome:** A clone left with genuinely empty required headers surfaces a visible warning instead of connecting silently against the wrong (or missing) credentials.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`crates/mcpmux-core/src/application/server.rs`](../../crates/mcpmux-core/src/application/server.rs) | `clone_server()` L374-443 (what is/isn't copied), `update_config()` L217-269 (existing override save path, unaffected by this fix) |
| [`crates/mcpmux-core/src/domain/server.rs`](../../crates/mcpmux-core/src/domain/server.rs) | `ServerSource` enum L80, `source` field L38 — inherited on clone today |
| [`crates/mcpmux-core/src/domain/installed_server.rs`](../../crates/mcpmux-core/src/domain/installed_server.rs) | `InstallationSource` enum L77-83 (`Registry` \| `UserConfig` \| `ManualEntry`), `source` field L159 |
| [`crates/mcpmux-core/src/domain/event.rs`](../../crates/mcpmux-core/src/domain/event.rs) | `ServerConfigUpdated` L200 — currently UI-only, no gateway consumer |
| [`crates/mcpmux-core/src/application/user_space_sync.rs`](../../crates/mcpmux-core/src/application/user_space_sync.rs) | `update_cached_definition()` calls L154-162 — silent today, no event emission |
| [`crates/mcpmux-gateway/src/pool/transport/resolution.rs`](../../crates/mcpmux-gateway/src/pool/transport/resolution.rs) | `build_transport_config()` L47-127 — confirms no parent/`cloned_from` lookup exists; `extra_headers` merge is the last step (L127) |
| [`crates/mcpmux-gateway/src/pool/service.rs`](../../crates/mcpmux-gateway/src/pool/service.rs) | `connect_server()` L267-297 — healthy-instance reuse (`reused: true` L291) skips config reload unless evicted first; `remove_instance()` L344 |
| [`apps/desktop/src-tauri/src/commands/server_manager.rs`](../../apps/desktop/src-tauri/src/commands/server_manager.rs) | `retry_connection()` L512-545 — existing evict-then-reconnect pattern this fix generalizes via the event subscriber |
| [`crates/mcpmux-gateway/src/admin/write_runtime.rs`](../../crates/mcpmux-gateway/src/admin/write_runtime.rs) | `retry_connection` stub L181-182 (`LiveGatewayWriteRuntime`), L407-409 (no-gateway fallback) |
| [`apps/desktop/src-tauri/src/commands/space.rs`](../../apps/desktop/src-tauri/src/commands/space.rs) | `update_server_in_config` error site L322 — the exact "not found in config" message users hit today |
| [`crates/mcpmux-gateway/src/admin/command_bridge/space.rs`](../../crates/mcpmux-gateway/src/admin/command_bridge/space.rs) | Same error, web-admin bridge copy, L182 |
| [`apps/desktop/src/components/ServerDefinitionModal.tsx`](../../apps/desktop/src/components/ServerDefinitionModal.tsx) | `isEditable` gate L80 — root of the false-affordance bug |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | `canEditDefinition` L1967; `retryConnectionV2` call sites L1087,1184,1221,1446,1472; `server-changed` handler L522 (doesn't yet react to `config_updated`) |
| `~/Library/Application Support/com.mcpmux.desktop/mcpmux.db`, `installed_servers` table | `posthog-personal-mesh` (broken, empty `extra_headers`) vs `posthog-personal-gait` (working, both headers set) — reference rows for any migration/validation logic |
| [Diagnostic session transcript](08ac92fe-f240-4cd1-a0d3-755f654cb613) | Jul 21/22 debugging session that produced the raw-SQL workaround this fix replaces |

---

## Related Documentation

- [`dev-to-main-port.md`](./dev-to-main-port.md) — original clone lineage work (migration 021, `cloned_from`), Phase 6 web-admin clone/display-name stubs this fix partially pulls forward
- [`user-config-sync-collision-fix.md`](./user-config-sync-collision-fix.md) — separate `UserSpaceSyncService` bug fixed same layer; this doc's Phase 3 touches the same `sync_from_file` file for the `ServerConfigUpdated` emission
- [`dev-rebased-post-port-completion.md`](./dev-rebased-post-port-completion.md) — QA checklist item "Clone account — independent config" this fix is meant to finally satisfy
