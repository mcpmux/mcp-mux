import { describe, it, expect, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useAppStore } from '@/stores/appStore';
import { useActiveSpace, useViewSpace, useIsLoading } from '@/stores/selectors';
import { createTestSpace, createDefaultSpace } from '../fixtures';

beforeEach(() => {
  // Reset store to initial state before each test
  useAppStore.setState({
    spaces: [],
    activeSpaceId: null,
    viewSpaceId: null,
    loading: { spaces: false, servers: false },
  });
});

describe('useActiveSpace', () => {
  it('returns matching space when activeSpaceId is set', () => {
    const space = createDefaultSpace({ id: 'space-1' });
    useAppStore.setState({ spaces: [space], activeSpaceId: 'space-1' });

    const { result } = renderHook(() => useActiveSpace());
    expect(result.current).toEqual(space);
  });

  it('returns null when no activeSpaceId', () => {
    useAppStore.setState({ spaces: [createTestSpace()], activeSpaceId: null });

    const { result } = renderHook(() => useActiveSpace());
    expect(result.current).toBeNull();
  });

  it('returns null when activeSpaceId not in list', () => {
    useAppStore.setState({
      spaces: [createTestSpace({ id: 'space-a' })],
      activeSpaceId: 'space-missing',
    });

    const { result } = renderHook(() => useActiveSpace());
    expect(result.current).toBeNull();
  });
});

describe('useViewSpace', () => {
  it('uses viewSpaceId when it differs from activeSpaceId', () => {
    const space1 = createTestSpace({ id: 'space-1', name: 'Space 1' });
    const space2 = createTestSpace({ id: 'space-2', name: 'Space 2' });
    useAppStore.setState({
      spaces: [space1, space2],
      activeSpaceId: 'space-1',
      viewSpaceId: 'space-2',
    });

    const { result } = renderHook(() => useViewSpace());
    expect(result.current).toEqual(space2);
  });

  it('falls back to activeSpaceId when viewSpaceId is null', () => {
    const space = createDefaultSpace({ id: 'space-1' });
    useAppStore.setState({
      spaces: [space],
      activeSpaceId: 'space-1',
      viewSpaceId: null,
    });

    const { result } = renderHook(() => useViewSpace());
    expect(result.current).toEqual(space);
  });

  it('returns null when both viewSpaceId and activeSpaceId are null', () => {
    useAppStore.setState({
      spaces: [createTestSpace()],
      activeSpaceId: null,
      viewSpaceId: null,
    });

    const { result } = renderHook(() => useViewSpace());
    expect(result.current).toBeNull();
  });
});

describe('useIsLoading', () => {
  it('initially returns false for spaces', () => {
    const { result } = renderHook(() => useIsLoading('spaces'));
    expect(result.current).toBe(false);
  });

  it('reflects setLoading for spaces', () => {
    act(() => {
      useAppStore.getState().setLoading('spaces', true);
    });

    const { result } = renderHook(() => useIsLoading('spaces'));
    expect(result.current).toBe(true);
  });
});
