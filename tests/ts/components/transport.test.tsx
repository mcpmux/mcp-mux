/**
 * Transport facade (`src/lib/transport`). In the test env `setup.ts` marks the
 * environment as Tauri (`__TAURI_INTERNALS__`), so the facade selects the Tauri
 * transport — which must forward to `invoke`/`listen` with the SAME arity and
 * argument shapes the call sites used (the contract the desktop relies on), and
 * expose the desktop capability set.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { call, subscribe, capabilities, transport } from '@/lib/transport';

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

describe('transport facade (Tauri env)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('call(cmd) forwards to invoke(cmd) with no args', async () => {
    invokeMock.mockResolvedValueOnce('ok');
    await call('list_spaces');
    expect(invokeMock).toHaveBeenCalledWith('list_spaces');
  });

  it('call(cmd, args) forwards to invoke(cmd, args)', async () => {
    invokeMock.mockResolvedValueOnce({ id: '1' });
    await call('get_space', { id: '1' });
    expect(invokeMock).toHaveBeenCalledWith('get_space', { id: '1' });
  });

  it('call returns invoke result', async () => {
    invokeMock.mockResolvedValueOnce([{ id: 'a' }]);
    const r = await call<Array<{ id: string }>>('list_spaces');
    expect(r).toEqual([{ id: 'a' }]);
  });

  it('subscribe maps to listen and returns an unsubscribe fn', async () => {
    const unlisten = vi.fn();
    listenMock.mockResolvedValueOnce(unlisten);
    const handler = vi.fn();
    const unsub = subscribe('client-changed', handler);

    expect(listenMock).toHaveBeenCalledWith('client-changed', expect.any(Function));

    // The listen callback delivers `event.payload` to the handler as payload.
    const cb = listenMock.mock.calls[0][1] as (e: { payload: unknown }) => void;
    cb({ payload: { action: 'registered' } });
    expect(handler).toHaveBeenCalledWith({ action: 'registered' });

    // Unsubscribe resolves the underlying unlisten.
    unsub();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalled();
  });

  it('exposes the desktop capability set', () => {
    expect(capabilities).toMatchObject({
      dialog: true,
      fsWrite: true,
      tray: true,
      deepLink: true,
      autostart: true,
      updater: true,
    });
    expect(transport.capabilities).toBe(capabilities);
  });
});
