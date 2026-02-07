# McpMux

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/MCP-Mux/mcp-mux)](https://github.com/MCP-Mux/mcp-mux/releases)

**One gateway. Every MCP server. Zero cloud traffic.**

McpMux is a cross-platform desktop application that aggregates all your Model Context Protocol (MCP) servers behind a single local endpoint. It manages credentials, isolates projects into Spaces, and keeps every tool call on your machine.

---

## The Problem

Working with MCP servers today means juggling multiple connections, credentials, and configurations across different AI clients. Each server may require its own authentication, and switching between projects means reconfiguring everything. Credentials end up scattered, there is no unified view of available tools, and sensitive data often passes through third-party services unnecessarily.

## The Solution

McpMux runs a local gateway on `localhost:9315` that acts as a single entry point for all your MCP servers. AI clients connect to McpMux once, and it handles the rest: routing tool calls to the right backend, refreshing OAuth tokens automatically, and keeping credentials locked in your OS keychain.

```
┌─────────────────────────────────────────────────────────┐
│                     AI Clients                          │
│           (Cursor, Claude Desktop, VS Code, etc.)       │
└──────────────────────┬──────────────────────────────────┘
                       │ Single connection
                       ▼
┌─────────────────────────────────────────────────────────┐
│                  McpMux Gateway                         │
│                 localhost:9315                           │
│                                                         │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐         │
│  │ Space A  │  │ Space B  │  │ Feature Sets  │         │
│  │ (Work)   │  │ (Personal)│  │ (Permissions) │         │
│  └──────────┘  └──────────┘  └───────────────┘         │
└──────────┬──────────┬──────────┬────────────────────────┘
           │          │          │
     ┌─────┘    ┌─────┘    ┌────┘
     ▼          ▼          ▼
┌─────────┐┌─────────┐┌─────────┐
│  stdio  ││  HTTP   ││   SSE   │
│ servers ││ servers ││ servers │
└─────────┘└─────────┘└─────────┘
```

---

## Features

### Local-First Gateway
All MCP traffic stays on your machine. McpMux runs entirely on `localhost` and never routes tool calls through external services. Cloud sync is optional and limited to configuration data only (server definitions, space settings) — never command payloads or responses.

### Spaces: Project Isolation
Organize servers into isolated workspaces called **Spaces**. Each Space has its own set of server configurations, credentials, and permissions. Switch between "Work", "Personal", or "Client Project" contexts without reconfiguring anything. Credentials are never shared across Spaces.

### Secure Credential Management
Credentials are stored using your operating system's native keychain:
- **macOS**: Keychain
- **Windows**: Credential Manager
- **Linux**: Secret Service (GNOME Keyring, KWallet)

Sensitive fields in the local database are encrypted with **AES-256-GCM** authenticated encryption. Encryption keys live in the OS keychain, not on disk. Sensitive data is zeroized from memory after use to prevent leaks.

### OAuth 2.1 with PKCE
McpMux implements the full OAuth 2.1 authorization flow with Proof Key for Code Exchange (PKCE, RFC 7636) for connecting to remote MCP servers. It handles:
- Automatic server discovery via RFC 8414 (`.well-known/oauth-authorization-server`)
- Dynamic Client Registration (RFC 7591)
- Automatic token refresh — no manual re-authentication

### Multi-Transport Support
Connect to MCP servers over any supported transport:
- **stdio** — local processes (Node.js, Python, Rust, Go, etc.)
- **Streamable HTTP** — remote servers over HTTP/HTTPS
- **SSE** — Server-Sent Events for streaming

### Server Registry & Discovery
Browse and install servers from the built-in MCP registry. McpMux automatically discovers available tools, prompts, and resources from each connected server and caches them for offline access.

### Feature Sets & Permissions
Create fine-grained permission bundles called **Feature Sets** to control which tools, prompts, and resources each client can access. Compose sets from other sets, include or exclude specific features, and assign them per client.

### Client Access Keys
Generate access keys for each AI client. Clients authenticate to the gateway using `MCP-Key` or `Bearer` tokens. Each client can be assigned different Feature Sets, giving you granular control over what each client can do.

### Server Logging & Monitoring
Each server gets its own rotating log file with entries categorized by source (Connection, OAuth, Transport, MCP, Feature). View logs directly in the app's UI. Sensitive tokens are never written to logs.

### System Tray & Auto-Start
McpMux runs in the system tray and can be configured to start automatically with your OS. The gateway keeps running in the background so your AI clients always have access to their tools.

### Auto-Updates
Built-in update mechanism checks for new releases automatically. Updates are signed and verified before installation.

### Deep Linking
The `mcpmux://` URL scheme allows external applications to trigger actions in McpMux directly, such as initiating OAuth flows or installing servers.

---

## Security

McpMux is designed with a defense-in-depth approach to credential and data security.

| Layer | Mechanism | Details |
|-------|-----------|---------|
| **Credential Storage** | OS Keychain | Master encryption key and JWT signing secret stored in platform-native secure storage |
| **Database Encryption** | AES-256-GCM | Field-level authenticated encryption with unique nonces per operation |
| **Memory Safety** | Zeroize | Sensitive data cleared from memory after use |
| **Authentication** | OAuth 2.1 + PKCE | S256 code challenge method; automatic token refresh |
| **Client Auth** | Access Keys | Per-client `mcp_<random>` tokens with configurable permissions |
| **Network** | Local-only by default | Gateway binds to `127.0.0.1`; no external exposure |
| **TLS** | rustls | HTTPS support for remote server connections |
| **Logging** | Sanitized | Tokens and secrets are never written to log files |
| **Isolation** | Spaces | Credentials and configurations never leak between Spaces |
| **Sessions** | JWT (HS256) | Signed with 32-byte secret stored in OS keychain |

---

## Getting Started

### Download

Download the latest release for your platform from the [Releases page](https://github.com/MCP-Mux/mcp-mux/releases):

| Platform | Format |
|----------|--------|
| **Windows** | MSI installer |
| **macOS** | DMG |
| **Linux** | DEB, RPM, AppImage |

### Configure Your AI Client

Once McpMux is running, point your AI client to the local gateway:

```json
{
  "mcpServers": {
    "mcpmux": {
      "url": "http://localhost:9315/mcp"
    }
  }
}
```

---

## Development

### Prerequisites

- [Rust](https://rustup.rs/) 1.75+
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 9+

**Linux additional dependencies** (for credential storage via Secret Service):

```bash
# Debian/Ubuntu
sudo apt install gnome-keyring libsecret-1-dev librsvg2-dev pkg-config

# Fedora/RHEL
sudo dnf install gnome-keyring libsecret-devel librsvg2-devel pkg-config
```

### Setup & Run

```bash
# First-time setup (installs dependencies, Playwright browsers, etc.)
pnpm setup

# Start development (launches both Tauri backend and React frontend)
pnpm dev

# Build for production
pnpm build
```

### Testing

```bash
# Run all tests
pnpm test

# Rust unit tests
pnpm test:rust:unit

# Rust integration tests
pnpm test:rust:int

# TypeScript tests
pnpm test:ts

# E2E tests (web, works on all platforms)
pnpm test:e2e:web

# Full Tauri E2E tests (Windows/Linux)
pnpm test:e2e
```

### Code Quality

```bash
# Lint, typecheck, and format check
pnpm validate

# Auto-format
pnpm format
```

---

## Project Structure

```
mcp-mux/
├── apps/
│   └── desktop/               # Tauri desktop application
│       ├── src/               # React 19 + TypeScript frontend
│       └── src-tauri/         # Rust backend (Tauri commands)
├── crates/
│   ├── mcpmux-core/           # Domain entities, services, events
│   ├── mcpmux-gateway/        # HTTP gateway, connection pool, OAuth, routing
│   ├── mcpmux-storage/        # SQLite, AES-256-GCM encryption, OS keychain
│   └── mcpmux-mcp/            # MCP protocol implementation
├── packages/
│   └── ui/                    # Shared React UI components
└── tests/
    ├── rust/                  # Rust integration tests
    ├── ts/                    # TypeScript unit tests
    └── e2e/                   # E2E tests (Playwright + WebdriverIO)
```

### Tech Stack

| Component | Technology |
|-----------|-----------|
| **Desktop shell** | Tauri 2.x |
| **Backend** | Rust (Tokio, Axum, SQLite) |
| **Frontend** | React 19, TypeScript 5.7, Tailwind CSS, Zustand |
| **MCP SDK** | rmcp 0.14 |
| **Encryption** | ring (AES-256-GCM) |
| **Keychain** | keyring (cross-platform) |
| **Build** | Cargo + Vite + pnpm workspaces |
| **Testing** | cargo test, Vitest, Playwright, WebdriverIO |

---

## Configuration

### Gateway

The gateway listens on `127.0.0.1:9315` by default. The port and host can be configured in the application settings.

### Data Storage

| Platform | Location |
|----------|----------|
| **Linux** | `~/.local/share/com.mcpmux.desktop/` |
| **macOS** | `~/Library/Application Support/com.mcpmux.desktop/` |
| **Windows** | `%LOCALAPPDATA%\com.mcpmux.desktop\` |

The database (`mcpmux.db`) and per-server log files are stored in this directory.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines, including the Developer Certificate of Origin (DCO) requirement.

## License

[GNU General Public License v3.0](LICENSE)
