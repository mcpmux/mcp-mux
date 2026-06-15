/**
 * E2E Tests: Built-in Servers page.
 *
 * The tab that houses McpMux's own bundled MCP servers. Today it surfaces one
 * concrete server — "Tool Optimization" (the mcpmux_* self-management tools) —
 * with a master enable switch, moved here out of Settings.
 *
 * Uses data-testid only (ADR-003).
 */

import { byTestId, safeClick } from '../helpers/selectors';

describe('Built-in Servers - Page shell', () => {
  it('TC-BS-001: Navigate to Built-in Servers and see Tool Optimization', async () => {
    const nav = await byTestId('nav-builtin-servers');
    await safeClick(nav);
    await browser.pause(1200);

    await browser.saveScreenshot('./tests/e2e/screenshots/bs-01-page.png');

    const src = await browser.getPageSource();
    expect(src.includes('Built-in')).toBe(true);
    expect(src.includes('Tool Optimization')).toBe(true);

    // The Tool Optimization server card + its enable switch are present.
    const card = await byTestId('builtin-server-tool-optimization');
    expect(await card.isDisplayed()).toBe(true);
    const toggle = await byTestId('meta-tools-enabled-switch');
    expect(await toggle.isDisplayed()).toBe(true);
  });
});
