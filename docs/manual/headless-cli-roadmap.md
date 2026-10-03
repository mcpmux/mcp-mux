# Headless daemon and CLI roadmap

## Purpose

Deliver a supported Linux headless deployment of McpMux without removing the
desktop application. The result is a long-running `mcpmuxd` process plus an
operator CLI, `mcpmux-cli`, which retains the current product model:

- one Streamable HTTP MCP gateway;
- Spaces, Feature Sets, server definitions, and workspace bindings;
- routing by MCP roots and the `X-Mcpmux-Workspace` override;
- encrypted credentials, inbound-client authentication, and outbound OAuth;
- local-by-default network exposure.

The desktop app remains a client of the same domain, storage, and gateway
libraries. It must continue to work against existing user data.

## Scope and product boundaries

### Included in the first supported release

- Linux daemon operation under `systemd`.
- An offline-safe operator CLI for configuration and lifecycle operations.
- A local control socket for commands while the daemon is running.
- Existing Spaces, Feature Sets, workspace bindings, enabled servers, and
  Streamable HTTP MCP routing.
- File-backed encryption keys on headless Linux, with strict ownership and
  permissions.
- A terminal-friendly approval and OAuth flow.
- Health checks, structured logs, graceful shutdown, and database migration.

### Explicitly deferred

- A browser dashboard or replacement web UI.
- Multi-user or multi-tenant hosting on one Unix account.
- LAN/public exposure by default.
- A new configuration format that replaces the database.
- Windows service and macOS launchd support.
- Changes to the server registry or server-definition repository.

The first release is a **single Unix user / single host** service. Multiple
people or untrusted clients require a separate threat model and are not an
incremental extension of this scope.

## Current assets to reuse

The implementation should reuse, not fork, these existing layers:

| Capability | Existing implementation | Headless work |
| --- | --- | --- |
| MCP gateway, pooling, filtering, OAuth middleware | `mcpmux-gateway` | Instantiate it from a daemon bootstrapper. |
| Spaces, Feature Sets, workspace routing | `mcpmux-core` + `mcpmux-storage` | Expose equivalent CLI operations. |
| SQLite repositories and migrations | `mcpmux-storage` | Make data directory and locking explicit. |
| Encrypted fields | `mcpmux-storage` | Select the file key provider in headless mode. |
| File key fallback | `keychain_file.rs` | Harden and document its use; do not create a plaintext-secret mode. |
| Startup auto-connect and graceful gateway handle | `mcpmux-gateway` | Own its lifecycle in `mcpmuxd`. |
| Domain events | `mcpmux-core` | Forward to logs and the control-socket event stream. |

`GatewayServer::new` is already dependency-injected and documented as usable
from Desktop, CLI, and tests. The principal gap is the Desktop bootstrap and
the Tauri-only interaction flows around it.

## Target architecture

```text
                     local operator
                          |
                    mcpmux-cli
                          |
              Unix control socket (0600)
                          |
                     mcpmuxd daemon
                  /       |        \
       SQLite + keys   gateway       event/log stream
                       /mcp
                         |
                 MCP clients and servers
```

### Process and paths

- `mcpmuxd` is the only writer to a live data directory.
- `mcpmux-cli` connects to `$XDG_RUNTIME_DIR/mcpmux/control.sock`; the socket is
  owned by the service user and mode `0600`.
- Default persistent data is `$XDG_STATE_HOME/mcpmux` (normally
  `~/.local/state/mcpmux`), configurable via `--data-dir`.
- Logs go to journald by default. `--log-dir` may enable rotating files for
  hosts without persistent journald.
- The gateway binds to `127.0.0.1:45818` unless explicitly configured
  otherwise. Remote binding is not part of the first milestone.

### Bootstrap extraction

Create a Rust library crate, proposed name `mcpmux-runtime`, which owns:

1. data-directory validation, exclusive lock, and migration;
2. key-provider selection;
3. SQLite/database/repository initialization;
4. creation of `ApplicationServices` and `GatewayDependencies`;
5. gateway startup, shutdown, and event subscription.

