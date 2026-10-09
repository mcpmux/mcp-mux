import { describe, it, expect, beforeEach, vi } from 'vitest';

vi.mock('posthog-js', () => ({
  default: {
    init: vi.fn(),
    register: vi.fn(),
    capture: vi.fn(),
    opt_in_capturing: vi.fn(),
    opt_out_capturing: vi.fn(),
    has_opted_out_capturing: vi.fn(() => false),
  },
}));

beforeEach(() => {
  vi.resetModules();
  vi.unstubAllEnvs();
});

describe('initAnalytics', () => {
  it('never lets PostHog load scripts into the app window', async () => {
    vi.stubEnv('VITE_POSTHOG_KEY', 'phc_test');
    const posthog = (await import('posthog-js')).default;
    const { initAnalytics } = await import('@/lib/analytics');

    initAnalytics('1.2.3');

    expect(posthog.init).toHaveBeenCalledWith(
      'phc_test',
      expect.objectContaining({
        disable_external_dependency_loading: true,
        disable_session_recording: true,
        disable_surveys: true,
        autocapture: false,
      })
    );
  });

  it('does nothing without a key', async () => {
    vi.stubEnv('VITE_POSTHOG_KEY', '');
    const posthog = (await import('posthog-js')).default;
    vi.mocked(posthog.init).mockClear();
    const { initAnalytics } = await import('@/lib/analytics');

    initAnalytics('1.2.3');

    expect(posthog.init).not.toHaveBeenCalled();
  });
});
