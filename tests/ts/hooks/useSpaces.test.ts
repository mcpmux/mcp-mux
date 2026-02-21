import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { useSpaces } from '@/hooks/useSpaces';
import { createTestSpace, createDefaultSpace } from '../fixtures';

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  vi.clearAllMocks();
});

describe('useSpaces', () => {
  it('load fetches spaces and active space', async () => {
    const space1 = createDefaultSpace({ id: 'sp-1' });
    const space2 = createTestSpace({ id: 'sp-2' });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_spaces') return [space1, space2];
      if (cmd === 'get_active_space') return space1;
      return null;
    });

    const { result } = renderHook(() => useSpaces());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.spaces).toHaveLength(2);
    expect(result.current.activeSpace).toEqual(space1);
  });

  it('loading is true during fetch, false after', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_spaces') return [];
      if (cmd === 'get_active_space') return null;
      return null;
    });

    const { result } = renderHook(() => useSpaces());

    // Initially loading
    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
  });

  it('sets error on fetch failure', async () => {
    mockInvoke.mockRejectedValue(new Error('Database connection failed'));

    const { result } = renderHook(() => useSpaces());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.error).toBe('Database connection failed');
  });

  it('create calls invoke then refreshes', async () => {
    const existing = createDefaultSpace({ id: 'sp-1' });
    const created = createTestSpace({ id: 'sp-2', name: 'New Space' });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_spaces') return [existing, created];
      if (cmd === 'get_active_space') return existing;
      if (cmd === 'create_space') return created;
      return null;
    });

    const { result } = renderHook(() => useSpaces());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.create('New Space', '🆕');
    });

    // create_space should have been called
    expect(mockInvoke).toHaveBeenCalledWith('create_space', {
      name: 'New Space',
      icon: '🆕',
    });

    // And list_spaces should have been called again (refresh)
    const listCalls = mockInvoke.mock.calls.filter(
      (call) => call[0] === 'list_spaces'
    );
    expect(listCalls.length).toBeGreaterThanOrEqual(2);
  });

  it('remove calls invoke then refreshes', async () => {
    const space = createDefaultSpace({ id: 'sp-1' });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_spaces') return [space];
      if (cmd === 'get_active_space') return space;
      if (cmd === 'delete_space') return undefined;
      return null;
    });

    const { result } = renderHook(() => useSpaces());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.remove('sp-1');
    });

    expect(mockInvoke).toHaveBeenCalledWith('delete_space', { id: 'sp-1' });
  });

  it('setActive calls invoke then refreshes', async () => {
    const space = createDefaultSpace({ id: 'sp-1' });

    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_spaces') return [space];
      if (cmd === 'get_active_space') return space;
      if (cmd === 'set_active_space') return undefined;
      return null;
    });

    const { result } = renderHook(() => useSpaces());

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.setActive('sp-1');
    });

    expect(mockInvoke).toHaveBeenCalledWith('set_active_space', { id: 'sp-1' });
  });
});
