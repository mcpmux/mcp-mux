# Windows stability fixes and pull request plan

This document records the Windows stability work, its review boundaries, and
the recommended order for landing the headless daemon and CLI work.

## Windows pull requests

The two Windows fixes are intentionally separate from each other and from the
headless work. They are safe to review and merge independently.

### PR #229: Windows gateway and child-process shutdown

PR: <https://github.com/mcpmux/mcp-mux/pull/229>

When the Desktop application exits from the tray, it now releases its pooled
backend connections before stopping the gateway. Releasing those connections
drops the stdio transports cleanly rather than leaving them alive after the
gateway listener is gone.

On Windows, each stdio MCP process is also placed in a Windows Job Object with
the `KILL_ON_JOB_CLOSE` limit. Closing the transport therefore terminates the
whole process tree. This matters for wrapper commands such as `npx`, which can
otherwise leave a `node` grandchild running after McpMux has exited.

> **Correction (2026-10-02).** An earlier revision of PR #229 used
> `process-wrap`'s `JobObject` wrapper directly and claimed `CREATE_NO_WINDOW`
> was retained, so no terminal flashed. That claim was wrong and was disproved
> by measurement. `JobObject` creates the child suspended and resumes it, and
> that resume allocates the child's console *with a visible window*, defeating
> `CREATE_NO_WINDOW` — every stdio server still popped open a terminal. The
> job is now attached to the already-running child instead of suspending it.
> PR #229 adds `docs/manual/windows-stdio-process-containment.md`, which
> records the full investigation, the measurements, and the traps involved.

Validation completed on Windows:

```text
cargo check --locked -p mcpmux -p mcpmux-gateway -p mcpmux-mcp
cargo clippy --locked -p mcpmux -p mcpmux-gateway -p mcpmux-mcp --all-targets -- -D warnings
cargo test --locked -p mcpmux-gateway -p mcpmux-mcp --lib
```

The focused Rust test run completed with 154 passing tests.

`cargo clippy` on `main` currently fails with 80 pre-existing
`clippy::double_must_use` errors in `mcpmux-core` after the CI runner moved to
Rust 1.99. That is unrelated to PR #229 and needs a separate fix on `main`.

### PR #230: isolate Desktop E2E state and process cleanup

PR: <https://github.com/mcpmux/mcp-mux/pull/230>

Desktop E2E now uses a temporary, test-only application data directory. The
test runner sets `MCPMUX_E2E_TEST` and `MCPMUX_E2E_DATA_DIR`; production runs
continue to use the normal operating-system application-data directory.

The runner also starts mock servers directly instead of through a shell. On
Windows it terminates their complete process trees, and removes stale
`tauri-driver` instances before opening a session. These changes prevent
orphaned mocks/drivers and ensure a test never deletes or replaces a
developer's profile.

Validation completed on Windows:

```text
pnpm prettier --check tests/e2e/wdio.conf.ts
pnpm typecheck
cargo check -p mcpmux
```

The full desktop E2E suite still reports pre-existing functional failures in
Spaces, workspace mappings, and UI expectations. Those failures are outside
the cleanup change and should be investigated in dedicated fixes.

## Review and merge order

1. Review and merge PR #229 first. It is the production shutdown fix and has
   no dependency on the test harness.
2. Review and merge PR #230 next. It prevents E2E runs from modifying a local
   profile and makes Windows cleanup deterministic.
3. Rebase each remaining branch immediately before merge if `main` advances.
   Keep the two fixes separate; neither needs the daemon or CLI changes.

## Headless daemon and CLI rollout

The CLI is not ready as one large pull request. It spans runtime bootstrap,
daemon lifecycle, a local control protocol, CLI commands, tests, and Linux
operations. Splitting it keeps each review focused and makes rollback safe.

Recommended sequence:

1. The two Windows fixes above.
2. The architecture RFC and `mcpmux-runtime`, with only the necessary Desktop
   bootstrap refactor.
3. `mcpmuxd` lifecycle: Linux startup, data-directory lock, health endpoint,
   and graceful shutdown.
4. `mcpmux-control` and the read-only `mcpmux-cli` commands over the local
   Unix control socket.
5. Mutating CLI commands, systemd installation, packaging, and the remaining
   operational documentation.

The initial headless product is Linux/Unix only. Windows service management
and macOS `launchd` support are explicitly deferred. Each CLI/daemon PR must
state its supported platforms and must not make the Desktop and daemon share a
data directory concurrently.

For the full design and operational contract, see:

- [Headless daemon and CLI RFC](./headless-cli-rfc.md)
- [Headless daemon and CLI roadmap](./headless-cli-roadmap.md)
- [Headless daemon operations](./headless-daemon-operations.md)

## PR description template

Use this short structure when opening each follow-up pull request:

```markdown
## Summary

- Describe one user-visible change.
- Describe the safety or compatibility boundary.

## Validation

- Exact command and result.

## Scope

- Supported platforms.
- Explicitly deferred work and known unrelated test failures, if any.
```

Do not include secrets, exported credentials, local profile paths containing
personal data, or binaries in pull requests.
