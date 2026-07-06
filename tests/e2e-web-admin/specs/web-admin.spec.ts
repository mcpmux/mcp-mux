import { test, expect } from '@playwright/test';

const TOKEN = 'e2e-web-token';

/**
 * The embedded desktop React app, served headless at /app, renders in a real
 * browser and drives the command-mirror RPC — with zero Tauri-IPC errors (the
 * events shim + HTTP transport replace raw IPC). Then it does the core write
 * loop (create a Space) against the RPC and confirms it persisted.
 */
test('embedded web admin renders, drives RPC, and does the core loop', async ({ page, request }) => {
  const rpcCalls = new Set<string>();
  const ipcErrors: string[] = [];
  page.on('request', (r) => {
    const u = r.url();
    if (u.includes('/admin/api/rpc/')) rpcCalls.add(u.split('/admin/api/rpc/')[1].split('?')[0]);
  });
  page.on('pageerror', (e) => {
    if (String(e).includes('transformCallback')) ipcErrors.push(String(e));
  });

  // `/` redirects to `/app/`.
  await page.goto('/');
  await expect(page).toHaveURL(/\/app\/?$/);

  // The web-admin login gate.
  await page.getByTestId('web-admin-token').fill(TOKEN);
  await page.getByTestId('web-admin-signin').click();

  // The full app shell renders (sidebar nav).
  await expect(page.locator('[data-testid^="nav-"]').first()).toBeVisible({ timeout: 10_000 });
  expect(await page.locator('[data-testid^="nav-"]').count()).toBeGreaterThan(3);

  // It drove the RPC mirror on load.
  await page.waitForTimeout(1500);
  expect(rpcCalls.has('list_spaces')).toBeTruthy();
  expect(rpcCalls.has('get_gateway_status')).toBeTruthy();

  // Zero Tauri-IPC crashes in the browser.
  expect(ipcErrors, ipcErrors.join('\n')).toHaveLength(0);

  // Core write loop over the RPC (via Playwright's API request context — same
  // serve binary + RPC the app uses): create a Space, confirm it persisted.
  const hdr = { Authorization: `Bearer ${TOKEN}` };
  const before = await (await request.post('/admin/api/rpc/list_spaces', { headers: hdr })).json();
  await request.post('/admin/api/rpc/create_space', {
    headers: hdr,
    data: { name: 'E2E Space' },
  });
  const after = await (await request.post('/admin/api/rpc/list_spaces', { headers: hdr })).json();
  expect(after.length).toBe(before.length + 1);
  expect(after.some((s: { name: string }) => s.name === 'E2E Space')).toBeTruthy();
});

/**
 * The gate rejects a wrong token (the app never renders behind it).
 */
test('web admin gate rejects an invalid token', async ({ page }) => {
  await page.goto('/app/');
  await page.getByTestId('web-admin-token').fill('wrong-token');
  await page.getByTestId('web-admin-signin').click();
  await expect(page.getByTestId('web-admin-error')).toBeVisible();
  // The app shell must NOT render.
  await expect(page.locator('[data-testid^="nav-"]')).toHaveCount(0);
});
