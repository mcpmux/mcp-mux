# User Config Sync — Cross-Source ID Collision Fix

**Last Updated:** Jul 7, 2026
**Status:** Planning — not started
**Depends on:** Write-only save flow + file watcher sync (this session, uncommitted — `save_space_config` write-only, `SpaceFileWatcher`, `space-servers-updated`/`space-servers-sync-failed` events)
**Unblocks:** Custom Server Configuration edits actually landing in the installed server list when a server ID overlaps with a registry-sourced install

---

## Problem

`UserSpaceSyncService::sync_from_file` (`crates/mcpmux-core/src/application/user_space_sync.rs`) scopes its "does this server already exist" check to rows from **this file only** (`list_by_source_file`). It has no visibility into rows installed from other sources (registry, manual entry) that happen to share the same `server_id`.

When a file entry's ID collides with a different-source row, the sync loop calls `installed_repo.install()` (a plain `INSERT`), which trips the `UNIQUE(space_id, server_id)` constraint. The `?` on that call aborts the whole `sync_from_file` pass immediately — every definition after the failing one in that iteration is never processed, added, or updated.

Confirmed live in `/Users/joe/Library/Application Support/com.mcpmux.desktop/logs/mcpmux.2026-07-07.log`: a 30-entry custom config file has ~8 IDs (`home-assistant`, `render`, `firebase-dev`, `n8n-mcp`, `langfuse`, `inngest-local`, `typesense`, `posthog-work`) that already exist as `registry`-sourced installs. Every save triggers a sync that dies on whichever of those IDs comes first in that pass's iteration order (`definitions` order is not stable — logs show a different failing ID nearly every run). `home-assistant-new`, a genuinely new entry with no collision, never gets installed because it keeps landing after whatever collides that round.

This isn't an isolated bug for one server — it silently blocks sync progress for the entire file on almost every save whenever any registry/user-config ID overlap exists.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Collision handling | **Adopt on conflict** — when a file entry's ID matches an existing row from a different source, convert that row's `source` to `UserConfig { file_path }` and overwrite its config via `update()`, instead of `install()` | Matches user intent: editing `home-assistant` in the JSON should update *the* `home-assistant` server. `update()` already persists `source`, `cached_definition`, `server_name` — no new repo method needed. |
| 2 | Loop behavior | **Continue on error** — one failing definition no longer aborts the pass; collect per-item errors and keep processing the rest | Matches the actual complaint: unrelated servers in the same file shouldn't be held hostage by one bad entry. |
| 3 | Visibility into cross-source collisions | Look up via existing `get_by_server_id(space_id, server_id)` before deciding insert vs. update vs. adopt, instead of only consulting the file-scoped `existing_map` | Trait method already exists (`crates/mcpmux-core/src/repository/mod.rs:89`, impl at `crates/mcpmux-storage/src/repositories/installed_server_repository.rs:320`). No schema or trait change required. |
| 4 | Surfacing adoptions | Add `adopted: Vec<String>` to `SyncResult`; not silent — logged at `info!` and included in the `space-servers-updated` payload so the frontend can toast "N servers took over an existing entry" if desired | Options brainstorm (pure silent adopt) was rejected for being too quiet about a source-changing mutation; full confirmation-dialog gating (deferred option) was rejected as disproportionate effort for the immediate bug. |
| 5 | Partial-failure reporting | Add `errors: Vec<(String, String)>` (id, message) to `SyncResult`; `sync_from_file` returns `Ok(result)` even when some items failed, unless **every** item failed | A file with 29 good entries and 1 truly broken one (e.g. malformed URL) should still sync the 29. `space-servers-sync-failed` is now only emitted when nothing could be synced at all; `space-servers-updated` payload carries partial-error info for a softer toast. |
| 6 | Frontend event payload | Extend `SpaceServersUpdatedPayload` with optional `adopted_count?: number` and `error_count?: number` for a richer (but still single) toast; keep `SpaceServersSyncFailedPayload` for the all-failed case | Reuses the event plumbing already wired this session (`useDomainEvents.ts`, `ServersPage.tsx`) instead of adding new channels. |

---

## Scope

**In:**
- `UserSpaceSyncService::sync_from_file`: per-item try/continue instead of `?`-abort; `get_by_server_id` lookup for cross-source adoption; `SyncResult.adopted` / `SyncResult.errors` fields
- `lib.rs` event emission: pass adopted/error counts through `space-servers-updated`; only emit `space-servers-sync-failed` when the sync call itself errors (file unreadable/unparseable) or every item failed
- Frontend: extend payload types, adjust toast copy to mention partial adoption/errors when present
- Unit tests in `user_space_sync.rs` covering: adopt-on-conflict, continue-past-one-failure, all-fail-still-reports

**Out:**

