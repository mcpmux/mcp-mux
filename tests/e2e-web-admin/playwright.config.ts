/**
 * E2E for the EMBEDDED web admin — the real desktop React app served headless
 * by `mcpmux serve` (feature `embed-ui`), driving the command-mirror RPC. This
 * is NOT the mocked web-layer suite (tests/e2e); it runs against a real
 * gateway binary with a real SQLite backend.
 *
 * Prerequisite: build the binary with the embedded UI first —
 *   cd apps/desktop && MCPMUX_WEB_BASE=/app/ pnpm build:web
 *   cargo build -p mcpmux-serve --features embed-ui
 * (the `test:e2e:web-admin` script does this for you).
 */
import { defineConfig } from '@playwright/test';
import path from 'path';
import { fileURLToPath } from 'url';

const here = path.dirname(fileURLToPath(import.meta.url));
const PORT = 45980;
const TOKEN = 'e2e-web-token';
const BIN = path.resolve(
  here,
  '../../target/debug',
  process.platform === 'win32' ? 'mcpmux.exe' : 'mcpmux'
);
const DATA_DIR = path.resolve(here, '.tmp-data');

export default defineConfig({
  testDir: './specs',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  workers: 1,
  timeout: 30_000,
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    screenshot: 'only-on-failure',
    launchOptions: { args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'] },
  },
  // Expose the token to the spec.
  metadata: { adminToken: TOKEN },
  webServer: {
    command: `"${BIN}"`,
    url: `http://127.0.0.1:${PORT}/health`,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
    env: {
      MCPMUX_DATA_DIR: DATA_DIR,
      MCPMUX_HOST: '127.0.0.1',
      MCPMUX_PORT: String(PORT),
      MCPMUX_AUTH_DISABLED: 'true',
      MCPMUX_ADMIN_TOKEN: TOKEN,
      MCPMUX_LOG: 'warn',
    },
  },
});
