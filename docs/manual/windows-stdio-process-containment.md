# Windows stdio process containment

Technical record of how McpMux launches stdio MCP servers on Windows, why the
obvious approach does not work, and how to verify changes to it.

Scope: `crates/mcpmux-gateway/src/pool/transport/stdio.rs` and the equivalent
spawn in `crates/mcpmux-mcp/src/transports.rs`.

## The symptom

Two separate problems were reported on Windows:

1. Every stdio MCP server popped open a terminal window.
2. MCP server processes survived the app closing.

They looked like one bug but have different causes, and fixing the first
naively makes it worse.

## Why only `npm` showed a terminal

`npx` resolves to `npx.cmd`. Windows `CreateProcess` implicitly runs a batch
file through `cmd.exe /e:ON /v:OFF /d /c <file>`, and `cmd.exe` is a
console-subsystem application. `uv`, `python` and `analytics-mcp` resolve to
`.exe` files. So the visible terminal is specific to batch-file wrappers.

Seeing it only on `npm` is the expected fingerprint of this class of bug, and is
a useful confirmation signal.

## Why `CREATE_NO_WINDOW` alone was not enough

`CREATE_NO_WINDOW` (`0x08000000`) is correct: it stops the child being given a
console at all. It was not reaching `CreateProcess`, because
`process-wrap`'s `JobObject` wrapper builds on it.

`JobObject` must guarantee that a child is attached to the job _before_ it can
spawn descendants, so it creates the process with `CREATE_SUSPENDED`, attaches
it, then resumes it. On Windows that resume allocates the child's console
**with a visible window**, which defeats `CREATE_NO_WINDOW`.

Measured on Windows 11 with Windows Terminal installed as the default console
host, repeated across runs:

| variant                                                     | visible terminal windows |
| ----------------------------------------------------------- | ------------------------ |
| `CREATE_NO_WINDOW` alone                                    | 0                        |
| `CREATE_NO_WINDOW` + `JobObject`                            | 1                        |
| `CREATE_NO_WINDOW` + kill-on-close job attached after spawn | 0                        |

A consequence: `DETACHED_PROCESS` makes it worse, not better. It is explicitly
ignored in combination with `CREATE_NO_WINDOW` and gives the child its own
console.

`STARTF_USESHOWWINDOW` with `SW_HIDE` is also not the answer. Unlike
`CREATE_NO_WINDOW`, which denies the child a console, it creates a real console
that is merely hidden, adding a console host per child.

## The approach that works

Attach the job to the already-running child instead of suspending it first. The
console stays suppressed, and the tree is still terminated on drop.

`JobTree` in `stdio.rs` does this: it creates a job with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and in `wrap_child` assigns the child's
process handle to it. The wrapper owns the handle, so dropping the transport
tears down the whole tree — which is what makes `npx -> cmd -> node` die as a
unit rather than leaving a `node` grandchild behind.

### Accepted trade-off

Between `CreateProcess` and `AssignProcessToJobObject` the child is briefly
running outside any job. A fast wrapper could in principle create a descendant
in that window that never joins the job and would survive when the handle
closes.

Closing this race requires attaching before execution, i.e. `CREATE_SUSPENDED`,
which is exactly what brings the terminal window back. The trade is taken
deliberately: the window is sub-millisecond, and the wrappers that would
exploit it need tens of milliseconds to start their own child.

## Verifying window suppression

This is easy to get wrong, and doing so will make you believe a broken build is
fixed.

**`conhost.exe` is not a signal.** Every console-subsystem child gets a console
host, whether or not a window is ever shown. A child having a `conhost.exe`
child proves nothing.

**`PseudoConsoleWindow` geometry is not a signal either.** The child-side
pseudoconsole window is reported as `0x0 @ (0,0)` regardless of whether a
terminal is visible, so a zero-sized `PseudoConsoleWindow` is not proof that
nothing is displayed.

