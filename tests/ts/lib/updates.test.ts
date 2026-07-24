import { describe, it, expect, beforeEach, vi } from 'vitest';
import { check, type Update } from '@tauri-apps/plugin-updater';

import {
  getUpdateChannel,
  setUpdateChannel,
  checkForUpdate,
  UPDATE_CHANNEL_HEADER,
} from '@/lib/updates';

const apiCallMock = vi.hoisted(() => vi.fn());
const isTauriMock = vi.hoisted(() => vi.fn(() => true));
const checkMock = vi.mocked(check);

vi.mock('@/lib/api/transport', () => ({
  apiCall: apiCallMock,
}));

vi.mock('@/lib/backend/data/transport', () => ({
  isTauri: isTauriMock,
}));

beforeEach(() => {
  apiCallMock.mockReset();
  isTauriMock.mockReturnValue(true);
  checkMock.mockReset();
});

describe('getUpdateChannel', () => {
  it('returns the stored channel', async () => {
    apiCallMock.mockResolvedValueOnce('prerelease');
    await expect(getUpdateChannel()).resolves.toBe('prerelease');
    expect(apiCallMock).toHaveBeenCalledWith('get_update_channel');
  });

  it('defaults to stable for unknown or missing values', async () => {
    apiCallMock.mockResolvedValueOnce('garbage');
    await expect(getUpdateChannel()).resolves.toBe('stable');
  });

  it('defaults to stable when the command throws', async () => {
    apiCallMock.mockRejectedValueOnce(new Error('command unavailable'));
    await expect(getUpdateChannel()).resolves.toBe('stable');
  });
});

describe('setUpdateChannel', () => {
  it('persists and returns the normalized value', async () => {
    apiCallMock.mockResolvedValueOnce('prerelease');
    await expect(setUpdateChannel('prerelease')).resolves.toBe('prerelease');
    expect(apiCallMock).toHaveBeenCalledWith('set_update_channel', { channel: 'prerelease' });
  });
});

describe('checkForUpdate', () => {
  it('forwards the stored channel as the update header (prerelease)', async () => {
    apiCallMock.mockResolvedValueOnce('prerelease');
    checkMock.mockResolvedValueOnce(null);
    await checkForUpdate();
    expect(checkMock).toHaveBeenCalledWith({
      headers: { [UPDATE_CHANNEL_HEADER]: 'prerelease' },
    });
  });

  it('falls back to the stable header when the channel is unavailable', async () => {
    apiCallMock.mockRejectedValueOnce(new Error('no setting'));
    checkMock.mockResolvedValueOnce(null);
    await checkForUpdate();
    expect(checkMock).toHaveBeenCalledWith({
      headers: { [UPDATE_CHANNEL_HEADER]: 'stable' },
    });
  });

  it('returns the update handle from check()', async () => {
    apiCallMock.mockResolvedValueOnce('stable');
    const handle = { version: '1.2.3' } as unknown as Update;
    checkMock.mockResolvedValueOnce(handle);
    await expect(checkForUpdate()).resolves.toBe(handle);
  });

  it('returns null on web admin (non-Tauri)', async () => {
    isTauriMock.mockReturnValue(false);
    await expect(checkForUpdate()).resolves.toBeNull();
    expect(checkMock).not.toHaveBeenCalled();
  });
});
