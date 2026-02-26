import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useServerManager } from '@/hooks/useServerManager';
import type { ServerStatusResponse } from '@/lib/api/serverManager';

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

beforeEach(() => {
  vi.clearAllMocks();
});

function mockStatuses(
  statuses: Record<string, ServerStatusResponse>
) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'get_server_statuses') return statuses;
    return undefined;
  });
}

function makeStatus(
  serverId: string,
  status: ServerStatusResponse['status'] = 'disconnected',
  flowId = 1
): ServerStatusResponse {
  return {
    server_id: serverId,
    status,
    flow_id: flowId,
    has_connected_before: false,
    message: null,
  };
}

describe('useServerManager', () => {
  it('initial fetch populates statuses', async () => {
    const data = {
      'srv-1': makeStatus('srv-1', 'connected'),
      'srv-2': makeStatus('srv-2', 'disconnected'),
    };
    mockStatuses(data);
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.statuses['srv-1']?.status).toBe('connected');
    expect(result.current.statuses['srv-2']?.status).toBe('disconnected');
  });

  it('loading is true then false', async () => {
    mockStatuses({});
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    // Initially loading
    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
  });

  it('error on invoke rejection', async () => {
    mockInvoke.mockRejectedValue(new Error('Network error'));
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Network error');
  });

  it('empty statuses when no servers', async () => {
    mockStatuses({});
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(Object.keys(result.current.statuses)).toHaveLength(0);
  });

  it('null spaceId skips fetch', async () => {
    mockListen.mockResolvedValue(vi.fn());

    const { result } = renderHook(() =>
      useServerManager({ spaceId: '' })
    );

    // Should not have called invoke since spaceId is empty
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });

    expect(mockInvoke).not.toHaveBeenCalledWith(
      'get_server_statuses',
      expect.anything()
    );
  });

  it('status event updates matching server state', async () => {
    const data = { 'srv-1': makeStatus('srv-1', 'connecting', 1) };
    mockStatuses(data);

    // Capture the status event handler
    let statusHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (channel, handler) => {
      if (channel === 'server-status-changed') {
        statusHandler = handler as (event: unknown) => void;
      }
      return vi.fn();
    });

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    // Simulate a status change event
    act(() => {
      statusHandler?.({
        payload: {
          space_id: 'space-1',
          server_id: 'srv-1',
          status: 'connected',
          flow_id: 2,
          has_connected_before: true,
        },
      });
    });

    expect(result.current.statuses['srv-1']?.status).toBe('connected');
  });

  it('wrong space event is ignored', async () => {
    const data = { 'srv-1': makeStatus('srv-1', 'connecting', 1) };
    mockStatuses(data);

    let statusHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (channel, handler) => {
      if (channel === 'server-status-changed') {
        statusHandler = handler as (event: unknown) => void;
      }
      return vi.fn();
    });

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    // Event from different space
    act(() => {
      statusHandler?.({
        payload: {
          space_id: 'space-OTHER',
          server_id: 'srv-1',
          status: 'error',
          flow_id: 99,
          has_connected_before: false,
        },
      });
    });

    // Status unchanged — still from the initial fetch
    expect(result.current.statuses['srv-1']?.status).toBe('connecting');
  });

  it('older flow_id event is ignored', async () => {
    const data = { 'srv-1': makeStatus('srv-1', 'connected', 5) };
    mockStatuses(data);

    let statusHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (channel, handler) => {
      if (channel === 'server-status-changed') {
        statusHandler = handler as (event: unknown) => void;
      }
      return vi.fn();
    });

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    // Event with older flow_id
    act(() => {
      statusHandler?.({
        payload: {
          space_id: 'space-1',
          server_id: 'srv-1',
          status: 'error',
          flow_id: 3,
          has_connected_before: false,
        },
      });
    });

    // Should remain 'connected' from the initial fetch
    expect(result.current.statuses['srv-1']?.status).toBe('connected');
  });

  it('newer flow_id event is accepted', async () => {
    const data = { 'srv-1': makeStatus('srv-1', 'connecting', 1) };
    mockStatuses(data);

    let statusHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (channel, handler) => {
      if (channel === 'server-status-changed') {
        statusHandler = handler as (event: unknown) => void;
      }
      return vi.fn();
    });

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    act(() => {
      statusHandler?.({
        payload: {
          space_id: 'space-1',
          server_id: 'srv-1',
          status: 'connected',
          flow_id: 10,
          has_connected_before: true,
        },
      });
    });

    expect(result.current.statuses['srv-1']?.status).toBe('connected');
    expect(result.current.statuses['srv-1']?.flow_id).toBe(10);
  });

  it('auth progress event sets authProgress state', async () => {
    mockStatuses({});

    let authHandler: ((event: unknown) => void) | undefined;
    mockListen.mockImplementation(async (channel, handler) => {
      if (channel === 'server-auth-progress') {
        authHandler = handler as (event: unknown) => void;
      }
      return vi.fn();
    });

    const { result } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    act(() => {
      authHandler?.({
        payload: {
          space_id: 'space-1',
          server_id: 'srv-1',
          remaining_seconds: 42,
          flow_id: 1,
        },
      });
    });

    expect(result.current.authProgress['srv-1']).toBe(42);
  });

  it('unmount calls unlisten functions', async () => {
    mockStatuses({});

    const unlistenFns = [vi.fn(), vi.fn(), vi.fn()];
    let idx = 0;
    mockListen.mockImplementation(async () => {
      return unlistenFns[idx++] ?? vi.fn();
    });

    const { unmount } = renderHook(() =>
      useServerManager({ spaceId: 'space-1' })
    );

    // Wait for listeners to be set up
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });

    unmount();

    // All 3 event listeners (status, auth, features) should be unsubscribed
    for (const fn of unlistenFns) {
      expect(fn).toHaveBeenCalled();
    }
  });
});
