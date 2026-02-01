# McpMux - Centralized MCP Server Management

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

> A desktop application for managing Model Context Protocol (MCP) servers with spaces, credentials, and cloud sync.

## Features

- 🔐 **Secure Credentials** - OS keychain + encrypted database storage
- 🌐 **Spaces** - Isolated environments for different projects
- ⚡ **Local Gateway** - All MCP traffic stays on your machine
- ☁️ **Cloud Sync** - Configuration sync across devices (optional)
- 🔌 **Multi-Transport** - Supports stdio, HTTP, and SSE MCP servers

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.75+
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 9+

### Development

```bash
# First time setup (installs deps, Playwright browsers, etc.)
pnpm setup

# Or manually:
pnpm install

# Start development
pnpm dev
```

### Build

```bash
# Build for production
pnpm build
```

## Project Structure

```
mcpmux/
├── apps/
│   └── desktop/          # Tauri desktop application
│       ├── src/          # React frontend
│       └── src-tauri/    # Rust backend
├── crates/
│   ├── mcpmux-core/       # Domain logic and entities
│   ├── mcpmux-mcp/        # MCP protocol handling
│   └── mcpmux-storage/    # Persistence layer
└── packages/
    └── ui/               # Shared React components
```

## Architecture

McpMux acts as a local gateway that:

1. **Aggregates** multiple MCP servers into a single endpoint
2. **Manages** credentials securely per space
3. **Routes** tool calls to the appropriate backend
4. **Syncs** configuration (not MCP traffic) to the cloud

```
┌─────────────────────────────────────────────────────────┐
│                    AI Clients                           │
│              (Cursor, Claude, etc.)                     │
└─────────────────────┬───────────────────────────────────┘
                      │ OAuth 2.1 + PKCE
                      ▼
┌─────────────────────────────────────────────────────────┐
│                  McpMux Gateway                          │
│                 localhost:9315                          │
├─────────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌─────────────┐            │
│  │ Space A │  │ Space B │  │ FeatureSets │            │
│  └─────────┘  └─────────┘  └─────────────┘            │
└─────────────────────┬───────────────────────────────────┘
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
   ┌─────────┐   ┌─────────┐   ┌─────────┐
   │ Backend │   │ Backend │   │ Backend │
   │ (stdio) │   │  (HTTP) │   │  (SSE)  │
   └─────────┘   └─────────┘   └─────────┘
```

## License

[GNU General Public License v3.0](LICENSE) - Free software, copyleft license.