Both `apps/desktop/src-tauri` and `apps/daemon` call this crate. Tauri must
not be imported by `mcpmux-runtime`, `mcpmux-core`, `mcpmux-gateway`, or
`mcpmux-storage`.

### Control protocol

Do not let the CLI write the SQLite database directly while the daemon is
running. That risks stale in-memory gateway state and concurrent writes.

Start with a versioned JSON request/response protocol over the Unix socket:

```json
{"version":1,"request_id":"...","method":"spaces.list","params":{}}
```

Responses include a stable machine-readable `code`, a human-readable
`message`, and optional `data`. Add an event subscription method for
`mcpmux-cli logs --follow` and future `mcpmux-cli approvals watch`.

The socket authenticates the Unix peer UID. It is not an HTTP API and must not
be exposed by a reverse proxy.

## CLI contract (MVP)

The CLI must support both `--output json` and concise human output. Commands
that change state require explicit arguments; destructive actions require
`--yes` or an interactive confirmation.

```text
mcpmux-cli daemon status
mcpmux-cli health
mcpmux-cli doctor
mcpmux-cli logs [--follow] [--server ID]

mcpmux-cli spaces list|create|delete|set-default
mcpmux-cli spaces base-dirs list|add|remove
mcpmux-cli feature-sets list|get|create|update|delete|include|remove
mcpmux-cli registry list|search
mcpmux-cli servers list|inspect|features|add|configure|enable|disable|remove|auth
mcpmux-cli workspaces list|bind|unbind

mcpmux-cli clients list|create|delete

mcpmux-cli config export|export-space|import|validate
mcpmux-cli workspace config
```

`servers add` installs from the registry; `servers configure --file` accepts a
JSON object with `inputs`, `env`, `args`, and `headers`. Every mutating command
supports `--output json` and `--yes`. The binary is named `mcpmux-cli` because
the desktop application already owns the `mcpmux` product/binary name.

`workspaces bind` takes an absolute canonical path, a Space, and one or more
Feature Sets (`--feature-set`, repeatable). It must preserve the existing
longest-prefix Space base-directory resolution and exact workspace-binding
behavior.

## Delivery phases

Each phase is independently reviewable. Do not begin a later phase while its
acceptance checks are red.

## Pull request sequence

Land the work in small, independently reviewable pull requests. Keep the
Windows process-lifecycle fixes separate from the headless work because they
are production bug fixes with no dependency on the daemon architecture.

1. Windows gateway/process shutdown and Windows E2E cleanup.
2. This RFC plus `mcpmux-runtime`, including the smallest Desktop bootstrap
   refactor needed to adopt it.
3. `mcpmuxd` lifecycle: Linux bootstrap, health check, data-directory lock,
   and graceful shutdown.
4. `mcpmux-control` and the basic `mcpmux-cli` commands that communicate over
   the local control socket.
5. Remaining management commands, systemd installation, and operational
   documentation.

Each PR must state its supported platforms. The initial daemon and CLI release
is Linux/Unix only; Windows service and macOS launchd support remain deferred.

### Phase 0 — RFC and compatibility contract

**Goal:** remove ambiguity before changing the workspace structure.

Tasks:

- Publish an RFC covering target user, threat model, paths, socket protocol,
  data ownership, credential provider, and supported OAuth behavior.
- Freeze the initial CLI grammar and JSON output envelope.
- Define compatibility rules: Desktop and daemon cannot concurrently use the
  same `--data-dir`; both may migrate the same historical database format.
- Decide the package names and service identity (`mcpmuxd`, `mcpmux`,
  `mcpmux.service`).
- Add a test matrix for Ubuntu/Debian headless environments.

Acceptance:

- Maintainers approve the RFC.
- The RFC names an owner and a rollback plan for every persistent-data change.
- No schema migration is introduced in this phase.

### Phase 1 — Shared runtime bootstrap

**Goal:** start the existing gateway without Tauri.

Tasks:

- Add `mcpmux-runtime` and move only environment-neutral bootstrap code into
  it.
- Implement explicit XDG path resolution and `--data-dir` validation.
- Add an exclusive process lock with an actionable error that identifies the
  owning PID when available.
- Initialize SQLite, repositories, encryption, discovery, logging, and
  `GatewayDependencies` through the shared runtime.
