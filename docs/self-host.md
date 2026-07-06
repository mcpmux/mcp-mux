# Self-hosting McpMux (`mcpmux serve`)

`mcpmux serve` runs the McpMux gateway headless — the same gateway the desktop
app uses, without the Tauri UI. Point any MCP client (Cursor, Claude, VS Code,
your own agent) at it. This is the foundation of the self-host / cloud story
(cloud-support M1).

> **Status:** M1 foundation. Admin is currently API-key + config driven; the
> web admin UI arrives in a later milestone. Manage servers/FeatureSets from a
> desktop instance sharing the same data dir, or via the `mcpmux_*` meta-tools
> over MCP, for now.

## Quick start

```bash
# Build (from the mcp-mux workspace)
cargo build --release -p mcpmux-serve   # produces target/release/mcpmux

# Run on loopback, auth required (the default)
./target/release/mcpmux
# → MCP endpoint: http://localhost:45818/mcp   health: /health
```

Verify it's up:

```bash
curl http://localhost:45818/health        # {"status":"ok","version":"..."}
```

## Web admin

The container image (and a `--features embed-ui` build) serves the **full McpMux
admin UI** — the same React app the desktop uses — at **`/app`**. Open
`http://<host>:45818/app/` (or your TLS origin), sign in with the admin token
printed at startup (or `MCPMUX_ADMIN_TOKEN`), and manage Spaces, FeatureSets,
clients, and mappings from the browser. It drives the management API's
command-mirror RPC (`POST /admin/api/rpc/<command>`); OS-only desktop features
(tray, updater, on-disk client-config editing) are unavailable in the browser.

There is also a lightweight standalone console at `/admin` (no build step
needed) covering the core read/write loop.

Build the binary with the embedded UI locally:

```bash
cd apps/desktop && MCPMUX_WEB_BASE=/app/ pnpm build:web   # produces dist/
cargo build --release -p mcpmux-serve --features embed-ui
```

## Configuration

Precedence: **defaults < TOML file < environment variables.**

`mcpmux.toml`:

```toml
data_dir = "/var/lib/mcpmux"     # db, keys, logs, spaces
host = "127.0.0.1"                # 0.0.0.0 to expose on the network (see below)
port = 45818
# public_base_url = "https://mcp.example.com"   # when fronted by a TLS proxy/tunnel
auth_disabled = false             # NEVER true on a network bind (refused at startup)
additional_allowed_hosts = []     # extra Host values accepted on a network bind
allow_any_host = false            # escape hatch; weakens DNS-rebinding protection
log = "info"
```

Run with it: `mcpmux --config /etc/mcpmux/mcpmux.toml`

### Environment variables

| Var | Meaning |
|-----|---------|
| `MCPMUX_CONFIG` | Path to the TOML file (same as `--config`) |
| `MCPMUX_DATA_DIR` | Data directory |
| `MCPMUX_HOST` / `MCPMUX_PORT` | Bind address |
| `MCPMUX_PUBLIC_BASE_URL` | External origin advertised in OAuth metadata |
| `MCPMUX_AUTH_DISABLED` | Disable inbound auth (loopback only) |
| `MCPMUX_ALLOWED_HOSTS` | Comma-separated extra Host values |
| `MCPMUX_ALLOW_ANY_HOST` | Accept any Host on a network bind |
| `MCPMUX_MASTER_KEY` | Hex-encoded 32-byte master key (see below) |
| `MCPMUX_REGISTRY_URL` | Server registry API (default `https://api.mcpmux.com`) |
| `MCPMUX_LOG` / `RUST_LOG` | Log filter |

## Secrets: the master encryption key

Credentials are encrypted at rest with a 32-byte master key. Two ways to supply it:

1. **On-disk / keychain (default).** With no `MCPMUX_MASTER_KEY`, the key is
   generated and stored under `data_dir` (or the OS keychain where available).
   Back up `data_dir` and it persists across restarts.
2. **Injected (`MCPMUX_MASTER_KEY`).** For containers/secret managers — provide
   the same hex key on every start (nothing is persisted). Generate one:
   ```bash
   openssl rand -hex 32
   ```
   **If this value changes, previously-encrypted credentials become unreadable.**

## Exposing on a network

Set `host = "0.0.0.0"`. The gateway then enforces, automatically:

- **Inbound authentication is mandatory** — `auth_disabled = true` with a
  network bind is **refused at startup** (there is no unauthenticated network
  mode).
- **Host-header allowlist** — only requests addressed to this machine's
  IPs/hostname, `public_base_url`, or `additional_allowed_hosts` are answered
  (DNS-rebinding protection). Add reverse-proxy / mDNS names to
  `additional_allowed_hosts`.
- **Rate limiting** on `/mcp` per peer + credential, with an auth-failure
  lockout.

**TLS:** the gateway speaks plain HTTP. For anything beyond a trusted LAN, front
it with a TLS-terminating reverse proxy (Caddy, nginx, Cloudflare Tunnel) and
set `public_base_url` to the external `https://` origin. See
`mcpmux.space/ADR/004-lan-tls-posture.md`.

### Example: Caddy in front

```
mcp.example.com {
    reverse_proxy 127.0.0.1:45818
}
```
```toml
# mcpmux.toml
host = "127.0.0.1"
public_base_url = "https://mcp.example.com"
```

## Connecting a client

With auth disabled (loopback dev):
```json
"mcpmux": { "url": "http://localhost:45818/mcp" }
```

With auth (register an API-key client on a desktop instance sharing the data
dir, or via the pairing flow) and use:
```json
"mcpmux": {
  "url": "https://mcp.example.com/mcp",
  "headers": { "Authorization": "Bearer <your API key>" }
}
```

## Operations

- **Health:** `GET /health` → `{"status":"ok","version":"…"}`.
- **Graceful shutdown:** `SIGTERM` / Ctrl-C drains in-flight requests and
  releases the port (safe for container restarts).
- **Backup/restore:** snapshot `data_dir` (SQLite `mcpmux.db*`, `keys/`,
  `spaces/`). Restore by putting it back before starting.

## Limitations (M1)

- No bundled web admin yet — manage config via a shared desktop instance or the
  `mcpmux_*` meta-tools over MCP.
- Upstream-server OAuth consent still completes interactively on a desktop host;
  headless upstream OAuth is limited to flows whose redirect can round-trip via
  `public_base_url`.
- Desktop-only OS features (tray, deep links, auto-update) are not present by
  design.
