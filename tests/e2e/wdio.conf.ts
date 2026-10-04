/**
 * WebdriverIO configuration for Tauri E2E testing
 *
 * Prerequisites:
 * 1. cargo install tauri-driver --locked
 * 2. Build the app: pnpm build
 * 3. Linux: apt-get install webkit2gtk-driver
 * 4. Windows: Edge Driver matching your Edge version
 *
 * Note: macOS is NOT supported (no WKWebView driver)
 *
 * Mock Servers:
 * - Mock Bundle API (port 3456): Returns test server configurations
 * - Stub MCP HTTP (port 3457): Streamable HTTP server without auth
 * - Stub MCP OAuth (port 3458): Streamable HTTP server with OAuth
 */

import type { Options } from '@wdio/types';
import { spawn, spawnSync, type ChildProcess } from 'child_process';
import path from 'path';
import os from 'os';
import fs from 'fs';
import video from 'wdio-video-reporter';

// Store process references
let tauriDriver: ChildProcess | null = null;
let mockBundleApi: ChildProcess | null = null;
let stubMcpHttp: ChildProcess | null = null;
let stubMcpOauth: ChildProcess | null = null;
let shouldExit = false;
let tauriDriverCrashed = false;

// Mock server ports
// Use 8787 for bundle API because that's the app's default MCPMUX_REGISTRY_URL
const MOCK_BUNDLE_API_PORT = 8787;
const STUB_MCP_HTTP_PORT = 3457;
const STUB_MCP_OAUTH_PORT = 3458;

// Keep E2E data outside the user's McpMux profile. The desktop app accepts this
// directory only when MCPMUX_E2E_TEST is also set.
const APP_DATA_DIR = path.join(os.tmpdir(), 'mcpmux-e2e');
const BUNDLE_CACHE_PATH = path.join(APP_DATA_DIR, 'cache', 'registry-bundle.json');

// Path to built app
const APP_PATH =
  process.platform === 'win32'
    ? path.resolve('./target/release/mcpmux.exe')
    : path.resolve('./target/release/mcpmux');

// Check if app is built
function checkAppBuilt(): void {
  if (!fs.existsSync(APP_PATH)) {
    console.error(`\n[ERROR] App not built. Expected at: ${APP_PATH}`);
    console.error('Run "pnpm build" first.\n');
    process.exit(1);
  }

  // A binary built before MCPMUX_E2E_DATA_DIR support silently ignores it and would
  // run every spec against the developer's real McpMux profile.
  if (!fs.readFileSync(APP_PATH).includes('MCPMUX_E2E_DATA_DIR')) {
    console.error(`\n[ERROR] App at ${APP_PATH} does not support MCPMUX_E2E_DATA_DIR.`);
    console.error('Rebuild it with "pnpm build" so E2E stays out of your real profile.\n');
    process.exit(1);
  }
}

