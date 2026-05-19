# Run McpMux from Source (macOS)

Two flows for working against this repo, picked by what you're doing:

| Flow | Use when | Speed | Cursor / Claude / VS Code see it? |
| ---- | -------- | ----- | --------------------------------- |
| **Dev watch mode** (`pnpm dev`) | Iterating on UI or Rust — you want HMR for React and auto-recompile for Tauri commands | Vite HMR is instant; Rust changes ~5–15s incremental | Yes — same `localhost:45818` endpoint while `pnpm dev` is running |
| **Build + swap** (replace `/Applications/McpMux.app`) | You want a real installed app on this branch — autostart, system tray, runs without a terminal, survives reboot | Full build ~5–10 min, incremental ~1–3 min | Yes — and stays running after you close your editor |

Quick rule of thumb: **`pnpm dev` while you're coding, swap when you're done** so other AI clients keep working when Cursor isn't open.

---

## What survives between flows

Both dev mode and a swapped `.app` use the same `com.mcpmux.desktop` bundle identifier, so they share data:

| Data | Location |
| ---- | -------- |
| SQLite DB (spaces, servers, clients, settings) | `~/Library/Application Support/com.mcpmux.desktop/mcpmux.db` |
| Per-space files | `~/Library/Application Support/com.mcpmux.desktop/spaces/` |
| Logs | `~/Library/Application Support/com.mcpmux.desktop/logs/` |
| Encryption master key | macOS Keychain (`com.mcpmux.desktop` service) |
| OAuth tokens / credentials | Encrypted in SQLite + Keychain |

The new binary reads the same data dir and keychain entries as the release. Spaces, server installs, and access keys persist across `pnpm dev` ↔ `/Applications` swaps.

**What you might need to redo:** OAuth re-auth in Cursor/Claude Desktop if DCR or token validation changed on your branch.

---

## Prerequisites

From repo root (`mcp-mux/`):

- Rust 1.75+
- Node.js 20+
- pnpm 9+
- Xcode Command Line Tools (`xcode-select --install`)

