# Changelog

## [0.0.13](https://github.com/mcpmux/mcp-mux/compare/v0.0.12...v0.0.13) (2026-02-16)


### Features

* add Homebrew tap support and ad-hoc macOS signing ([#79](https://github.com/mcpmux/mcp-mux/issues/79)) ([b07f1a3](https://github.com/mcpmux/mcp-mux/commit/b07f1a3a6a11fd1ae944368fa0909838ffb41292))
* add Linux APT repository and install infrastructure ([#85](https://github.com/mcpmux/mcp-mux/issues/85)) ([473eb1a](https://github.com/mcpmux/mcp-mux/commit/473eb1aeb25c8e01d2111b9e1a82e8fee1a4d4dd))
* apply McpMux branding to OAuth authorization pages ([#74](https://github.com/mcpmux/mcp-mux/issues/74)) ([c84e036](https://github.com/mcpmux/mcp-mux/commit/c84e036b13b276520b8433439954303dbe3dbaed))
* capture MCP protocol logging notifications in server connection logs ([#76](https://github.com/mcpmux/mcp-mux/issues/76)) ([0587741](https://github.com/mcpmux/mcp-mux/commit/058774135fed2c0220a4900372f665e88eb3dff5))
* redesign README, screenshots, and E2E capture ([#87](https://github.com/mcpmux/mcp-mux/issues/87)) ([84f15cb](https://github.com/mcpmux/mcp-mux/commit/84f15cb8b805912ca810ad00bff9f819802f4d78))


### Bug Fixes

* e2e flaky fix ([#75](https://github.com/mcpmux/mcp-mux/issues/75)) ([d8e28f8](https://github.com/mcpmux/mcp-mux/commit/d8e28f8fd7a6ab50d3a12d766b970d482ae2fbe2))
* gracefully handle invalid Apple certificate in release builds ([bb4221f](https://github.com/mcpmux/mcp-mux/commit/bb4221f9e4a47ff7fad041b13e432a2ed55e1f96))
* replace deprecated macos-13 runner with macos-latest ([92ad770](https://github.com/mcpmux/mcp-mux/commit/92ad7702d2cada69df46c3a221bc2101caf17a20))
* taskbar icon visibility ([#83](https://github.com/mcpmux/mcp-mux/issues/83)) ([400c4bc](https://github.com/mcpmux/mcp-mux/commit/400c4bcce315bc7354dacb40b1cfa95a51e0edd3))

## [0.0.12](https://github.com/mcpmux/mcp-mux/compare/v0.0.1...v0.0.12) (2026-02-14)

Prior test releases (v0.0.2–v0.0.12) during development. See v0.0.13 for the first public release.

### Features

* initial release of McpMux desktop app ([72181e2](https://github.com/mcpmux/mcp-mux/commit/72181e2b462f4f70eb586758e8bd029dcb3b7631))
* add autostart and system tray functionality ([#38](https://github.com/mcpmux/mcp-mux/issues/38)) ([cc99fcf](https://github.com/mcpmux/mcp-mux/commit/cc99fcf412f24f48edba12b8f0359fa71b5247c6))
* implement Tauri updater functionality ([#36](https://github.com/mcpmux/mcp-mux/issues/36)) ([d355c68](https://github.com/mcpmux/mcp-mux/commit/d355c68a4b33901adb7f9be8c0765252f8c3577f))
* add custom server configuration fields (env vars, args, headers) ([#54](https://github.com/mcpmux/mcp-mux/issues/54)) ([37ce0f5](https://github.com/mcpmux/mcp-mux/commit/37ce0f575883680e2ee12354e3bfea48e7a9337e))
* Streamable HTTP transport with SSE notifications and E2E tests ([#61](https://github.com/mcpmux/mcp-mux/issues/61)) ([ca5b0ff](https://github.com/mcpmux/mcp-mux/commit/ca5b0ffab19aa395a75c5f10a18ab0e6efb1752a))
* Capture and stream process stderr to server log manager ([#63](https://github.com/mcpmux/mcp-mux/issues/63)) ([96795b0](https://github.com/mcpmux/mcp-mux/commit/96795b0b54ecfaa9743bb9e6045bfc86ddadcc2f))
* support default values for input definitions ([#70](https://github.com/mcpmux/mcp-mux/issues/70)) ([a1d9599](https://github.com/mcpmux/mcp-mux/commit/a1d9599601c212c1b7054fc4c5c76f065e0ea920))
