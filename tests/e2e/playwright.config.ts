/**
 * Playwright config for WEB-ONLY testing (no Tauri backend)
 * 
 * Use this for:
 * - UI component testing
 * - Layout/styling verification
 * - Static page testing
 * 
 * NOT for:
 * - Testing Tauri commands (use test:e2e with WebdriverIO)
 * - Testing data from backend
 * - Full integration testing
 * 
 * These tests mock Tauri IPC and only test the web layer.
 */

import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './specs',
  testMatch: '**/*.spec.ts', // Only .spec.ts files (not .wdio.ts)
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ['html', { outputFolder: './reports/html' }],
    ['junit', { outputFile: './reports/junit.xml' }],
    ['list'],
  ],
  use: {
    baseURL: 'http://localhost:1420',
    trace: 'on-first-retry',
    video: 'retain-on-failure',
    screenshot: 'only-on-failure',
    launchOptions: {
      args: ['--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage'],
    },
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
    {
      name: 'webkit',
      use: { ...devices['Desktop Safari'] },
    },
  ],
  webServer: {
    command: 'pnpm --filter @mcpmux/desktop dev:web',
    url: 'http://localhost:1420',
    reuseExistingServer: !process.env.CI,
    cwd: '../..',
    timeout: 120000,
  },
});
