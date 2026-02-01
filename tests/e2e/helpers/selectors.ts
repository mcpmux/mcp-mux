/**
 * E2E Test Selectors - data-testid only (ADR-003)
 * Use $('[data-testid="x"]') for all element selection.
 */

/** Get element by data-testid */
export const byTestId = (testId: string) => $(`[data-testid="${testId}"]`);