// Check if tauri-driver is available
function checkTauriDriver(): boolean {
  try {
    // tauri-driver doesn't have --version, so check --help instead
    spawnSync('tauri-driver', ['--help'], { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

// Clear the app's SQLite database and related files for a clean start
function clearAppData(): void {
  const filesToDelete = [
    path.join(APP_DATA_DIR, 'mcpmux.db'),
    path.join(APP_DATA_DIR, 'mcpmux.db-shm'),
    path.join(APP_DATA_DIR, 'mcpmux.db-wal'),
    path.join(APP_DATA_DIR, 'cache'),
    path.join(APP_DATA_DIR, 'spaces'),
  ];

  for (const filePath of filesToDelete) {
    try {
      if (fs.existsSync(filePath)) {
        if (fs.lstatSync(filePath).isDirectory()) {
          fs.rmSync(filePath, { recursive: true, force: true });
        } else {
          fs.unlinkSync(filePath);
        }
        console.log(`[e2e] Cleared: ${filePath}`);
      }
    } catch (error) {
      console.warn(`[e2e] Failed to clear ${filePath}:`, error);
    }
  }
}

// Clear the app's registry bundle cache so it fetches fresh from our mock
function clearBundleCache(): void {
  try {
    if (fs.existsSync(BUNDLE_CACHE_PATH)) {
      fs.unlinkSync(BUNDLE_CACHE_PATH);
      console.log(`[e2e] Cleared bundle cache: ${BUNDLE_CACHE_PATH}`);
    } else {
      console.log(`[e2e] No bundle cache to clear`);
    }
  } catch (error) {
    console.warn(`[e2e] Failed to clear bundle cache: ${error}`);
  }
}

// Wait for a server to be ready by polling a health endpoint
async function waitForServer(port: number, name: string, timeout = 30000): Promise<void> {
  const start = Date.now();
  const healthUrl = `http://localhost:${port}/health`;

  while (Date.now() - start < timeout) {
    try {
      const response = await fetch(healthUrl);
      if (response.ok) {
        console.log(`[e2e] ${name} is ready on port ${port}`);
        return;
      }
    } catch {
      // Server not ready yet
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }

  throw new Error(`[e2e] ${name} failed to start within ${timeout}ms`);
}

// Start mock servers
async function startMockServers(): Promise<void> {
  const mocksDir = path.resolve('./tests/e2e/mocks');
  const tsxCli = path.resolve('./node_modules/tsx/dist/cli.mjs');

  function startMockServer(scriptPath: string, port: number): ChildProcess {
    return spawn(process.execPath, [tsxCli, scriptPath], {
      env: { ...process.env, PORT: String(port) },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  }

  // Start Mock Bundle API
  console.log('[e2e] Starting Mock Bundle API...');
  mockBundleApi = startMockServer(
    path.join(mocksDir, 'mock-bundle-api', 'server.ts'),
    MOCK_BUNDLE_API_PORT
  );
  mockBundleApi.stdout?.on('data', (data) =>
    console.log(`[mock-bundle-api] ${data.toString().trim()}`)
  );
  mockBundleApi.stderr?.on('data', (data) =>
    console.error(`[mock-bundle-api] ${data.toString().trim()}`)
  );

  // Start Stub MCP HTTP Server
  console.log('[e2e] Starting Stub MCP HTTP Server...');
  stubMcpHttp = startMockServer(
    path.join(mocksDir, 'stub-mcp-server', 'http-server.ts'),
    STUB_MCP_HTTP_PORT
  );
  stubMcpHttp.stdout?.on('data', (data) =>
    console.log(`[stub-mcp-http] ${data.toString().trim()}`)
  );
  stubMcpHttp.stderr?.on('data', (data) =>
    console.error(`[stub-mcp-http] ${data.toString().trim()}`)
  );

  // Start Stub MCP OAuth Server
  console.log('[e2e] Starting Stub MCP OAuth Server...');
  stubMcpOauth = startMockServer(
    path.join(mocksDir, 'stub-mcp-server', 'http-oauth-server.ts'),
    STUB_MCP_OAUTH_PORT
  );
  stubMcpOauth.stdout?.on('data', (data) =>
    console.log(`[stub-mcp-oauth] ${data.toString().trim()}`)
  );
  stubMcpOauth.stderr?.on('data', (data) =>
    console.error(`[stub-mcp-oauth] ${data.toString().trim()}`)
  );

  // Wait for all servers to be ready
  await Promise.all([
    waitForServer(MOCK_BUNDLE_API_PORT, 'Mock Bundle API'),
    waitForServer(STUB_MCP_HTTP_PORT, 'Stub MCP HTTP'),
    waitForServer(STUB_MCP_OAUTH_PORT, 'Stub MCP OAuth'),
  ]);
}

// Stop all mock servers
function stopMockServers(): void {
  for (const server of [mockBundleApi, stubMcpHttp, stubMcpOauth]) {
    // Skip servers that already exited: their PID may have been reused by an unrelated
    // process, which taskkill /T would take down along with its children.
    if (!server?.pid || server.exitCode !== null || server.signalCode !== null) {
      continue;
    }

    if (process.platform === 'win32') {
      spawnSync('taskkill', ['/F', '/T', '/PID', String(server.pid)], { stdio: 'ignore' });
    } else {
      server.kill();
    }
  }
  mockBundleApi = null;
  stubMcpHttp = null;
  stubMcpOauth = null;
}

function closeTauriDriver() {
  shouldExit = true;
  if (tauriDriver) {
    tauriDriver.kill();
    tauriDriver = null;
  }
  // NOTE: Do NOT pkill tauri-driver or stop mock servers here.
  // This function is called during afterSession while WebdriverIO's own
  // deleteSession is still in-flight. Killing tauri-driver at this point
  // causes connection errors that cascade to all subsequent workers.
  // Aggressive cleanup is done in beforeSession instead, before spawning
  // a fresh tauri-driver.
}

// Wait for tauri-driver to accept WebDriver connections on port 4444.
// Polls GET /status until tauri-driver responds (any HTTP response means it's ready).
async function waitForTauriDriverReady(timeout = 30000): Promise<boolean> {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    try {
      await fetch('http://localhost:4444/status');
      console.log('[e2e] tauri-driver is ready on port 4444');
      return true;
    } catch {
      // Not ready yet (ECONNREFUSED)
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  console.error(`[e2e] tauri-driver not ready after ${timeout}ms`);
  return false;
}

// Kill any processes listening on our mock server ports (leftover from previous runs)
function killPortProcesses(): void {
  const ports = [MOCK_BUNDLE_API_PORT, STUB_MCP_HTTP_PORT, STUB_MCP_OAUTH_PORT];
  for (const port of ports) {
    try {
      if (process.platform === 'win32') {
        // Find PIDs listening on this port and kill them
        const result = spawnSync(
          'cmd',
          [
            '/c',
            `for /f "tokens=5" %a in ('netstat -ano ^| findstr ":${port} " ^| findstr LISTEN') do taskkill /F /PID %a`,
          ],
          { stdio: 'pipe', shell: true }
        );
        if (result.stdout?.toString().includes('SUCCESS')) {
          console.log(`[e2e] Killed process on port ${port}`);
        }
      } else {
        spawnSync('fuser', ['-k', `${port}/tcp`], { stdio: 'ignore' });
      }
    } catch {
      // Ignore errors - no process may be on this port
    }
  }
}

// Kill any running mcpmux processes to prevent single-instance conflicts
function killMcpmuxProcesses(): void {
  try {
    if (process.platform === 'win32') {
      // On Windows, use taskkill (matches exact process name)
      spawnSync('taskkill', ['/F', '/IM', 'mcpmux.exe'], { stdio: 'ignore' });
    } else {
      // On Linux, kill only the exact mcpmux binary (not anything with "mcpmux" in its cmdline).
      // pkill without -f matches the process name only, which is safer than -f (full cmdline).
      spawnSync('pkill', ['-9', 'mcpmux'], { stdio: 'ignore' });
    }
    console.log('[e2e] Killed any existing mcpmux processes');
  } catch (error) {
    // Ignore errors - process may not exist
  }
}

function killTauriDriverProcesses(): void {
  try {
    if (process.platform === 'win32') {
      spawnSync('taskkill', ['/F', '/T', '/IM', 'tauri-driver.exe'], { stdio: 'ignore' });
      // tauri-driver starts msedgedriver.exe as a child. A driver killed earlier (e.g. by
      // closeTauriDriver) leaves it orphaned, outside the /T tree, still holding port 4445.
      spawnSync('taskkill', ['/F', '/IM', 'msedgedriver.exe'], { stdio: 'ignore' });
    } else {
      spawnSync('pkill', ['-9', 'tauri-driver'], { stdio: 'ignore' });
    }
  } catch {
    // Ignore errors - the driver may not be running.
  }
}

function killGatewayPortProcess(): void {
  try {
    if (process.platform === 'win32') {
      const result = spawnSync('netstat', ['-ano', '-p', 'tcp'], { encoding: 'utf8' });
      const pids = new Set<number>();

      for (const line of result.stdout.split(/\r?\n/)) {
        const match = line.match(/^\s*TCP\s+\S+:45818\s+\S+\s+\S+\s+(\d+)\s*$/i);
        if (match) {
          pids.add(Number(match[1]));
        }
      }

      for (const pid of pids) {
        spawnSync('taskkill', ['/F', '/PID', String(pid)], { stdio: 'ignore' });
      }
    } else {
      spawnSync('fuser', ['-k', '-9', '45818/tcp'], { stdio: 'ignore' });
    }
  } catch {
    // Ignore errors - nothing may be listening on the gateway port.
  }
}

// Clear single-instance lock file
function clearSingleInstanceLock(): void {
  try {
    const lockFilePath = path.join(APP_DATA_DIR, '.single-instance-lock');
    if (fs.existsSync(lockFilePath)) {
      fs.unlinkSync(lockFilePath);
      console.log('[e2e] Cleared single-instance lock');
    }
  } catch (error) {
    console.warn('[e2e] Failed to clear single-instance lock:', error);
  }
}

function onShutdown(fn: () => void) {
  const cleanup = () => {
    try {
      fn();
    } finally {
      process.exit();
    }
  };

  process.on('exit', cleanup);
  process.on('SIGINT', cleanup);
  process.on('SIGTERM', cleanup);
}

// Ensure tauri-driver and mock servers are closed when test process exits
onShutdown(() => {
  closeTauriDriver();
  stopMockServers();
});

export const config: Options.Testrunner = {
  // Connect to tauri-driver
  host: '127.0.0.1',
  port: 4444,

  autoCompileOpts: {
    autoCompile: true,
    tsNodeOpts: {
      project: './tsconfig.json',
      transpileOnly: true,
    },
  },

  specs: ['./specs/**/*.wdio.ts'],
  exclude: [],

  maxInstances: 1, // Tauri only supports one instance

  capabilities: [
    {
      maxInstances: 1,
      'tauri:options': {
        application: APP_PATH,
      },
    } as WebdriverIO.Capabilities,
  ],

  logLevel: 'info',
  bail: 0,
  waitforTimeout: 10000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,

  framework: 'mocha',
  reporters: [
    'spec',
    [
      'junit',
      {
        outputDir: './tests/e2e/reports/',
        outputFileFormat: function (options) {
          return `wdio-junit-${options.cid}.xml`;
        },
      },
    ],
    // Only enable video reporter when explicitly requested via SAVE_ALL_VIDEOS=true.
    // The video reporter takes a screenshot after every WebDriver command, including
    // during session teardown. This races with deleteSession killing the Tauri app,
    // causing UND_ERR_SOCKET errors that mark passing specs as FAILED.
    ...(process.env.SAVE_ALL_VIDEOS === 'true'
      ? [
          [
            video,
            {
              saveAllVideos: true,
              videoSlowdownMultiplier: 1,
              outputDir: './tests/e2e/videos/',
            },
          ],
        ]
      : []),
  ],

  mochaOpts: {
    ui: 'bdd',
    timeout: 60000,
  },

  // Wait for the webview bundle (main.tsx) to expose the test API before any
  // spec runs. Specs whose first step is an API call (e.g. comprehensive's
  // `before`) otherwise race app boot — seen on WebView2 as
  // "Tauri Test API not available".
  before: async function () {
    await browser.waitUntil(
      () =>
        browser
          .execute(
            () => !!(window as unknown as { __TAURI_TEST_API__?: unknown }).__TAURI_TEST_API__
          )
          .catch(() => false),
      {
        timeout: 30000,
        interval: 250,
        timeoutMsg: 'window.__TAURI_TEST_API__ was not exposed within 30s',
      }
    );
  },

  // Take screenshot on test failure
  afterTest: async function (test, context, { error }) {
    if (error) {
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      // Sanitize test title: replace invalid filename chars (NTFS: " : < > | * ? \r \n) and spaces
      const safeTitle = test.title.replace(/[":*?<>|\r\n\\\/]+/g, '-').replace(/\s+/g, '_');
      const filename = `./tests/e2e/screenshots/FAIL-${safeTitle}-${timestamp}.png`;
      try {
        await browser.saveScreenshot(filename);
        console.log(`[e2e] Screenshot saved: ${filename}`);
      } catch {
        // Screenshot may fail if tauri-driver crashed
        if (tauriDriverCrashed) {
          console.error(`[e2e] Cannot save screenshot - tauri-driver crashed`);
        }
      }
    }
  },

  // Verify prerequisites before running
  onPrepare: async function () {
    // Create output directories (gitignored; needed for CI and fresh clones)
    const screenshotsDir = path.resolve('./tests/e2e/screenshots');
    const videosDir = path.resolve('./tests/e2e/videos');
    const reportsDir = path.resolve('./tests/e2e/reports');
    fs.mkdirSync(screenshotsDir, { recursive: true });
    fs.mkdirSync(videosDir, { recursive: true });
    fs.mkdirSync(reportsDir, { recursive: true });

    // Verify tauri-driver is installed
    const hasTauriDriver = checkTauriDriver();
    if (!hasTauriDriver) {
      console.error('\n[ERROR] tauri-driver is not installed.');
      console.error('Install it with: cargo install tauri-driver --locked\n');
      process.exit(1);
    }

    // Verify app is built
    checkAppBuilt();

    // Kill any leftover mcpmux and tauri-driver processes and clear all app data BEFORE
    // tauri-driver starts the app. This avoids EBUSY errors from trying
    // to delete the SQLite DB while the app still holds a lock on it.
    killMcpmuxProcesses();
    killTauriDriverProcesses();
    killGatewayPortProcess();
    // Brief pause to let processes fully exit
    await new Promise((resolve) => setTimeout(resolve, 2000));
    clearSingleInstanceLock();
    clearAppData();

    // Clear bundle cache so app fetches from our mock
    clearBundleCache();

    // Kill any leftover mock servers from previous runs (prevents EADDRINUSE)
    killPortProcesses();
    await new Promise((resolve) => setTimeout(resolve, 1000));

    // Start mock servers
    await startMockServers();
  },

  // Start tauri-driver before the session starts.
  // Performs aggressive cleanup of leftover processes from the previous spec
  // before spawning a fresh tauri-driver, then waits for it to be ready.
  beforeSession: async function () {
    shouldExit = false;
    tauriDriverCrashed = false;

    // --- Aggressive cleanup from previous spec ---
    // Kill any leftover tauri-driver processes (may remain if previous spec crashed).
    // This is safe to do here because no tauri-driver should be running between specs.
    killTauriDriverProcesses();
    // Kill any leftover mcpmux app processes and clear single-instance lock
    killMcpmuxProcesses();
    clearSingleInstanceLock();

    // Free the gateway port (45818) in case mcpmux didn't release it
    killGatewayPortProcess();

    // Wait for OS to fully reclaim process resources (ports, file locks, etc.)
    await new Promise((resolve) => setTimeout(resolve, 2000));

    // --- Spawn fresh tauri-driver ---
    const tauriDriverPath = path.resolve(
      os.homedir(),
      '.cargo',
      'bin',
      process.platform === 'win32' ? 'tauri-driver.exe' : 'tauri-driver'
    );

    // Pass environment variables to tauri-driver (which passes to the app)
    // MCPMUX_E2E_TEST=1 enables test-only endpoints:
    //   - POST /oauth/consent/approve (HTTP consent for programmatic OAuth flow)
    //   - approve_oauth_client Tauri IPC command
    tauriDriver = spawn(tauriDriverPath, [], {
      stdio: [null, process.stdout, process.stderr],
      env: {
        ...process.env,
        MCPMUX_REGISTRY_URL: `http://localhost:${MOCK_BUNDLE_API_PORT}`,
        MCPMUX_E2E_TEST: '1',
        MCPMUX_E2E_DATA_DIR: APP_DATA_DIR,
      },
    });

    tauriDriver.on('error', (error) => {
      console.error('[tauri-driver] Error:', error);
      // Don't call process.exit(1) - it kills the worker before JUnit XML
      // reports are finalized, resulting in malformed/empty XML files.
      // Let WebdriverIO handle the failure naturally via connection errors.
      tauriDriverCrashed = true;
    });

    tauriDriver.on('exit', (code) => {
      if (!shouldExit) {
        console.error('[tauri-driver] Exited unexpectedly with code:', code);
        // Don't call process.exit(1) - let the test fail gracefully so that
        // JUnit XML reports are properly written. WebdriverIO will detect
        // the broken connection and fail the affected tests.
        tauriDriverCrashed = true;
      }
    });

    // Wait for tauri-driver to be ready before letting WebdriverIO create a session.
    // Without this, WebdriverIO may send POST /session before tauri-driver is listening,
    // causing a 2-minute timeout (connectionRetryTimeout) and cascading failures.
    await waitForTauriDriverReady(30000);
  },

  // Stop tauri-driver after the session.
  // Uses graceful SIGTERM only — aggressive cleanup (pkill -9) is deferred to
  // the next spec's beforeSession to avoid racing with WebdriverIO's own
  // deleteSession call, which would cause cascading failures in subsequent specs.
  afterSession: async function () {
    // Mark as crashed to prevent afterTest screenshot attempts against a dying session.
    // On Linux, WebKitGTK tears down the process synchronously on deleteSession,
    // so any pending screenshot requests will hit a dead socket.
    tauriDriverCrashed = true;

    closeTauriDriver();

    // Brief pause to let tauri-driver/mcpmux handle SIGTERM gracefully
    await new Promise((resolve) => setTimeout(resolve, 1000));
  },

  // Clean up mock servers and the last spec's driver after all tests complete
  onComplete: function () {
    stopMockServers();
    killTauriDriverProcesses();
  },
};
