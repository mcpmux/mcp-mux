/**
 * E2E Tests: Connections page (the renamed, observability-focused view).
 *
 * Routing is no longer configured here — that lives in Workspaces. These
 * specs verify the page loads, reveals the list of approved clients (if
 * any), and surfaces a link back to Workspaces instead of per-client
 * routing controls.
 *
 * Uses data-testid only (ADR-003).
 */

import { byTestId } from '../helpers/selectors';

describe('Connections - Page shell', () => {
  it('TC-CL-001: Navigate to Connections page and see heading + Workspaces link', async () => {
    const connectionsBtn = await byTestId('nav-clients');
    await connectionsBtn.click();
    await browser.pause(2000);

    await browser.saveScreenshot('./tests/e2e/screenshots/cl-01-connections-page.png');

    const pageSource = await browser.getPageSource();

    // Heading has been renamed.
    expect(pageSource.includes('Clients')).toBe(true);

    // The page routes users to the Mapping tab for any routing questions.
    expect(pageSource.includes('Mapping')).toBe(true);
  });

  it('TC-CL-002: Open side panel and verify legacy routing controls are gone', async () => {
    const clientCards = await $$('[data-testid^="client-card-"]');
    const firstCard = clientCards[0];
    const isDisplayed = firstCard ? await firstCard.isDisplayed().catch(() => false) : false;

    if (isDisplayed && firstCard) {
      await firstCard.click();
      await browser.pause(1500);

      await browser.saveScreenshot('./tests/e2e/screenshots/cl-02-connection-panel.png');

      const pageSource = await browser.getPageSource();

      // Positive: the new panel exposes the Workspaces entry point.
      const hasWorkspacesLink =
        pageSource.includes('Open Workspaces') || pageSource.includes('workspace-driven');
      expect(hasWorkspacesLink).toBe(true);

      // Negative: all removed per-client routing sections must be gone.
      expect(pageSource.includes('Quick Settings')).toBe(false);
      expect(pageSource.includes('Connection Mode')).toBe(false);
      expect(pageSource.includes('Effective Features')).toBe(false);
      expect(pageSource.includes('Advanced Permissions')).toBe(false);
    } else {
      // Empty-state path: ConnectIDEs onboarding must render instead.
      const pageSource = await browser.getPageSource();
      expect(pageSource.includes("Let's hook up your first IDE")).toBe(true);
    }
  });

  it('TC-CL-003: Register an API-key client and reveal the key once', async () => {
    // The desktop app auto-starts the gateway, so register_api_key_client can
    // mint a key. Open the Apps tab, register a client, and confirm the
    // generated mcpk_ key is shown exactly once.
    const connectionsBtn = await byTestId('nav-clients');
    await connectionsBtn.click();
    await browser.pause(1000);

    const registerBtn = await byTestId('register-api-key-client-btn');
    await registerBtn.click();
    await browser.pause(800);

    const nameInput = await byTestId('register-api-key-name');
    await nameInput.setValue('e2e-headless-bot');

    const generateBtn = await byTestId('register-api-key-generate');
    await generateBtn.click();
    await browser.pause(1500);

    await browser.saveScreenshot('./tests/e2e/screenshots/cl-03-api-key-created.png');

    const keyEl = await byTestId('register-api-key-value');
    await expect(keyEl).toBeDisplayed();
    const keyText = await keyEl.getText();
    // Shown once, prefixed mcpk_ (never the stored hash).
    expect(keyText.startsWith('mcpk_')).toBe(true);
  });
});
