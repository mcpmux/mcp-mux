# Headless daemon operations

This guide records the current Linux deployment path for the McpMux headless
gateway. It applies to the daemon, its systemd user-service installer, and the
`mcpmux-cli` operator CLI.

## Current scope

The daemon runs a Streamable HTTP gateway on loopback only:

    http://127.0.0.1:45818

It owns one SQLite data directory, including encrypted credentials and
file-backed encryption keys. The Desktop app and daemon must never run against
the same data directory at the same time; startup refuses the second process
through an exclusive lock.

The deployed profile in this guide is:

    /home/leo/.local/state/mcpmux

Use another absolute path with --data-dir when a separate profile is required.

## Build

Build the production binaries from the repository root:

    cargo build --release -p mcpmuxd -p mcpmux-cli

The resulting executables are:

    target/release/mcpmuxd
    target/release/mcpmux-cli

## Install as a user service

The daemon installs a user-level systemd unit. It does not require a root
system service and does not expose the gateway on the LAN or Internet.

First, ensure user services survive logout and reboot:

    loginctl show-user "$USER" -p Linger

If the value is not yes, an administrator can enable it once:

    sudo loginctl enable-linger "$USER"

Install, enable, and start the service:

    target/release/mcpmuxd service install \
      --data-dir "$HOME/.local/state/mcpmux" \
      --key-provider file \
      --port 45818

The command writes the unit to:

    $XDG_CONFIG_HOME/systemd/user/mcpmux.service

or, when XDG_CONFIG_HOME is unset:

    $HOME/.config/systemd/user/mcpmux.service

It then runs systemctl --user daemon-reload, enables the unit, and restarts it.
The generated unit uses the exact executable that ran service install and
preserves supplied options such as --data-dir, --port, --registry-url,
--log-dir, --log-filter, and --public-base-url.

Use --key-provider file on a headless Linux host. It keeps master and JWT keys
under the selected data directory with owner-only file permissions.

## Verify and operate

Check the HTTP health endpoint:

    curl --fail http://127.0.0.1:45818/health

Expected response:

    {"status":"ok","version":"0.5.0"}

Run the deployment doctor through the daemon (data dir, exclusive lock, key
permissions, database, listener, registry reachability, and whether each
configured stdio server command resolves on PATH):

    mcpmux-cli doctor

It exits `0` when healthy and `2` when any check fails; `--output json` emits
the full report for monitoring.

Check service state and logs:

    systemctl --user status mcpmux.service
    journalctl --user -u mcpmux.service -f

Manage the service:

    systemctl --user restart mcpmux.service
    systemctl --user stop mcpmux.service
    systemctl --user start mcpmux.service

Confirm that the socket remains loopback-only:

    ss -ltnp '( sport = :45818 )'

The listening address must be 127.0.0.1:45818. Do not use
--auth-disabled in a persistent deployment.

## Remote operator access

Keep the gateway private and tunnel it from an operator workstation:

    ssh -N -L 45818:127.0.0.1:45818 user@server

Then, on the workstation:

    curl --fail http://127.0.0.1:45818/health

The tunnel does not start the daemon. It only forwards the already-running
loopback port over the authenticated SSH connection.

## Upgrade from this repository

After changing daemon code, rebuild and run the installer again with the same
operational flags:

    cargo build --release -p mcpmuxd
    target/release/mcpmuxd service install \
      --data-dir "$HOME/.local/state/mcpmux" \
      --key-provider file \
      --port 45818

The installer updates the unit to the newly built executable and restarts it.
Verify the health endpoint afterward.

## Adding an MCP server by CLI

The `mcpmux-cli` operator CLI talks to the running daemon over a local Unix
control socket (`$XDG_RUNTIME_DIR/mcpmux/control.sock`, mode `0600`). It never
opens `mcpmux.db` directly, so the in-memory gateway state, domain events,
encryption flow, and exclusive-lock discipline are always respected. The CLI
requires the daemon to be running; if the socket is absent it exits with an
actionable error instead of touching storage.

The CLI derives the socket path from `--data-dir` exactly like the daemon, so
pass the same `--data-dir` you used for `service install`:

    export MCPMUX_DATA_DIR="$HOME/.local/state/mcpmux"

The workflow to install and enable a registry server is:

    # Browse the catalog to find the registry-server-id.
    mcpmux-cli registry list
    mcpmux-cli registry search github

    # Install from the registry into a Space (defaults to the default Space).
    mcpmux-cli servers add <registry-server-id> --space <space-id>

    # Provide inputs, env overrides, extra args, or headers from a JSON file.
    mcpmux-cli servers configure <registry-server-id> --file config.json

    # Enable and connect it.
    mcpmux-cli servers enable <registry-server-id>

    # Grant its discovered tools to clients by adding them to a Feature Set.
    mcpmux-cli feature-sets include <feature-set-id> --server <registry-server-id>