- Add a minimal `apps/daemon` binary with `serve`, SIGTERM/SIGINT handling,
  gateway `run_with_shutdown`, and `/health` verification.
- Refactor Desktop to use the shared bootstrap without changing Desktop
  behavior.

Acceptance:

- A daemon starts an empty database and responds on `/health`.
- A Desktop smoke test still starts against a fresh data directory.
- Integration tests boot Desktop and daemon separately against the same
  fixture, never at the same time.
- No Tauri type is referenced from the new runtime crate.

### Phase 2 — Safe headless secrets and service lifecycle

**Goal:** make a daemon safe and operable before adding management commands.

Tasks:

- Add an explicit `--key-provider=auto|keychain|file` policy. On a headless
  host, `auto` selects the existing file provider; `keychain` fails clearly if
  unavailable.
- Verify data, key directory, key files, database, and socket permissions at
  startup. Refuse unsafe ownership or group/world-readable key material.
- Add journald-friendly structured logging and redaction tests.
- Add `mcpmux.service` with `Restart=on-failure`, an unprivileged service user,
  `StateDirectory=mcpmux`, `RuntimeDirectory=mcpmux`, and conservative
  filesystem/network hardening compatible with stdio MCP servers.
- Add `mcpmux-cli daemon status` and `mcpmux-cli doctor` checks for port, lock,
  database, keys, and configured server executables.

Acceptance:

- Service restarts cleanly and retains the same encrypted credentials.
- A key file with unsafe permissions makes startup fail, not silently repair.
- `systemctl stop mcpmux` drains the listener and releases port 45818.
- Secret values never appear in logs, errors, or CLI JSON output.

### Phase 3 — Control socket and read-only CLI

**Goal:** establish one authoritative process and an automation-friendly
operator interface.

Tasks:

- Implement the versioned Unix-socket server in the daemon.
- Implement handshake, UID authorization, request IDs, timeouts, and protocol
  version negotiation.
- Expose read-only methods: status, health, Spaces, Feature Sets, servers,
  bindings, clients, and sanitized logs.
- Implement `--output json`, deterministic exit codes, and shell completion.
- Forward relevant domain events to the socket event stream.

Acceptance:

- `mcpmux-cli spaces list --output json` is stable enough for scripts.
- A different Unix user cannot read or control the daemon.
- Killing a CLI process cannot stop or corrupt the daemon.
- Integration tests cover incompatible protocol versions and malformed input.

### Phase 4 — Configuration and workspace mapping mutations

**Goal:** achieve the Spaces/mapping workflow without a GUI.

Tasks:

- Add mutation commands for Spaces, Feature Sets, servers, Space base
  directories, and workspace bindings.
- Route all mutations through existing application services and repository
  traits; emit domain events and refresh active gateway state.
- Implement transactional config export/import with a schema version,
  redacted-by-default exports, validation, dry run, and backup before import.
- Add `mcpmux-cli workspace config --path PATH --client CLIENT` to print the
  per-workspace MCP client snippet, including the workspace header when
  required.

Acceptance:

- Binding `/srv/repos/acme` to a Space and Feature Set changes the tool list
  for a session reporting that root without restarting the gateway.
- The explicit workspace header has the same precedence as Desktop.
- Import rejects malformed or secret-bearing exports before touching storage.
- Existing Desktop data displays identically after a headless mutation.

### Phase 5 — Headless consent and OAuth

**Goal:** replace the current Tauri-only approval boundary without weakening
security.

The current production code intentionally exposes no HTTP consent endpoint;
approval is Tauri IPC only. This phase must not replace that protection with
an unauthenticated HTTP endpoint.

Tasks:

- Introduce a persistent pending-approval model with expiry, target Space,
  requested action, and audit metadata. Store no secret in the pending record.
- Create an `ApprovalPublisher` abstraction. Desktop implements it using
  Tauri events; daemon implements it through the control socket and CLI.
- Implement `mcpmux-cli approvals watch`, `approve`, and `deny`; approval requires
  a matching pending ID and records an audit event.