| Item | Reason |
| ---- | ------ |
| Confirmation dialog before adopting a cross-source server | Deferred (Option 4 from brainstorm) — disproportionate UX work for the immediate bug; revisit if adoption surprises users in practice |
| Schema field (`"replace": true`) to opt into adoption per-entry | Deferred alongside the dialog option — same reasoning |
| Auto-suffixing colliding IDs instead of adopting | Rejected in brainstorm — unstable naming, confuses gateway tool-prefixing |
| Bootstrap/startup sync (currently file-watcher-only) | Separate concern flagged in `docs/planning/istauri-audit.md`; not required to fix this collision bug |
| Registry version-tracking fields (`pinned_version`, etc.) reconciliation on adopt | Adoption simply carries over whatever the file specifies; no attempt to preserve registry version metadata across the source change |

---

## Architecture

### Current loop (abort-on-first-error)

```rust
for definition in definitions {
    let server_id = definition.id.clone();

    if let Some(existing_server) = existing_map.get(&server_id) {
        // update...
        self.installed_repo.update_cached_definition(...).await
            .with_context(...)?;   // ← aborts whole sync on first error
        result.updated.push(server_id);
    } else {
        // install...
        self.installed_repo.install(&installed).await
            .with_context(...)?;   // ← this is what fails on cross-source collision
        result.added.push(server_id);
    }
}
```

### New loop (continue-on-error, adopt-on-conflict)

```rust
for definition in definitions {
    let server_id = definition.id.clone();

    let outcome = if let Some(existing_server) = existing_map.get(&server_id) {
        // Same file already owns this row — refresh as before.
        let cached_def = serde_json::to_string(&definition).ok();
        self.installed_repo
            .update_cached_definition(&existing_server.id, Some(definition.name.clone()), cached_def)
            .await
            .map(|_| SyncOutcome::Updated)
    } else if let Some(mut other_source) = self
        .installed_repo
        .get_by_server_id(space_id, &server_id)
        .await
        .unwrap_or(None)
    {
        // Row exists under a different source — adopt it into this file.
        other_source.source = InstallationSource::UserConfig { file_path: file_path.to_path_buf() };
        other_source.cached_definition = serde_json::to_string(&definition).ok();
        other_source.server_name = Some(definition.name.clone());
        self.installed_repo
            .update(&other_source)
            .await
            .map(|_| SyncOutcome::Adopted)
    } else {
        let installed = InstalledServer::new(space_id, &server_id)
            .with_definition(&definition)
            .with_source(InstallationSource::UserConfig { file_path: file_path.to_path_buf() })
            .with_enabled(true);
        self.installed_repo.install(&installed).await.map(|_| SyncOutcome::Added)
    };

    match outcome {
        Ok(SyncOutcome::Added) => result.added.push(server_id),
        Ok(SyncOutcome::Updated) => result.updated.push(server_id),
        Ok(SyncOutcome::Adopted) => result.adopted.push(server_id),
        Err(e) => result.errors.push((server_id, e.to_string())),
    }
}

// After the loop: only bail if literally nothing succeeded.
if result.added.is_empty() && result.updated.is_empty() && result.adopted.is_empty() && !result.errors.is_empty() {
    anyhow::bail!("All {} servers failed to sync: {:?}", result.errors.len(), result.errors);
}
```

`SyncOutcome` is a small internal enum (`Added | Updated | Adopted`) — not exposed outside this module.

### `SyncResult` additions

```rust
#[derive(Debug, Default)]
pub struct SyncResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
    pub adopted: Vec<String>,           // new
    pub errors: Vec<(String, String)>,  // new — (server_id, error message)
}

impl SyncResult {
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.updated.is_empty()
            || !self.removed.is_empty() || !self.adopted.is_empty()
    }
}
```

### Event payload changes

```typescript
// useDomainEvents.ts
export interface SpaceServersUpdatedPayload extends DomainEventPayload {
  space_id: string;
  adopted_count?: number;
  error_count?: number;
}
```

`lib.rs` only emits `space-servers-sync-failed` when `sync_from_file` returns `Err` (i.e. total failure or unreadable/unparseable file) — the same as today. Partial failures now surface as a softer note attached to the success toast instead of the warning toast.

---

## Files to Modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-core/src/application/user_space_sync.rs`](../../crates/mcpmux-core/src/application/user_space_sync.rs) | Continue-on-error loop; `get_by_server_id` cross-source lookup; adopt-on-conflict branch; `SyncResult.adopted`/`errors`; bail only when everything failed; new unit tests |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Thread `adopted.len()` / `errors.len()` into the `space-servers-updated` payload |
| [`apps/desktop/src/lib/backend/events/useDomainEvents.ts`](../../apps/desktop/src/lib/backend/events/useDomainEvents.ts) | Extend `SpaceServersUpdatedPayload` with `adopted_count?` / `error_count?` |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | On `space-servers-updated`, if `adopted_count`/`error_count` present, show an info toast summarizing partial results in addition to reloading the list |
| [`apps/desktop/src/locales/en/servers.json`](../../apps/desktop/src/locales/en/servers.json) | New `toast.syncPartial*` strings (e.g. "Config synced — 2 servers took over existing entries, 1 failed") |

