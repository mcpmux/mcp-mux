import { describe, it, expect, beforeEach, vi } from 'vitest';
import { openUrl as openUrlWithPlugin } from '@tauri-apps/plugin-opener';
import { openUrl } from '@/lib/api/gateway';
import { openRedirectUrl } from '@/lib/oauthRedirect';

vi.mock('@/lib/api/gateway', () => ({ openUrl: vi.fn() }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

const openUrlMock = vi.mocked(openUrl);
const pluginOpenMock = vi.mocked(openUrlWithPlugin);

beforeEach(() => {
  openUrlMock.mockReset();
  pluginOpenMock.mockReset();
});

describe('openRedirectUrl', () => {
  it('hands the redirect to the open_url command', async () => {
    openUrlMock.mockResolvedValueOnce(undefined);

    await openRedirectUrl('cursor://anysphere.cursor-mcp/oauth/callback?code=mc_1');

    expect(openUrlMock).toHaveBeenCalledWith(
      'cursor://anysphere.cursor-mcp/oauth/callback?code=mc_1'
    );
    expect(pluginOpenMock).not.toHaveBeenCalled();
  });

  it('falls back to the opener plugin when open_url fails', async () => {
    openUrlMock.mockRejectedValueOnce(new Error('no handler'));
    pluginOpenMock.mockResolvedValueOnce(undefined);

    await openRedirectUrl('https://chatgpt.com/connector/oauth/abc?code=mc_1');

    expect(pluginOpenMock).toHaveBeenCalledWith(
      'https://chatgpt.com/connector/oauth/abc?code=mc_1'
    );
  });

  it('never navigates the app window when both openers refuse', async () => {
    const before = window.location.href;
    openUrlMock.mockRejectedValueOnce(new Error('no handler'));
    pluginOpenMock.mockRejectedValueOnce(new Error('scheme not allowed'));

    await expect(openRedirectUrl('javascript:alert(1)//?code=mc_1')).rejects.toThrow(
      'scheme not allowed'
    );
    expect(window.location.href).toBe(before);
  });
});