`registry list` prints the servers available to install (id, name, transport,
auth, and an `[installed]` marker); `registry search QUERY` and
`registry list --query Q --category C` filter it, and `--refresh` bypasses the
5-minute cache. The catalog comes from the daemon's registry feed
(`MCPMUX_REGISTRY_URL`, default `https://api.mcpmux.com`).

`server_id` is the registry id shown by `mcpmux-cli servers list` (for example
`com.cloudflare/bindings-mcp`); the installation is scoped by `--space`, which
defaults to the default Space. `servers configure --file` accepts a JSON object
with any of `inputs`, `env`, `args`, and `headers`; omitted fields are left
unchanged. Secret **values** are never echoed by `servers inspect`.

The `feature-sets include` step matters: connecting a server caches its tools,
but a client only sees the tools its resolved Feature Set contains. The empty
"Starter" Feature Set that ships with each Space is the fallback for unmapped
folders, so including a server there grants it to every unmapped client in that
Space:

    # Default Space's Starter:
    #   fs_default_00000000-0000-0000-0000-000000000001
    mcpmux-cli feature-sets include fs_default_00000000-0000-0000-0000-000000000001 \
      --server <registry-server-id>

Inspect what a server exposes before granting it:

    mcpmux-cli servers features <registry-server-id>
    mcpmux-cli feature-sets get <feature-set-id>

`feature-sets include` accepts `--type tool|prompt|resource` and `--name SUBSTR`
to narrow the selection, and `feature-sets remove <fs-id> <feature> --server
<id>` to remove by feature name.

For OAuth-protected servers, `servers enable` leaves the server in
`auth_required` state. Start the browser flow on the daemon host (or over the
SSH tunnel below) and open the printed URL:

    mcpmux-cli servers auth <registry-server-id>

Read-only and configuration commands use the same socket:

    mcpmux-cli daemon status
    mcpmux-cli health
    mcpmux-cli doctor
    mcpmux-cli registry list [--query Q] [--category C] [--refresh]
    mcpmux-cli registry search QUERY
    mcpmux-cli spaces list
    mcpmux-cli feature-sets list
    mcpmux-cli workspaces list
    mcpmux-cli servers list
    mcpmux-cli servers features <registry-server-id>
    mcpmux-cli logs --server <registry-server-id> [--follow]

    mcpmux-cli spaces create "Team"
    mcpmux-cli spaces set-default <space-id>
    mcpmux-cli spaces base-dirs add <space-id> /srv/repos/acme
    mcpmux-cli feature-sets include <feature-set-id> --server <registry-server-id>
    mcpmux-cli workspaces bind /srv/repos/acme --space <space-id> --feature-set <fs-id>
    mcpmux-cli clients create "CI runner" --type custom

Every mutating command accepts `--output json` for scripting and `--yes` to
skip the interactive confirmation for destructive actions.

Do not edit mcpmux.db directly. Direct SQLite writes bypass the daemon's
in-memory gateway state, domain events, encryption flow, configuration
validation, and exclusive-lock discipline.

The per-workspace client snippet (with the `X-Mcpmux-Workspace` header) is
generated by:

    mcpmux-cli workspace config --path /srv/repos/acme --client cursor

Supported snippet clients are `cursor`, `claude-code`, `vscode`, `opencode`,
and `zed`. Client config export and validation are also available:

    mcpmux-cli config export --format cursor --server <registry-server-id>
    mcpmux-cli config export-space --space <space-id> --out space.json
    mcpmux-cli config import space.json --space <space-id> [--dry-run]
    mcpmux-cli config validate /path/to/mcp.json

Move a whole Space's server set between profiles or hosts with a portable
`mcpServers` document (transport only — no credentials):

    # Export every server installed in a Space.
    mcpmux-cli config export-space --space <space-id> --out space.json

    # Preview and then import into another Space.
    mcpmux-cli config import space.json --space <space-id> --dry-run
    mcpmux-cli config import space.json --space <space-id>

`config import` rejects malformed input before touching storage, backs up the
target Space's config file to `<space>.json.mcpmux-bak`, and applies a 3-way
diff (added / updated / removed) — new servers are enabled automatically. Point
Desktop and mcpmuxd at the same data directory only one at a time; the
exclusive lock refuses the second process.

Do not point Desktop and mcpmuxd at the same data directory concurrently; the
exclusive lock refuses the second process.

## Validation performed for this deployment

- cargo fmt --all --check
- cargo clippy --workspace -- -D warnings
- cargo test -p mcpmuxd -p mcpmux-control -p mcpmux-cli
- A real systemd user-service installation, restart, health check, and
  loopback socket verification
- Control-socket integration tests: ping/status, doctor, protocol version
  mismatch, unknown method, live event streaming, and registry list against a
  spawned daemon
