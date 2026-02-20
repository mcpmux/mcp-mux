# Screenshot Generation

Automated screenshots of the McpMux desktop app for docs and marketing.

## Quick Start

```bash
# 1. Build the desktop app (required — E2E runs against the release binary)
pnpm build

# 2. (Optional) Start the discover web UI for web screenshots
cd ../mcpmux.discover.ui && pnpm dev &

# 3. Run the screenshot capture spec
pnpm exec wdio run tests/e2e/wdio.conf.ts --spec tests/e2e/specs/capture-screenshots.manual.ts
```

## What Gets Captured

### Page Screenshots

| Screenshot | Page | Type | Content |
|---|---|---|---|
| `dashboard.png` | Dashboard | content area | Gateway status, stats cards, client config snippet |
| `servers.png` | My Servers | content area | Installed servers with connection states |
| `discover.png` | Discover | content area | Server registry (mock bundle with 14 servers) |
| `spaces.png` | Spaces | content area | Workspace cards (default + 5 additional) |
| `featuresets.png` | FeatureSets | content area | Permission bundles |
| `clients.png` | Clients | full window | OAuth consent modal over connected clients (Cursor, VS Code, Windsurf, Claude Code) |
| `settings.png` | Settings | content area | App settings page |

### Detail Screenshots (modals, panels, focused views)

| Screenshot | Content |
|---|---|
| `featureset-detail.png` | Feature set panel with permission checkboxes |
| `client-detail.png` | Client detail panel with Permissions section (Quick Settings collapsed) |
| `client-permissions.png` | Client panel with Effective Features section expanded (resolved tools/prompts/resources) |
| `server-expanded.png` | Server card expanded showing tools, prompts, resources |
| `space-switcher.png` | Sidebar with space switcher dropdown open (full window) |
| `install-modal.png` | Server install modal from registry |

### Web Screenshots

| Screenshot | Content |
|---|---|
| `discover-web.png` | Discover web UI (Next.js site — requires dev server on port 3000) |

## Output Locations

Screenshots are saved to two locations:

- **`mcp-mux/docs/screenshots/`** — Used by the project README
- **`mcpmux.discover.ui/public/screenshots/`** — Used by the discover website

## Screenshot Strategy

- **Page screenshots** capture the `<main>` content area only (excluding sidebar and titlebar) for maximum readability when displayed on the website
- **Detail screenshots** capture specific elements (modals, panels) for feature-focused views
- **Full-window screenshots** are used only when sidebar context is valuable (e.g., space switcher)
- Window size is 1600×1000 — content area is ~1360×964

## Editing Mock Data

All screenshot mock data lives in a single file:

```
tests/e2e/mocks/screenshot-preseed.ts
```

This controls:
- **Spaces** created (name, icon)
- **Servers** installed and enabled (by ID from `fixtures.ts`)
- **FeatureSets** created (name, description)

OAuth client registrations are defined in `capture-screenshots.manual.ts` itself (the `OAUTH_CLIENTS` constant).

Server icons use real GitHub avatar URLs matching the production server definitions in `mcp-servers/servers/*.json`.

After editing, re-run the capture command above.

## How It Works

1. The E2E framework launches the Tauri app with a mock bundle API (port 8787)
2. The `before()` hook seeds data via `window.__TAURI_TEST_API__` (spaces, servers, feature sets)
3. OAuth clients are registered via DCR POST to the gateway and then approved
4. Each `it()` block navigates to a page, optionally opens a panel/modal, and saves a focused screenshot
5. The discover-web screenshot navigates to `localhost:3000` (Next.js dev server)

## Notes

- This spec uses `.manual.ts` (not `.wdio.ts`) so it is **excluded from `pnpm test:e2e`** — it only runs when invoked explicitly
- Server definitions come from `tests/e2e/mocks/mock-bundle-api/fixtures.ts`
- The app must be built before running (`pnpm build`) — the signing error is non-blocking
- For discover-web screenshots, start the discover UI dev server first: `cd ../mcpmux.discover.ui && pnpm dev`
