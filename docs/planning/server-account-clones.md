# Server Account Clones (UI-Assisted Multi-Account)

**Last Updated:** May 23, 2026
**Status:** In progress — Phases 1–4 complete, Phase 5 optional
**Branch:** `feat/server-account-clones`
**Base branch:** `main`
**Issue:** TBD — file after planning review
**Depends on:** None (orthogonal to session meta-tools; benefits from but does not require PR #154)
**Unblocks:** Personal MCP migration (`jsg-tech-check/docs/setup/mcpmux-server-migration.md`) — Gmail, Sheets, PostHog ×2, Firebase ×4, and other single-account stdio servers

---

## Problem

McpMux installs servers once per `(space_id, server_id)` — enforced by `UNIQUE(space_id, server_id)` on `installed_servers`, `credentials`, and `outbound_oauth_clients`. That model works when one Space maps to one account (Personal vs S2H vs GAIT), but breaks down when a user needs **two accounts for the same MCP in the same Space**:

| Server type | Example need | Current workaround |
| ----------- | ------------ | ------------------ |
| Native multi-account | Google Workspace (`user_google_email` per call) | One install — works today |
| Single-account stdio | PostHog personal + work in Personal Space | Hand-edit user space JSON with suffixed IDs (`posthog-personal`, `posthog-work`) |
| Single-account OAuth HTTP | Two Notion workspaces in one Space | Same JSON hack or split Spaces |
| Env-at-startup servers | Firebase ×4 projects | Four manual JSON entries with different env paths |

The workaround **works** — custom entries with unique IDs get separate processes, credential rows, and tool prefixes — but it is undiscoverable, error-prone, and regresses the "click Install" UX. Users migrating from a 40-entry `~/.cursor/mcp.json` hit this immediately.

Spaces remain the canonical answer for **context-level** separation (work vs personal repos). This feature targets **account-level** duplication inside a Space when context splitting is wrong or insufficient.

---

## Decisions

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Primary approach | **Option 2: UI-assisted clone** — new `server_id` + `manual_entry`, no schema migration | Highest value-to-effort. Removes JSON-editing pain without touching OAuth/credential layer mid-migration. |
| 2 | Schema change | **Defer Option 3** (`instance_label` column) to optional Phase 5 | Architecturally cleaner long-term, but 2–3 weeks of migration risk across 5+ tables. Ship clones first; revisit when migration volume justifies it. |
| 3 | Clone identity | **`{base_server_id}-{suffix}`** where suffix is user-chosen (default suggestions: `work`, `personal`, `prod`) | Satisfies unique constraint. Hyphen suffix only — underscores are stripped by `normalize_server_id` and reserved as the tool-name delimiter. |
| 4 | Definition source | **Copy `cached_definition` from source install** into clone at creation time | Clone is self-contained for offline/gateway startup. Registry updates do not auto-propagate — acceptable tradeoff for v1; document in UI. |
| 5 | Prefix / alias | **Auto-set alias = suffix** (e.g. `posthog-work` → tools prefixed `posthog-work_*`) | Reuses `PrefixCacheService` first-come assignment. User can override alias in configure step. |
| 6 | Credentials | **Never copy secrets** — clone starts with empty `input_values` / no OAuth; user configures fresh | Prevents accidental credential sharing. Clone wizard opens configure flow immediately after create. |
| 7 | Source tracking | **`InstallationSource::ManualEntry`** + optional `cloned_from: Option<String>` metadata on `InstalledServer` | Distinguishes registry installs from clones in UI (`SourceBadge`). `cloned_from` is display-only in v1 — not a FK. |
| 8 | Registry dedup UX | **"Add another account" disabled when source is already a clone-of-clone** (max depth 1) or when suffix collision detected | Prevents unbounded ID sprawl (`posthog-work-work-work`). Clones clone from registry/original only. |
| 9 | Spaces unchanged | **No change to Space model** — clone is per-Space like any install | Work/personal split via Spaces stays documented as primary pattern; clones are the escape hatch. |

---

## The Model

### What a clone is

A clone is a **new `InstalledServer` row** in the same Space as the source, with:

```text
InstalledServer {
  server_id:     "{base_id}-{suffix}",     // e.g. "posthog-work"
  server_name:   "{display} ({suffix})",   // e.g. "PostHog (work)"
  cached_definition: <copy from source>,
  input_values:  {},                       // empty — user fills in configure step
  source:        ManualEntry,
  cloned_from:   Some("{base_id}"),        // new optional field, v1 display-only
  enabled:       false,                    // same default as registry install
}
```

Prefix resolution treats the clone as an independent server. Tool names become `{alias}_{tool}` (e.g. `posthog-work_capture_event`).

### What a clone is NOT

- Not a second OAuth session on the same `server_id` row
- Not a runtime credential swap (stdio env is fixed at process spawn)
- Not a registry duplicate — the registry still has one definition for `posthog`; clones are local installs

### Composition with existing patterns

```text
Multi-account need?
├─ MCP has per-call account param (Google Workspace)
│   └─ ONE install — no clone needed
├─ Accounts map to repo context (Personal / S2H / GAIT)
│   └─ Spaces — no clone needed
└─ Two+ accounts in SAME Space, single-account MCP
    └─ Clone via "Add another account" (this feature)
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  My Servers UI                                                  │
│                                                                 │
│  [PostHog ▼]  Connected                                       │
│    ├─ Configure / Logs / …                                      │
│    └─ "Add another account…"  ─────────────────────┐            │
└────────────────────────────────────────────────────│────────────┘
                                                     │
                                                     ▼
                              ┌──────────────────────────────────┐
                              │  CloneAccountModal                 │
                              │  • suffix input (work/personal/…)  │
                              │  • alias preview (posthog-work)    │
                              │  • collision check                 │
                              └──────────────────────────────────┘
                                                     │
                                                     ▼
                              ┌──────────────────────────────────┐
                              │  ServerAppService::clone_server  │
                              │  1. validate unique server_id    │
                              │  2. copy cached_definition       │
                              │  3. set alias in definition JSON │
                              │  4. install as ManualEntry       │
                              │  5. emit ServerInstalled         │
                              └──────────────────────────────────┘
                                                     │
                    ┌────────────────────────────────┴───────────────┐
                    ▼                                                ▼
         installed_servers row                          credentials row(s)
         (new server_id)                                (empty until configure)
                    │
                    ▼
         PrefixCache assigns alias on first enable/connect
                    │
                    ▼
         Separate stdio process / OAuth flow with clone's creds
```

- **No gateway routing changes** — clone is a distinct `server_id`; existing prefix cache + routing already handle multiple servers in one Space.
- **FeatureSets** see clones as separate servers — user adds `posthog-work` to a FeatureSet independently. Future Option 3 could add "all instances of server X" grouping.
- **Meta tools** (`mcpmux_enable_server`) already accept any `server_id` string — clones work with session enable once the user knows the suffixed ID. Phase 3 adds optional `cloned_from` hint in `mcpmux_list_servers` response.

---

## Files to create

| File | Purpose |
| ---- | ------- |
| `apps/desktop/src/features/servers/CloneAccountModal.tsx` | Suffix input, alias preview, collision feedback, submit → Tauri command |
| `apps/desktop/src/lib/api/serverClone.ts` | TS wrappers: `cloneServer`, `suggestCloneSuffix`, `isCloneIdAvailable` |
| `apps/desktop/src-tauri/src/commands/server_clone.rs` | Tauri commands delegating to `ServerAppService::clone_server` |
| `tests/rust/tests/integration/server_clone.rs` | Clone creates distinct install, empty creds, unique prefix, collision rejection |
| `docs/planning/server-account-clones.md` | This doc |

## Files to modify

| File | Change |
| ---- | ------ |
| [`crates/mcpmux-core/src/domain/installed_server.rs`](../../crates/mcpmux-core/src/domain/installed_server.rs) | Add optional `cloned_from: Option<String>`. Builder `with_cloned_from`. |
| [`crates/mcpmux-core/src/application/server.rs`](../../crates/mcpmux-core/src/application/server.rs) | `clone_server(space_id, source_server_id, suffix, alias_override?)` — copy definition, derive new ID, install as `ManualEntry`. |
| [`crates/mcpmux-storage/src/repositories/installed_server_repository.rs`](../../crates/mcpmux-storage/src/repositories/installed_server_repository.rs) | Serialize/deserialize `cloned_from` (new nullable column or JSON in existing row — see Phase 1). |
| [`crates/mcpmux-storage/src/migrations/`](../../crates/mcpmux-storage/src/migrations/) | New migration: `cloned_from TEXT` nullable on `installed_servers`. |
| [`apps/desktop/src/features/servers/ServerActionMenu.tsx`](../../apps/desktop/src/features/servers/ServerActionMenu.tsx) | Add "Add another account…" action; hidden for clones-of-clones. |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | Wire modal, group clones visually under source (optional Phase 2 polish). |
| [`apps/desktop/src/components/SourceBadge.tsx`](../../apps/desktop/src/components/SourceBadge.tsx) | Badge variant for cloned servers ("Clone of posthog"). |
| [`apps/desktop/src-tauri/src/lib.rs`](../../apps/desktop/src-tauri/src/lib.rs) | Register `clone_server`, `suggest_clone_suffix`, `is_clone_id_available` commands. |
| [`crates/mcpmux-gateway/src/services/meta_tools/tools.rs`](../../crates/mcpmux-gateway/src/services/meta_tools/tools.rs) | (Phase 3) Optional `cloned_from` field in `mcpmux_list_servers` payload. |
| [`docs/guide/servers.mdx`](../../docs/guide/servers.mdx) | Document multi-account patterns: Spaces vs native params vs clones. |

---

## Phasing

### Phase 1 — Core clone API + storage

**Effort:** ~1 day

- [x] Migration: `cloned_from TEXT NULL` on `installed_servers`
- [x] `InstalledServer.cloned_from` field + repo round-trip
- [x] `ServerAppService::clone_server`:
  - Load source install + definition from `cached_definition`
  - Derive `new_id = "{base}-{suffix}"` using same normalization as `UserServerEntry::normalize_server_id`
  - Reject if `(space_id, new_id)` exists or source is missing
  - Patch definition `alias` to suffix (or user override)
  - Install via existing `install()` path with `ManualEntry` + `with_cloned_from(source_id)`
- [x] Unit tests: happy path, collision, missing source, suffix normalization (no underscores)
- [x] Tauri command `clone_server(space_id, source_server_id, suffix, alias?)`

**Outcome:** `clone_server` from Tauri creates a disabled `posthog-work` install with copied definition, empty creds, and `cloned_from = "posthog"`. Verifiable via `list_installed_servers` and SQLite inspection. No UI yet.

### Phase 2 — Clone wizard UI

**Effort:** ~1 day

- [x] `CloneAccountModal` — suffix field with suggestions (`work`, `personal`, `prod`, `staging`), live alias preview, inline collision error
- [x] `ServerActionMenu` → "Add another account…" on registry and manual installs (not on clones)
- [x] Post-clone flow: open existing `ConfigEditorModal` for credential entry before enable
- [x] `SourceBadge` shows clone lineage
- [ ] Optional: collapsed "Accounts" group on `ServersPage` when `cloned_from` matches same base (visual only, no schema)

**Outcome:** User clicks "Add another account" on PostHog, enters suffix `work`, gets `posthog-work` card in My Servers, configures API key, enables — tools appear as `posthog-work_*` in gateway. No JSON editing.

### Phase 3 — Meta-tool + docs surfacing

**Effort:** ~0.5 day

- [x] `mcpmux_list_servers` returns optional `cloned_from` for clone rows
- [x] `docs/guide/servers.mdx` section: "Multiple accounts" — decision tree (Spaces / native param / clone)
- [ ] Migration doc update in `jsg-tech-check` with concrete clone targets (PostHog, Gmail, Sheets, Firebase) — out of repo; deferred

**Outcome:** LLM manifest shows clone lineage. Docs explain when to clone vs use a Space. Migration checklist has explicit suffix naming convention.

### Phase 4 — Validation + edge cases

**Effort:** ~0.5 day

- [x] Integration test: two clones in one Space, distinct prefixes, both connect with different env
- [x] Uninstall clone does not affect source
- [x] Uninstall source warns if clones exist (list dependents, offer bulk uninstall)
- [x] Prefix collision: two different registry servers cannot claim same alias (existing behavior — verify clones don't break it)
- [x] `pnpm validate` + targeted Rust/TS tests

**Outcome:** Clone lifecycle is safe through install → configure → enable → uninstall. Source/uninstall warnings prevent orphaned expectations.

### Phase 5 — (Optional) First-class instances (Option 3)

**Effort:** ~2–3 weeks — **defer until clone UX proves demand**

- [ ] Schema: replace `UNIQUE(space_id, server_id)` with `UNIQUE(space_id, server_id, instance_label)` on `installed_servers`, `credentials`, `outbound_oauth_clients`, `server_features`
- [ ] `instance_label` default `"default"` for existing rows; migration backfills
- [ ] UI: one registry card with N instance sub-cards instead of flat clone list
- [ ] FeatureSet member type: `ServerInstance { server_id, instance_label }` for grouped grants
- [ ] Data migration: existing clones (`posthog-work`) → `(posthog, instance_label=work)`
- [ ] Log paths, OAuth refresh, event payloads gain instance dimension

**Outcome:** Registry server is the template; instances are first-class. Clone IDs like `posthog-work` become legacy format migrated to structured instances. Only pursue if Phase 1–4 adoption shows ID-suffix sprawl or FeatureSet pain.

---

## Out of scope

| Item | Reason |
| ---- | ------ |
| Auto-sync clone definition when registry updates | Requires shared definition store or periodic refresh job. Defer; document "clone may drift from registry" in UI. Option 3 addresses properly. |
| Credential copy / "duplicate with same secrets" | Security footgun. User always re-enters creds on clone. |
| Runtime account switching on one process | Impossible for stdio env-at-startup servers. Not McpMux's layer to fix. |
| Wrapper MCP shims per backend | Per-server maintenance burden (Option 5 from brainstorm). Rejected. |
| Cross-Space clone | Install separately per Space — already works via Spaces. "Clone to another Space" is a nice follow-up, not v1. |
| Tool-level account selection injection | Would require MCP spec / client header support. Out of scope. |

---

## Key files referenced

| File | Why |
| ---- | --- |
| [`crates/mcpmux-core/src/application/server.rs`](../../crates/mcpmux-core/src/application/server.rs) | `install()` uniqueness check — clone must use a new `server_id`. |
| [`crates/mcpmux-core/src/domain/config.rs`](../../crates/mcpmux-core/src/domain/config.rs) | `normalize_server_id` / `normalize_alias` — suffix rules (no underscores). |
| [`crates/mcpmux-storage/src/migrations/001_initial.sql`](../../crates/mcpmux-storage/src/migrations/001_initial.sql) | Current `UNIQUE(space_id, server_id)` constraints clone works around. |
| [`crates/mcpmux-gateway/src/services/prefix_cache.rs`](../../crates/mcpmux-gateway/src/services/prefix_cache.rs) | Prefix assignment for clone's alias at connect time. |
| [`apps/desktop/src/features/servers/ServersPage.tsx`](../../apps/desktop/src/features/servers/ServersPage.tsx) | Primary UI surface for install/configure/enable flow. |
| [`apps/desktop/src/components/ConfigEditorModal.tsx`](../../apps/desktop/src/components/ConfigEditorModal.tsx) | Reused post-clone credential entry. |
| [`docs/guide/spaces.mdx`](../../docs/guide/spaces.mdx) | Canonical work/personal separation — clones complement, not replace. |

---

## Related documentation

- [`docs/planning/dynamic-mcp-toggle-meta-tools.md`](./dynamic-mcp-toggle-meta-tools.md) — session enable works with clone `server_id`s once user knows the suffixed name; Phase 3 links them.
- [`docs/guide/servers.mdx`](../../docs/guide/servers.mdx) — server management baseline; gets multi-account section in Phase 3.
- [`docs/guide/spaces.mdx`](../../docs/guide/spaces.mdx) — primary pattern for context-level account separation.
- Personal migration tracker: `jsg-tech-check/docs/setup/mcpmux-server-migration.md` — consuming checklist for PostHog, Gmail, Sheets, Firebase clones.

---

## Reconciliation

This doc is the source of truth for server account clones. When implementation starts, update **Status** and **Branch** at the top. Phase 5 remains optional — do not block Phases 1–4 on it.

**Decision record (May 23, 2026):** Option 2 (UI-assisted clone) selected over status quo, first-class instances (deferred Phase 5), per-client credential override (rejected), and wrapper meta-servers (rejected). Brainstorm source: Cursor session on multi-account MCP patterns.