First-time setup (if deps aren't installed):

```bash
pnpm install
```

---

## Flow 1 — Dev watch mode (`pnpm dev`)

Live-reload while you code. Best for tight iteration on UI or Rust.

### What it does

| Layer | Behavior |
| ----- | -------- |
| React / Tailwind / TS | Vite dev server on `localhost:1420` with **HMR** — change a `.tsx`/`.css`, see it instantly without losing app state |
| Rust (Tauri commands, gateway, storage) | Recompiles + relaunches the Tauri window on any `.rs` save under `src-tauri/` or `crates/` |
| Bundle ID | Same `com.mcpmux.desktop` — reads your real DB and Keychain entries |

### Run it

```bash
# Quit the installed app first so the dev gateway can bind 127.0.0.1:45818
osascript -e 'tell application "McpMux" to quit' 2>/dev/null; sleep 2

pnpm dev
```

A Tauri window opens. Edit `.tsx` files for instant HMR; edit Rust and the window will relaunch on its own after recompile.

### Frontend-only iteration

If you're only changing UI and want the fastest possible loop:

```bash
pnpm dev:web
```

This runs Vite alone in a browser tab — no Rust, no Tauri shell. Tauri `invoke()` calls won't work (no backend), but for pure layout/styling work it's the quickest path.

### Gotchas

| Symptom | Why | Fix |
| ------- | --- | --- |
| Keychain prompts on first launch of the dev binary | Different signer than `/Applications/McpMux.app` | Click **Always Allow** once — sticks for that built artifact. See `Keychain prompts` below for detail |
| `Address already in use: 45818` | Installed `.app` still running | `osascript -e 'tell application "McpMux" to quit'` then retry |
| Cursor's MCP server "disconnected" mid-session | You stopped `pnpm dev` | Cursor reconnects when the gateway is back on `localhost:45818` (either flow) |
| Rust recompile feels slow | Big edits in `mcpmux-gateway` / `mcpmux-storage` | Expected — keep edits scoped or use `pnpm dev:web` for UI |
| `pnpm dev` keeps crashing with "Master key not found" | DB/keychain mismatch from manual deletion | Don't manually delete keychain entries — see `Keychain prompts` below |

### Keychain prompts

McpMux reads two secrets from Keychain on startup:

1. **Master encryption key** — every app launch (decrypts SQLite credentials)
2. **JWT signing secret** — first time you start the gateway in a session

macOS scopes Keychain access to the **specific signed binary**, not just the bundle ID. So:

- First launch of a `pnpm dev` build → 1–2 prompts
- First launch after a fresh `pnpm build` swap → 1–2 prompts
- Subsequent launches of the **same** built binary → silent if you clicked **Always Allow**
- Alternating between `pnpm dev` and `/Applications/McpMux.app` → may re-prompt because each is a different signer

This is expected. Click **Always Allow** the first time you see each prompt for a new build.

---

## Flow 2 — Build and swap into `/Applications`

Use when you want the source build to behave like an installed app: launch from Spotlight/Dock, autostart, run in the background without a dev terminal, survive reboots.

### Option A — Full build (recommended)

Rebuilds the React frontend and produces a fresh `.app` bundle. Use this when frontend or Tauri config changed, or when you want a clean bundle.

#### 1. Quit the running app

```bash
osascript -e 'tell application "McpMux" to quit' 2>/dev/null || true
# Give it a moment to release the gateway port
sleep 2
```

#### 2. Build

```bash
cd /path/to/mcp-mux
pnpm build
```

First build: ~5–10 min. Incremental: ~1–3 min.

Output:

```
target/release/bundle/macos/McpMux.app
target/release/bundle/dmg/McpMux_*.dmg   # optional installer artifact
```

> **Note:** the build may exit non-zero at the very end with `TAURI_SIGNING_PRIVATE_KEY` missing. That only blocks the auto-update artifact; the `.app` and `.dmg` are still produced and usable.

#### 3. Backup and swap

```bash
# Backup current install (skip if you already have a recent .bak)
sudo mv /Applications/McpMux.app /Applications/McpMux.app.bak

# Install the new build
sudo cp -R target/release/bundle/macos/McpMux.app /Applications/

# Fix ownership (sudo cp leaves root-owned files)
sudo chown -R "$(whoami):admin" /Applications/McpMux.app
```

#### 4. Re-sign (required after manual swap)

macOS Gatekeeper rejects a bundle whose binary was replaced without re-signing:

```bash
xattr -dr com.apple.quarantine /Applications/McpMux.app 2>/dev/null || true
codesign --force --deep --sign - /Applications/McpMux.app
```

#### 5. Launch

```bash
open /Applications/McpMux.app
```

Verify: spaces, installed servers, and gateway on `localhost:45818` should look exactly as before. First launch will trigger 1–2 Keychain prompts because the new ad-hoc signature is a different signer than the previous build — click **Always Allow** once and you're set until the next swap.

### Option B — Binary-only swap (fast path)

When you changed **Rust only** (no frontend, no `tauri.conf.json` changes). Skips the Vite build and DMG step.

```bash
osascript -e 'tell application "McpMux" to quit' 2>/dev/null || true
sleep 2

cd /path/to/mcp-mux
cargo build --release -p mcpmux

cp /Applications/McpMux.app/Contents/MacOS/mcpmux \
   /Applications/McpMux.app/Contents/MacOS/mcpmux.bak
cp target/release/mcpmux /Applications/McpMux.app/Contents/MacOS/mcpmux

xattr -dr com.apple.quarantine /Applications/McpMux.app 2>/dev/null || true
codesign --force --deep --sign - /Applications/McpMux.app

open /Applications/McpMux.app
```

Keeps the existing bundle shell (icons, Info.plist, embedded frontend from last full build). Only the Rust binary updates.

---

## Rollback (Flow 2 only)

If a swapped build is broken, restore the previous `/Applications/McpMux.app`. Dev-mode (`pnpm dev`) doesn't need a rollback — just stop the dev process.

### Full build rollback

```bash
osascript -e 'tell application "McpMux" to quit' 2>/dev/null || true
sudo rm -rf /Applications/McpMux.app
sudo mv /Applications/McpMux.app.bak /Applications/McpMux.app
open /Applications/McpMux.app
```

### Binary-only rollback

```bash
osascript -e 'tell application "McpMux" to quit' 2>/dev/null || true
cp /Applications/McpMux.app/Contents/MacOS/mcpmux.bak \
   /Applications/McpMux.app/Contents/MacOS/mcpmux
codesign --force --deep --sign - /Applications/McpMux.app
open /Applications/McpMux.app
```

---

## Troubleshooting

| Symptom | Applies to | Fix |
| ------- | ---------- | --- |
| "App is damaged" / won't open | Flow 2 | Re-run `codesign --force --deep --sign - /Applications/McpMux.app` |
| Gateway port already in use | Both | Old process still running — `pkill -f mcpmux` then relaunch |
| Cursor OAuth fails after swap | Both | Re-trigger MCP OAuth in Cursor (DCR redirect URI validation may have changed) |
| Empty app / missing UI | Flow 2 (Option B) | You used binary-only swap but frontend changed — run Option A (full build) |
| Permission denied on `/Applications` | Flow 2 | Use `sudo` for mv/cp/chown, or install to `~/Applications/` and skip sudo |
| Keychain prompts on every launch of the same binary | Both | Click **Always Allow** (not just **Allow**); check Keychain Access for duplicate `master-encryption-key` / `jwt-signing-secret` entries from old signers |
| `pnpm dev` won't start — `EADDRINUSE 45818` | Flow 1 | The installed `.app` is still running — quit it before `pnpm dev` |

---

## One-liner (full build + swap)

Assumes you're in repo root and have a recent backup:

```bash
osascript -e 'tell application "McpMux" to quit' 2>/dev/null; sleep 2 && \
pnpm build && \
sudo rm -rf /Applications/McpMux.app && \
sudo cp -R target/release/bundle/macos/McpMux.app /Applications/ && \
sudo chown -R "$(whoami):admin" /Applications/McpMux.app && \
xattr -dr com.apple.quarantine /Applications/McpMux.app 2>/dev/null; \
codesign --force --deep --sign - /Applications/McpMux.app && \
open /Applications/McpMux.app
```

---

## Related

- [`AGENTS.md`](../AGENTS.md) — build commands and project layout
- [`CLAUDE.md`](../CLAUDE.md) — full dev environment reference
