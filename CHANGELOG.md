# Changelog

## [0.2.0](https://github.com/mcpmux/mcp-mux/compare/v0.1.2...v0.2.0) (2026-02-18)


### Features

* add select, file_path, and directory_path input types ([#121](https://github.com/mcpmux/mcp-mux/issues/121)) ([942ee1a](https://github.com/mcpmux/mcp-mux/commit/942ee1ae88f60aa1454bc97cec3839bcacf74454))


### Bug Fixes

* add one-click IDE install for VS Code and Cursor ([#119](https://github.com/mcpmux/mcp-mux/issues/119)) ([5b280fb](https://github.com/mcpmux/mcp-mux/commit/5b280fbfdcd04165827b7662ba6896cea96deb83))
* version display & update check ([#117](https://github.com/mcpmux/mcp-mux/issues/117)) ([b40c59b](https://github.com/mcpmux/mcp-mux/commit/b40c59bfb7b9ec19be8848abe04e38ba6fed1422))

## [0.1.2](https://github.com/mcpmux/mcp-mux/compare/v0.1.1...v0.1.2) (2026-02-18)


### Bug Fixes

* resolve npx/node PATH on macOS GUI apps ([#113](https://github.com/mcpmux/mcp-mux/issues/113)) ([98c013d](https://github.com/mcpmux/mcp-mux/commit/98c013d4e6955e678949df6068c038e1b8cf00fc))


### Documentation

* improve README first impression with problem/fix diagrams ([#109](https://github.com/mcpmux/mcp-mux/issues/109)) ([b15482b](https://github.com/mcpmux/mcp-mux/commit/b15482b32a016e3ca92753f26212f5827f744903))

## [0.1.1](https://github.com/mcpmux/mcp-mux/compare/v0.1.0...v0.1.1) (2026-02-16)


### Bug Fixes

* file-based keychain fallback for headless Linux/WSL ([#103](https://github.com/mcpmux/mcp-mux/issues/103)) ([9b60e0b](https://github.com/mcpmux/mcp-mux/commit/9b60e0bbe47a2318e7352efd3ba8b1888f393f38))
* stdio enable error UI state ([#104](https://github.com/mcpmux/mcp-mux/issues/104)) ([b4598e6](https://github.com/mcpmux/mcp-mux/commit/b4598e60e12d3389717fc2252bac8eb29e96f9c9))

## [0.1.0](https://github.com/mcpmux/mcp-mux/compare/v0.0.1...v0.1.0) (2026-02-16)

First public release of McpMux — the unified MCP gateway and manager for AI clients.

### Features

* Unified MCP gateway — configure servers once, connect every AI client through a single endpoint
* Encrypted credential storage via OS keychain (DPAPI, Keychain, Secret Service) with AES-256-GCM field-level encryption
* Spaces for organizing servers into workspaces with per-client access key authentication
* FeatureSet filtering — fine-grained control over tools, resources, and prompts per client
* OAuth 2.1 + PKCE with automatic token refresh for OAuth-enabled MCP servers
* Server discovery — browse and install from the community registry at mcpmux.com
* Streamable HTTP transport with SSE notifications
* Stdio transport with platform-specific process isolation
* Server connection logging with MCP protocol notifications and stderr capture
* Custom server configuration fields — environment variables, arguments, and headers
* Default values for server input definitions
* McpMux-branded OAuth authorization pages
* System tray with autostart on login
* Built-in auto-updater with signed releases
* Cross-platform installers — Windows (NSIS), macOS (DMG via Homebrew), Linux (APT + AppImage + .deb)