Check the actual top-level window instead. Enumerate `ConsoleWindowClass` and
`CASCADIA_HOSTING_WINDOW_CLASS` windows via `EnumWindows`, and compare
`IsWindowVisible` **and** the window rectangle against a known-visible
terminal. A real terminal reports something like `1129x635 @ (96,104)`; a
suppressed console host reports `0x0`.

When Windows Terminal is the default console host, a console allocation is
delegated to it and surfaces as a new `CASCADIA_HOSTING_WINDOW_CLASS` window,
so that class has to be watched too.

**Rule out other terminals by control.** A terminal window belonging to another
application looks identical in the enumeration. Kill the app under test and
confirm the window survives; if it does, it was never yours.

## Process cleanup paths

All three paths that end a stdio connection must release the pool, or MCP
processes outlive the gateway:

- quit via the tray (_Salir_) — `apps/desktop/src-tauri/src/lib.rs`
- explicit stop — `apps/desktop/src-tauri/src/commands/gateway.rs`
- **restart** — the same file, `restart_gateway`

`restart_gateway` originally only took the server handle. The old pool is kept
alive by an `Arc` held by the detached OAuth handler in `init_gateway_runtime`,
so without an explicit pool shutdown every restart leaked the previous stdio
processes while starting a second set. All three paths now call
`PoolService::shutdown()`.

`PoolService::shutdown()` also guards against being raced by `connect_server`: a
`shutting_down` flag gates new connections, the drain loops until the instance
map stays empty, and a connection that completes during a shutdown reaps its own
instance.

When verifying orphan behaviour, exercise **restart** as well as quit. A
shutdown-path fix can look correct if only the quit path is tested.

## Verifying orphan cleanup

Snapshot every pid in the app's process tree, perform the action, then check
that none survive.

Beware the false positive: server processes whose parent belongs to a _different_
application are not leaks. On a machine with several MCP clients running, most
`uv` / `python` / `node` processes belong to Claude, Codex, or another manager.
Confirm the parent pid is actually dead before calling something orphaned.

## Cargo manifest trap

A platform-gated table placed in the middle of `[dependencies]` silently
captures every key below it. This block:

```toml
rmcp.workspace = true

[target.'cfg(windows)'.dependencies]
process-wrap.workspace = true
windows = "0.62.2"

# OAuth
oauth2 = "5"
mcpmux-core.workspace = true
```

makes `oauth2`, `mcpmux-core` and `mcpmux-storage` **Windows-only**, and Linux
CI then fails with `cannot find module or crate`. Always place
`[target.'cfg(windows)'.dependencies]` after every shared dependency.

This class of bug is invisible on a Windows dev box and invisible to
`cargo check` on Windows. Verify by parsing the manifest and inspecting the
table, not by reading it:

```python
import tomllib
d = tomllib.load(open("crates/mcpmux-gateway/Cargo.toml", "rb"))
print(sorted(d.get("target", {}).get("cfg(windows)", {}).get("dependencies", {})))
# expected: ['process-wrap', 'windows']
```

## Related CI failure

`cargo clippy --workspace -- -D warnings` currently fails on `main` with 80
`clippy::double_must_use` errors in `crates/mcpmux-core/src/repository/mod.rs`.
The GitHub runner moved to Rust 1.99 and the lint now fires on the existing
`#[async_trait]` repository traits. It is unrelated to this work; it needs a
separate fix (pin the toolchain, or allow the lint on those traits).

## Known follow-ups

- The Windows job implementation is duplicated between `mcpmux-gateway` and
  `mcpmux-mcp` rather than routed through the shared
  `configure_child_process_platform()` helper that `AGENTS.md` mandates.
  Extracting it is a worthwhile separate change.
- `process-wrap` is declared as `process-wrap = "9"` in the workspace root while
  the crates also pin `windows = "0.62.2"` directly. Worth consolidating.
