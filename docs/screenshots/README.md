# Screenshot Generation

Automated screenshots of the McpMux desktop app for docs and marketing.

## Quick Start

```bash
# 1. Build the desktop app (required — E2E runs against the release binary)
pnpm build

# 2. Run the screenshot capture spec
pnpm exec wdio run tests/e2e/wdio.conf.ts --spec tests/e2e/specs/capture-screenshots.manual.ts
```

## What Gets Captured

| Screenshot | Page | Content |
|---|---|---|
| `dashboard.png` | Dashboard | Gateway status, stats cards, client config snippet |
| `servers.png` | My Servers | Installed servers with connection states |
| `discover.png` | Discover | Server registry (mock bundle with 12 servers) |
| `spaces.png` | Spaces | Workspace cards (default + 3 additional) |
| `featuresets.png` | FeatureSets | Permission bundles |
| `clients.png` | Clients | Connected AI clients (Cursor, VS Code, Claude Desktop) |
| `settings.png` | Settings | App settings page |

## Output Locations

Screenshots are saved to two locations:

- **`mcp-mux/docs/screenshots/`** — Used by the project README
- **`mcpmux.discover.ui/public/screenshots/`** — Used by the discover website

## Editing Mock Data

All screenshot mock data lives in a single file:

```
tests/e2e/mocks/screenshot-preseed.ts
```

This controls:
- **Spaces** created (name, icon)
- **Servers** installed and enabled (by ID from `fixtures.ts`)
- **FeatureSets** created (name, description)
- **OAuth Clients** shown on the Clients page (name, version, connection mode)

After editing, re-run the capture command above.

## How It Works

1. The E2E framework launches the Tauri app with a mock bundle API (port 8787)
2. The `before()` hook seeds data via `window.__TAURI_TEST_API__` (spaces, servers, feature sets)
3. OAuth clients are mocked by intercepting `window.__TAURI_INTERNALS__.invoke`
4. Each `it()` block navigates to a page and saves a screenshot

## Notes

- This spec uses `.manual.ts` (not `.wdio.ts`) so it is **excluded from `pnpm test:e2e`** — it only runs when invoked explicitly
- Server definitions come from `tests/e2e/mocks/mock-bundle-api/fixtures.ts`
- The app must be built before running (`pnpm build`) — the signing error is non-blocking
