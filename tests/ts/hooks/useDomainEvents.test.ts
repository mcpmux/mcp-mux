import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { listen } from '@tauri-apps/api/event';
import { useDomainEvents } from '@/hooks/useDomainEvents';

const mockListen = vi.mocked(listen);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('useDomainEvents', () => {
  it('subscribe calls listen with the correct channel name', () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const { result } = renderHook(() => useDomainEvents());

    act(() => {
      result.current.subscribe('server-status-changed', vi.fn());
    });

    expect(mockListen).toHaveBeenCalledWith(
      'server-status-changed',
      expect.any(Function)
    );
  });

  it('subscribe callback receives event payload', async () => {
    let capturedHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (_channel, handler) => {
      capturedHandler = handler as (event: unknown) => void;
      return vi.fn();
    });

    const callback = vi.fn();
    const { result } = renderHook(() => useDomainEvents());

    act(() => {
      result.current.subscribe('space-changed', callback);
    });

    // Simulate an event
    const payload = { action: 'created', space_id: 'sp-1', name: 'New Space' };
    act(() => {
      capturedHandler?.({ payload });
    });

    expect(callback).toHaveBeenCalledWith(payload);
  });

  it('subscribe returns cleanup that calls unlisten', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const { result } = renderHook(() => useDomainEvents());

    let cleanup: (() => void) | undefined;
    act(() => {
      cleanup = result.current.subscribe('server-changed', vi.fn());
    });

    // Wait for listen promise to resolve
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });

    act(() => {
      cleanup?.();
    });

    expect(unlisten).toHaveBeenCalled();
  });

  it('subscribe updates lastEvent state', async () => {
    let capturedHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (_channel, handler) => {
      capturedHandler = handler as (event: unknown) => void;
      return vi.fn();
    });

    const { result } = renderHook(() => useDomainEvents());

    act(() => {
      result.current.subscribe('gateway-changed', vi.fn());
    });

    const payload = { action: 'started', url: 'http://localhost:45818' };
    act(() => {
      capturedHandler?.({ payload });
    });

    expect(result.current.lastEvent).toEqual({
      channel: 'gateway-changed',
      payload,
    });
  });

  it('subscribeAll registers all channels', () => {
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() => useDomainEvents());

    act(() => {
      result.current.subscribeAll(vi.fn());
    });

    // Should have registered listeners for all 10 channels
    expect(mockListen).toHaveBeenCalledTimes(10);
    const channels = mockListen.mock.calls.map((call) => call[0]);
    expect(channels).toContain('space-changed');
    expect(channels).toContain('server-changed');
    expect(channels).toContain('server-status-changed');
    expect(channels).toContain('gateway-changed');
    expect(channels).toContain('mcp-notification');
  });

  it('subscribeAll cleanup removes all listeners', async () => {
    const unlistenFns = Array.from({ length: 10 }, () => vi.fn());
    let callIdx = 0;
    mockListen.mockImplementation(async () => {
      return unlistenFns[callIdx++] ?? vi.fn();
    });

    const { result } = renderHook(() => useDomainEvents());

    let cleanup: (() => void) | undefined;
    act(() => {
      cleanup = result.current.subscribeAll(vi.fn());
    });

    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });

    act(() => {
      cleanup?.();
    });

    for (const fn of unlistenFns) {
      expect(fn).toHaveBeenCalled();
    }
  });

  it('subscribeMany only registers specified channels', () => {
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() => useDomainEvents());

    act(() => {
      result.current.subscribeMany(
        ['client-changed', 'grants-changed'],
        vi.fn()
      );
    });

    expect(mockListen).toHaveBeenCalledTimes(2);
    const channels = mockListen.mock.calls.map((call) => call[0]);
    expect(channels).toEqual(['client-changed', 'grants-changed']);
  });

  it('unmount cleans up all active listeners', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);

    const { result, unmount } = renderHook(() => useDomainEvents());

    act(() => {
      result.current.subscribe('space-changed', vi.fn());
    });

    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });

    unmount();

    expect(unlisten).toHaveBeenCalled();
  });
});