- Support inbound-client OAuth consent through that approval abstraction.
- For outbound OAuth, have `mcpmux-cli servers auth ID` print an authorization URL
  and state/expiry. Support loopback callback plus documented SSH port
  forwarding. Add device authorization only for providers that actually offer
  it; do not emulate it for standard authorization-code providers.
- Define an expiry and recovery procedure for an interrupted authorization.

Acceptance:

- A new client remains denied until a local operator approves its specific
  pending request.
- Reusing, guessing, or approving an expired pending ID fails.
- OAuth authorization can complete from a remote administrator workstation via
  a documented SSH tunnel.
- Desktop continues to show the existing approval dialog.

### Phase 6 — Packaging, migration, and release readiness

**Goal:** make installation and upgrades boring.

Tasks:

- Package `mcpmuxd`, `mcpmux`, the systemd unit, man pages, completions, and
  an example environment file in Debian packages.
- Add installation, upgrade, rollback, backup, and disaster-recovery guides.
- Decide and implement a supported Desktop-to-daemon migration command that
  stops one process before transferring ownership of the data directory.
- Add release CI for a headless Ubuntu integration job: install package, start
  service, configure a fixture server via CLI, bind workspace, call MCP,
  restart service, and re-call MCP.
- Run security review for socket permissions, file-key mode, OAuth redirects,
  log redaction, and systemd hardening.

Acceptance:

- A clean Ubuntu VM can install, configure, and restart the daemon using only
  documentation and CLI commands.
- Upgrade preserves data and encrypted credentials; downgrade/rollback is
  explicitly documented.
- The release artifact contains no desktop/WebKit dependency.

## Test strategy

Add tests beside each layer rather than relying on end-to-end coverage alone.

| Layer | Required coverage |
| --- | --- |
| `mcpmux-runtime` | path resolution, locks, migrations, provider selection, shutdown |
| Control protocol | auth by UID, version mismatch, invalid input, timeout, event stream |
| CLI | argument validation, JSON schema, exit codes, confirmation behavior |
| Gateway | existing Space and workspace resolver tests, plus runtime bootstrap |
| Approval/OAuth | expiry, replay refusal, approval audit, remote callback procedure |
| Service package | install/start/restart/stop and persistence in a headless VM |

Use a disposable data directory per test. Never point tests at a developer's
Desktop data directory or real credential store.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Desktop and daemon corrupt shared state | exclusive data-dir lock; one writer; explicit migration command |
| CLI becomes a second business-logic implementation | control socket calls application services; no direct SQLite writes live |
| Headless mode weakens OAuth consent | persistent one-time approvals over a Unix socket; no public consent route |
| File keys are copied or exposed | 0700 key directory, 0600 files, startup permission checks, documented backup policy |
| stdio server needs host tools | run service as the intended operator; document `PATH`, working directory, and environment allowlist |
| Public binding exposes management APIs | keep loopback default; defer remote binding; separate management socket from MCP listener |
| Migration traps users | database backup, explicit locking, dry run, and recovery guide |

## Implementation order for the first pull requests

1. RFC + workspace/package skeleton only.
2. `mcpmux-runtime` bootstrap with tests; switch Desktop to it.
3. `mcpmuxd serve` and graceful lifecycle integration tests.
4. Key-provider policy and systemd unit.
5. Read-only control socket + CLI.
6. Mutating CLI for Spaces, servers, Feature Sets, and workspace bindings.
7. Approval broker abstraction and headless inbound consent.
8. Outbound OAuth CLI flow.
9. Packaging, migration tool, full VM acceptance suite.

Each PR should be narrow, feature-gated where necessary, and leave Desktop
behavior unchanged. The daemon must not be advertised as supported until
Phases 1 through 5 pass their acceptance criteria.

## Decisions required before Phase 1

1. Is the initial target a personal server under one Unix account, as assumed
   here, or a multi-user/shared service?
2. Must the daemon be usable remotely from day one, or is SSH access to the
   host sufficient for initial operation and OAuth approval?
3. Is file-backed key storage acceptable for the target hosts, or must the
   first release integrate a secret manager such as systemd credentials/Vault?
4. Should Desktop and daemon share a data directory through an explicit
   handoff command, or should they start as separate profiles with import and
   export only?
