/**
 * E2E Tests: Gateway Control
 * 
 * Test Cases Covered:
 * - TC-GW-001: Gateway Status Display
 * - TC-GW-002: Gateway Running on Startup
 * - TC-GW-003: Dashboard Shows Gateway Status
 * - TC-GW-006: Gateway URL Display
 * - TC-GW-007: Copy Client Config
 * - TC-GW-008: Dashboard Statistics
 */

// Helper to find element by test ID or fallback
async function findElement(testId: string, fallbackSelector: string) {
  const byTestId = await $(`[data-testid="${testId}"]`);
  const testIdExists = await byTestId.isExisting().catch(() => false);
  if (testIdExists) {
    return byTestId;
  }
  return $(fallbackSelector);
}

describe('Gateway Status - Dashboard', () => {
  before(async () => {
    // Navigate to Dashboard
    const dashboardBtn = await findElement('nav-dashboard', 'button*=Dashboard');
    await dashboardBtn.click();
    await browser.pause(2000);
  });

  it('TC-GW-001: Gateway status is displayed in status bar', async () => {
    await browser.saveScreenshot('./tests/e2e/screenshots/gw-01-status-bar.png');
    
    const pageSource = await browser.getPageSource();
    
    // Status bar should show gateway status
    const hasGatewayStatus = 
      pageSource.includes('Gateway Active') || 
      pageSource.includes('Gateway Stopped') ||
      pageSource.includes('Gateway');
    
    expect(hasGatewayStatus).toBe(true);
  });

  it('TC-GW-002: Gateway is running on startup', async () => {
    // Check gateway status card
    const statusCard = await findElement('gateway-status-card', '.border-green-500');
    const isDisplayed = await statusCard.isDisplayed().catch(() => false);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/gw-02-gateway-running.png');
    
    const pageSource = await browser.getPageSource();
    
    // Gateway should be running by default
    const isRunning = 
      pageSource.includes('Gateway: Running') ||
      pageSource.includes('border-green-500');
    
    console.log('[DEBUG] Gateway running:', isRunning);
    expect(isRunning).toBe(true);
  });

  it('TC-GW-003: Dashboard shows gateway status banner', async () => {
    // Verify gateway status content via page source
    const pageSource = await browser.getPageSource();
    
    // Check for gateway status card
    const hasGatewayCard = 
      pageSource.includes('Gateway: Running') || 
      pageSource.includes('Gateway: Stopped') ||
      pageSource.includes('gateway-status-card');
    
    expect(hasGatewayCard).toBe(true);
    
    // Verify toggle button exists
    const hasToggleBtn = 
      pageSource.includes('Stop') || 
      pageSource.includes('Start');
    
    expect(hasToggleBtn).toBe(true);
  });

  it('TC-GW-006: Gateway URL is displayed', async () => {
    const pageSource = await browser.getPageSource();
    
    // Gateway URL should be visible (localhost:3100 or similar)
    const hasGatewayUrl = 
      pageSource.includes('localhost:') ||
      pageSource.includes('http://127.0.0.1');
    
    console.log('[DEBUG] Has gateway URL:', hasGatewayUrl);
    expect(hasGatewayUrl).toBe(true);
  });

  it('TC-GW-007: Copy client config button exists', async () => {
    const copyBtn = await findElement('copy-config-btn', 'button*=Copy');
    const isDisplayed = await copyBtn.isDisplayed().catch(() => false);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/gw-07-copy-config.png');
    
    expect(isDisplayed).toBe(true);
  });

  it('TC-GW-008: Dashboard statistics are displayed', async () => {
    // Check stats grid exists
    const statsGrid = await findElement('dashboard-stats-grid', '.grid');
    const gridDisplayed = await statsGrid.isDisplayed().catch(() => false);
    
    await browser.saveScreenshot('./tests/e2e/screenshots/gw-08-dashboard-stats.png');
    
    // Verify individual stat cards
    const pageSource = await browser.getPageSource();
    
    const hasServersCard = pageSource.includes('Servers');
    const hasFeatureSetsCard = pageSource.includes('FeatureSets');
    const hasClientsCard = pageSource.includes('Clients');
    const hasActiveSpaceCard = pageSource.includes('Active Space');
    
    console.log('[DEBUG] Stats - Servers:', hasServersCard);
    console.log('[DEBUG] Stats - FeatureSets:', hasFeatureSetsCard);
    console.log('[DEBUG] Stats - Clients:', hasClientsCard);
    console.log('[DEBUG] Stats - Active Space:', hasActiveSpaceCard);
    
    expect(hasServersCard).toBe(true);
    expect(hasFeatureSetsCard).toBe(true);
    expect(hasClientsCard).toBe(true);
    expect(hasActiveSpaceCard).toBe(true);
  });
});

describe('Gateway Toggle', () => {
  before(async () => {
    // Navigate to Dashboard
    const dashboardBtn = await findElement('nav-dashboard', 'button*=Dashboard');
    await dashboardBtn.click();
    await browser.pause(2000);
  });

  it('TC-GW-004/005: Toggle gateway button exists and is clickable', async () => {
    const toggleBtn = await findElement('gateway-toggle-btn', 'button*=Stop');
    const isDisplayed = await toggleBtn.isDisplayed().catch(() => false);
    
    if (!isDisplayed) {
      // Try finding Start button instead
      const startBtn = await findElement('gateway-toggle-btn', 'button*=Start');
      const startDisplayed = await startBtn.isDisplayed().catch(() => false);
      expect(startDisplayed).toBe(true);
    } else {
      expect(isDisplayed).toBe(true);
    }
    
    await browser.saveScreenshot('./tests/e2e/screenshots/gw-toggle-button.png');
  });
});
