# Headless daemon and CLI — RFC

**Status:** Proposed (PR #1 of the headless-cli-roadmap phases)
**Owner:** McpMux maintainers
**Operations:** [headless-daemon-operations.md](./headless-daemon-operations.md)
**Related:** [`headless-cli-roadmap.md`](./headless-cli-roadmap.md)

## Purpose

Add a Linux headless deployment of McpMux without removing the desktop application:
a long-running `mcpmuxd` daemon plus an operator CLI (`mcpmux`). The result
must not regress any desktop behaviour and must continue to work against
existing user data.

## Resolved decisions

The roadmap lists four open questions. This PR resolves them as follows.

### Decision 1 — Single Unix user / single host

**Resolved: yes, single Unix user / single host.**

The first supported release is exactly what the roadmap assumes: one operator
account, one daemon, one data directory. Multi-user or shared-host hosting
require a different threat model and are not an incremental extension of this
work; they remain out of scope.

### Decision 2 — Remote access from day one

**Resolved: SSH access to the host is sufficient for initial operation.**

The gateway binds only to `127.0.0.1`. Remote operators reach the daemon over
SSH (config, logs, OAuth approval, restart). Network exposure is deferred to
a future, separately reviewed phase.

### Decision 3 — Credential provider

**Resolved: file-backed keys on headless Linux, OS keychain on macOS, DPAPI on
Windows.**

`mcpmux-runtime` reuses the existing provider selection from
`mcpmux_storage::create_key_provider`. On Linux with no Secret Service
available it falls back to `FileKeyProvider` (already 0700/0600-permissioned).
A `--key-provider` flag (Phase 2) will let the operator force the policy.

Headless servers should prefer the file provider explicitly when no desktop
session exists, so the daemon does not write to a per-user Secret Service
that may not be available across SSH logins.

### Decision 4 — Desktop and daemon data directory

**Resolved: separate profiles; explicit `mcpmux-cli config export|import` for
handoff. Concurrent access to the same data directory is forbidden.**

The runtime acquires an exclusive `flock` on `<data_dir>/mcpmux.lock` at
startup. The lock records the owning PID. If the desktop or the daemon is
already holding the lock, the other process exits with an actionable error
identifying the owner — never silently repairs or removes the lockfile.

The single supported way to move configuration between desktop and daemon is
the existing `ConfigExporter` (export) and a matching import command (Phase 4
CLI work). The roadmap defers a live handoff command.

## Target architecture

The runtime crate (`mcpmux-runtime`) owns the bootstrap that the desktop
currently inlines in `apps/desktop/src-tauri/src/state/mod.rs`:

1. Path resolution (`--data-dir` → `$XDG_STATE_HOME/mcpmux` fallback).
2. Exclusive data-dir lock with PID-stamped error.
3. Key-provider selection (auto now, `--key-provider` in Phase 2).
4. SQLite + migrations + 11 repository handles + `FieldEncryptor`.
5. `GatewayPortService` + port probe + first-run persistence.
6. `ServerDiscoveryService` + `ServerLogManager`.
7. `GatewayDependencies` for `GatewayServer::new`.
8. Logging init (extracted from `apps/desktop/src-tauri/src/lib.rs`).
9. Optional single shared `EventBus` bridged from the gateway's broadcast.

Both `apps/desktop/src-tauri` and the new `apps/daemon` call into the same
crate. No Tauri dependency is allowed in `mcpmux-runtime`,
`mcpmux-core`, `mcpmux-gateway`, or `mcpmux-storage` — verified before the
PR lands by `cargo tree -p mcpmux-runtime | grep tauri`.

## Compatibility rules

- **No concurrent data-dir access.** Enforced by the exclusive lock at
  startup. Both desktop and daemon read the same database schema; lock
  violation is a hard exit, not a warning.
- **No schema migration in this PR.** The 22 existing migrations remain
  authoritative; the runtime uses `Database::open(&db_path)` exactly like
  the desktop today.
- **No new key provider in this PR.** The daemon exposes the existing
  `auto|keychain|file` providers explicitly; it does not add a storage format.
  `keychain` fails rather than falling back, and `file` selects the existing
  owner-only file provider on Unix.
- **Desktop behaviour is unchanged.** The desktop refactor is a pure
  rearrangement: `AppState::new` delegates to `mcpmux-runtime::RuntimeBuilder`,
  the duplicated gateway construction in `lib.rs` and `commands/gateway.rs`
  collapses to a single helper, and all Tauri-specific seams (approval
  publisher, domain-event bridge, file watcher) remain in the desktop.

## Package and service identity

| Item | Name |
| --- | --- |
| Daemon binary | `mcpmuxd` |
| CLI binary (Phase 3) | `mcpmux-cli` (package `mcpmux-cli`; the desktop app already owns the `mcpmux` binary/product name) |
| Runtime crate | `mcpmux-runtime` |
| Control protocol crate | `mcpmux-control` |
| systemd unit | `mcpmux.service` |
| Control socket | `$XDG_RUNTIME_DIR/mcpmux/control.sock` (0600) |
| Data dir | `$XDG_STATE_HOME/mcpmux` (or `--data-dir`) |
| Logs dir | `<data-dir>/logs` (or `--log-dir`) |
| Default port | `127.0.0.1:45818` (existing `DEFAULT_GATEWAY_PORT`) |

## Phase boundaries

This RFC originally covered **Phase 0 + Phase 1**. The workspace now also
contains the Phase 2 (doctor), Phase 3 and Phase 4 implementations:

- Phase 2 (headless secrets and lifecycle) — **partially implemented**:
  `mcpmux-cli doctor` audits data dir, lock, key-file permissions, database,
  listener, registry reachability, and server executables. Permission checks
  are enforced by `doctor` (`FAIL` on world-readable keys) rather than
  refusing startup; startup refusal remains Phase 2 work.
- Phase 3 (control socket, CLI, read-only commands) — **implemented**:
  `mcpmux-control` (versioned wire protocol) + the daemon's Unix control
  socket + `mcpmux-cli` read-only commands.
- Phase 4 (mutating CLI, config export/import, workspace snippet) —
  **implemented**: Space / FeatureSet / server / base-dir / workspace-binding
  mutations, `config export|export-space|import|validate`, and
  `workspace config`.

Still NOT covered:

- Remaining Phase 2 hardening: startup permission refusal, log retention, and
  the systemd hardening directives.
- Phase 5 (approval broker abstraction, headless inbound consent).
- Phase 6 (packaging, migration tool, release CI).

## Linux service installation

On Linux, install and start a per-user service with:

    mcpmuxd service install --key-provider file

The command writes the user systemd mcpmux.service unit, runs systemctl
--user daemon-reload, enables the unit, and restarts it. The generated unit
preserves all supplied daemon options and always binds the gateway to loopback.

The daemon is **not advertised as supported** until Phases 1–5 land. PRs
marking a phase complete must reference the acceptance criteria in the
roadmap.

## Test matrix

| Layer | Required coverage | Where |
| --- | --- | --- |
| `mcpmux-runtime` | path resolution, lock acquire/conflict, migrations, key-provider, GatewayDependencies build, shutdown | `tests/rust/tests/runtime/` |
| `mcpmux-control` | frame round-trip, oversized-frame rejection, stable method names, error envelope | `crates/mcpmux-control/src/lib.rs` unit tests |
| `mcpmuxd` control socket | spawn → socket ready → ping/status, protocol-version mismatch, unknown method, live event stream → SIGTERM → clean exit | `apps/daemon/tests/daemon.rs` |
| Desktop smoke | `AppState::new` against fixture data-dir still produces a working state | existing desktop tests + `tests/rust/tests/runtime/bootstrap.rs` |

Per the roadmap: every test uses a disposable `TempDir`. No test points at a
developer's real data directory or credential store.

## Rollback plans

| Change | Rollback |
| --- | --- |
| New `mcpmux-runtime` crate | Revert commit; desktop `AppState::new` falls back to inlined bootstrap. |
| New `mcpmux-control` crate + `apps/cli` | Delete the crate/`apps/cli` + remove from root `Cargo.toml`; the daemon compiles without the control socket module. |
| `apps/daemon` binary | Delete directory + remove from root `Cargo.toml`. |
| Desktop delegating to runtime | Keep both code paths behind a feature flag? No — `mcpmux-runtime` is the only supported path going forward. Revert the whole PR. |
| `tests/rust/tests/runtime/` | Remove the directory; no behavioural impact. |

## Out of scope

- Windows service support (the daemon binary compiles, but service management
  is not part of this PR).
- macOS launchd support.
- Public dashboards, multi-tenant hosting, LAN exposure by default.
- A new configuration format that replaces the database.
- Changes to the server registry or server-definition repository.

## Open questions deferred

- Should the daemon expose a Prometheus-style `/metrics` endpoint?
  **Deferred** until a real consumer is identified (no Phase 1–5 requirement).
- Should the runtime own the file watcher that the desktop uses to keep
  `spaces/<uuid>.json` in sync with `installed_server` rows? **Deferred** to
  Phase 4 — the watcher's notification callback is Tauri-coupled and the
  daemon has no UI to notify.