---

## Phases

### Phase 1 — Backend: continue-on-error + adopt-on-conflict (~half day)

- Add internal `SyncOutcome` enum and rewrite the add/update loop in `sync_from_file` per Architecture above
- Add `adopted: Vec<String>` and `errors: Vec<(String, String)>` to `SyncResult`; update `has_changes()`
- Change the post-loop error condition to bail only when every item failed
- Unit tests: adopt-on-conflict (file entry ID matches a `Registry`-sourced row → row's source flips to `UserConfig`, no DB error), continue-past-failure (one malformed entry among several valid ones → valid ones still land, error captured), all-fail-still-errors (every entry collides in a way that can't be resolved → `sync_from_file` returns `Err`)
- Run `cargo nextest run -p mcpmux-core` targeted on `user_space_sync`

**Outcome:** Re-running `sync_from_file` against the current on-disk config (`00000000-0000-0000-0000-000000000001.json`) adopts `home-assistant`, `render`, `firebase-dev`, `n8n-mcp`, `langfuse`, `inngest-local`, `typesense`, `posthog-work` into `UserConfig` source and installs `home-assistant-new` (and any other net-new entries) without aborting. `cargo nextest run --workspace` and `cargo clippy --workspace -- -D warnings` pass clean.

---

### Phase 2 — Event payload + frontend toast (~quarter day)

- Extend `SpaceServersUpdatedPayload` in `useDomainEvents.ts` with `adopted_count?` / `error_count?`
- Thread the counts through in `lib.rs` where `space-servers-updated` is emitted
- `ServersPage.tsx`: when either count is > 0, append a short info toast alongside the existing reload (don't block or replace the reload)
- Add `servers.json` i18n strings for the partial-result toast

**Outcome:** Saving the Custom Server Configuration with a mix of new, adopted, and (if any) genuinely broken entries reloads the list, installs/adopts everything it can, and shows one toast summarizing what happened instead of a bare "sync failed" with no detail. `pnpm typecheck && pnpm lint` pass clean.

---

## Key Files Referenced

| File | Notes |
| ---- | ----- |
| [`crates/mcpmux-core/src/application/user_space_sync.rs`](../../crates/mcpmux-core/src/application/user_space_sync.rs) | Target file — current abort-on-first-error loop, `ensure_unique_server_ids`, `SyncResult` |
| [`crates/mcpmux-core/src/repository/mod.rs`](../../crates/mcpmux-core/src/repository/mod.rs) | `InstalledServerRepository` trait — `get_by_server_id` (already exists, line 89), `update` (line 99, persists `source`) |
| [`crates/mcpmux-storage/src/repositories/installed_server_repository.rs`](../../crates/mcpmux-storage/src/repositories/installed_server_repository.rs) | Confirms `update()` writes `source` column (line 408) — adoption needs no new repo method |
| [`crates/mcpmux-storage/src/migrations/001_initial.sql`](../../crates/mcpmux-storage/src/migrations/001_initial.sql) | `UNIQUE(space_id, server_id)` constraint — the actual trigger for the collision |
| [`apps/desktop/src-tauri/src/services/file_watcher.rs`](../../apps/desktop/src-tauri/src/services/file_watcher.rs) | Sole caller of `sync_from_file`; 500ms debounce; where `Sync failed for ...` is logged today |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Emits `space-servers-updated` / `space-servers-sync-failed` after watcher callback |
| [`apps/desktop/src/lib/backend/events/useDomainEvents.ts`](../../apps/desktop/src/lib/backend/events/useDomainEvents.ts) | `SpaceServersUpdatedPayload` / `SpaceServersSyncFailedPayload` types added this session |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | Subscribes to both sync events; where the partial-result toast gets added |
| `/Users/joe/Library/Application Support/com.mcpmux.desktop/logs/mcpmux.2026-07-07.log` | Live evidence — rotating `Failed to install server: <id>` errors across `render`, `firebase-dev`, `home-assistant`, `n8n-mcp`, `langfuse`, `inngest-local`, `typesense`, `posthog-work` |

---

## Related Documentation

- [`istauri-audit.md`](./istauri-audit.md) — flags the same file-watcher-only sync as a separate gap (no bootstrap sync on app launch); out of scope here but worth revisiting together
- Prior session (uncommitted at time of writing): write-only `save_space_config`, `SpaceFileWatcher` debounce, `space-servers-updated`/`space-servers-sync-failed` event wiring this fix extends
