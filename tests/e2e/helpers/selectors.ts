/**
 * E2E Test Selectors - data-testid only (ADR-003)
 * Use $('[data-testid="x"]') for all element selection.
 */

// CI-friendly timeouts (Windows CI is slower)
export const TIMEOUT = {
  short: 5000,
  medium: 15000,  // Default for waitForDisplayed/Clickable
  long: 30000,    // For slow operations like MCP connections
  veryLong: 60000,
};

/** Get element by data-testid */
export const byTestId = (testId: string) => $(`[data-testid="${testId}"]`);

/** Wait for any modal overlay to close (backdrop with blur) */
export async function waitForModalClose(timeout = TIMEOUT.medium): Promise<void> {
  const overlay = await $('.fixed.inset-0.bg-black\\/20');
  if (await overlay.isExisting()) {
    await overlay.waitForDisplayed({ timeout, reverse: true });
  }
}

/** Click element after ensuring no modal overlay is blocking */
export async function safeClick(element: WebdriverIO.Element, timeout = TIMEOUT.medium): Promise<void> {
  await waitForModalClose(timeout);
  await element.waitForClickable({ timeout });
  await element.click();
}
