import { describe, it, expect, beforeEach, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';
import {
  getUpdateChannel,
  setUpdateChannel,
  checkForUpdate,
  UPDATE_CHANNEL_HEADER,
} from '@/lib/updates';

const invokeMock = vi.mocked(invoke);
const checkMock = vi.mocked(check);

beforeEach(() => {
  invokeMock.mockReset();
  checkMock.mockReset();
});

describe('getUpdateChannel', () => {
  it('returns the stored channel', async () => {
    invokeMock.mockResolvedValueOnce('prerelease');
    await expect(getUpdateChannel()).resolves.toBe('prerelease');
    expect(invokeMock).toHaveBeenCalledWith('get_update_channel');
  });

  it('defaults to stable for unknown or missing values', async () => {
    invokeMock.mockResolvedValueOnce('garbage');
    await expect(getUpdateChannel()).resolves.toBe('stable');
  });

  it('defaults to stable when the command throws', async () => {
    invokeMock.mockRejectedValueOnce(new Error('command unavailable'));
    await expect(getUpdateChannel()).resolves.toBe('stable');
  });
});

describe('setUpdateChannel', () => {
  it('persists and returns the normalized value', async () => {
    invokeMock.mockResolvedValueOnce('prerelease');
    await expect(setUpdateChannel('prerelease')).resolves.toBe('prerelease');
    expect(invokeMock).toHaveBeenCalledWith('set_update_channel', { channel: 'prerelease' });
  });
});

describe('checkForUpdate', () => {
  it('forwards the stored channel as the update header (prerelease)', async () => {
    invokeMock.mockResolvedValueOnce('prerelease');
    checkMock.mockResolvedValueOnce(null);
    await checkForUpdate();
    expect(checkMock).toHaveBeenCalledWith({
      headers: { [UPDATE_CHANNEL_HEADER]: 'prerelease' },
    });
  });

  it('falls back to the stable header when the channel is unavailable', async () => {
    invokeMock.mockRejectedValueOnce(new Error('no setting'));
    checkMock.mockResolvedValueOnce(null);
    await checkForUpdate();
    expect(checkMock).toHaveBeenCalledWith({
      headers: { [UPDATE_CHANNEL_HEADER]: 'stable' },
    });
  });

  it('returns the update handle from check()', async () => {
    invokeMock.mockResolvedValueOnce('stable');
    const handle = { version: '1.2.3' } as unknown as Update;
    checkMock.mockResolvedValueOnce(handle);
    await expect(checkForUpdate()).resolves.toBe(handle);
  });
});
