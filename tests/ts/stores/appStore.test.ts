import { describe, it, expect, beforeEach } from 'vitest';
import { useAppStore } from '../../../apps/desktop/src/stores/appStore';
import { createTestSpace, createDefaultSpace, createTestSpaces, Space } from '../fixtures';

describe('appStore', () => {
  beforeEach(() => {
    // Reset store to initial state before each test
    useAppStore.setState({
      spaces: [],
      activeSpaceId: null,
      viewSpaceId: null,
      sidebarCollapsed: false,
      theme: 'system',
      loading: { spaces: false, servers: false },
    });
  });

  describe('setSpaces', () => {
    it('should set spaces array', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);

      expect(useAppStore.getState().spaces).toEqual(spaces);
    });

    it('should auto-select first space as active when none selected', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);

      expect(useAppStore.getState().activeSpaceId).toBe(spaces[0].id);
    });

    it('should prefer default space when auto-selecting', () => {
      const spaces = [
        createTestSpace({ name: 'Space 1', is_default: false }),
        createDefaultSpace({ name: 'Default' }),
        createTestSpace({ name: 'Space 3', is_default: false }),
      ];
      useAppStore.getState().setSpaces(spaces);

      expect(useAppStore.getState().activeSpaceId).toBe(spaces[1].id);
    });

    it('should set viewSpaceId to activeSpaceId when not set', () => {
      const spaces = createTestSpaces(2);
      useAppStore.getState().setSpaces(spaces);

      expect(useAppStore.getState().viewSpaceId).toBe(useAppStore.getState().activeSpaceId);
    });

    it('should reset viewSpaceId if current view space no longer exists', () => {
      const spaces = createTestSpaces(2);
      useAppStore.setState({ viewSpaceId: 'non-existent-id' });
      useAppStore.getState().setSpaces(spaces);

      expect(useAppStore.getState().viewSpaceId).toBe(useAppStore.getState().activeSpaceId);
    });
  });

  describe('setActiveSpace', () => {
    it('should set active space id', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      useAppStore.getState().setActiveSpace(spaces[2].id);

      expect(useAppStore.getState().activeSpaceId).toBe(spaces[2].id);
    });

    it('should follow with viewSpaceId when they were the same', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      // viewSpaceId should now equal activeSpaceId (both spaces[0].id)

      useAppStore.getState().setActiveSpace(spaces[1].id);

      expect(useAppStore.getState().viewSpaceId).toBe(spaces[1].id);
    });

    it('should not change viewSpaceId when different from activeSpaceId', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      useAppStore.getState().setViewSpace(spaces[2].id);

      useAppStore.getState().setActiveSpace(spaces[1].id);

      expect(useAppStore.getState().viewSpaceId).toBe(spaces[2].id);
    });
  });

  describe('setViewSpace', () => {
    it('should set view space id', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      useAppStore.getState().setViewSpace(spaces[1].id);

      expect(useAppStore.getState().viewSpaceId).toBe(spaces[1].id);
    });
  });

  describe('addSpace', () => {
    it('should add a space to the array', () => {
      const space = createTestSpace();
      useAppStore.getState().addSpace(space);

      expect(useAppStore.getState().spaces).toContainEqual(space);
    });

    it('should set active space when first space is added', () => {
      const space = createTestSpace();
      useAppStore.getState().addSpace(space);

      expect(useAppStore.getState().activeSpaceId).toBe(space.id);
    });

    it('should set active space when default space is added', () => {
      const existing = createTestSpace();
      const defaultSpace = createDefaultSpace();

      useAppStore.getState().addSpace(existing);
      useAppStore.getState().addSpace(defaultSpace);

      expect(useAppStore.getState().activeSpaceId).toBe(defaultSpace.id);
    });

    it('should not change active space when non-default space is added', () => {
      const first = createTestSpace({ name: 'First' });
      const second = createTestSpace({ name: 'Second' });

      useAppStore.getState().addSpace(first);
      useAppStore.getState().addSpace(second);

      expect(useAppStore.getState().activeSpaceId).toBe(first.id);
    });

    it('should initialize viewSpaceId when first space is added', () => {
      const space = createTestSpace();
      useAppStore.getState().addSpace(space);

      expect(useAppStore.getState().viewSpaceId).toBe(space.id);
    });
  });

  describe('removeSpace', () => {
    it('should remove a space from the array', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      useAppStore.getState().removeSpace(spaces[1].id);

      expect(useAppStore.getState().spaces).toHaveLength(2);
      expect(useAppStore.getState().spaces.find((s) => s.id === spaces[1].id)).toBeUndefined();
    });

    it('should select first remaining space when active space is removed', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      useAppStore.getState().removeSpace(spaces[0].id);

      expect(useAppStore.getState().activeSpaceId).toBe(spaces[1].id);
    });

    it('should set activeSpaceId to null when last space is removed', () => {
      const space = createTestSpace();
      useAppStore.getState().addSpace(space);
      useAppStore.getState().removeSpace(space.id);

      expect(useAppStore.getState().activeSpaceId).toBeNull();
    });

    it('should update viewSpaceId when viewed space is removed', () => {
      const spaces = createTestSpaces(3);
      useAppStore.getState().setSpaces(spaces);
      useAppStore.getState().setViewSpace(spaces[1].id);
      useAppStore.getState().removeSpace(spaces[1].id);

      expect(useAppStore.getState().viewSpaceId).toBe(useAppStore.getState().activeSpaceId);
    });
  });

  describe('updateSpace', () => {
    it('should update space properties', () => {
      const space = createTestSpace({ name: 'Original' });
      useAppStore.getState().addSpace(space);
      useAppStore.getState().updateSpace(space.id, { name: 'Updated' });

      const updated = useAppStore.getState().spaces.find((s) => s.id === space.id);
      expect(updated?.name).toBe('Updated');
    });

    it('should preserve other space properties', () => {
      const space = createTestSpace({ name: 'Original', description: 'Test desc' });
      useAppStore.getState().addSpace(space);
      useAppStore.getState().updateSpace(space.id, { name: 'Updated' });

      const updated = useAppStore.getState().spaces.find((s) => s.id === space.id);
      expect(updated?.description).toBe('Test desc');
    });

    it('should do nothing if space not found', () => {
      const space = createTestSpace();
      useAppStore.getState().addSpace(space);
      useAppStore.getState().updateSpace('non-existent', { name: 'Updated' });

      expect(useAppStore.getState().spaces[0].name).toBe(space.name);
    });
  });

  describe('toggleSidebar', () => {
    it('should toggle sidebar collapsed state', () => {
      expect(useAppStore.getState().sidebarCollapsed).toBe(false);

      useAppStore.getState().toggleSidebar();
      expect(useAppStore.getState().sidebarCollapsed).toBe(true);

      useAppStore.getState().toggleSidebar();
      expect(useAppStore.getState().sidebarCollapsed).toBe(false);
    });
  });

  describe('setTheme', () => {
    it('should set theme to light', () => {
      useAppStore.getState().setTheme('light');
      expect(useAppStore.getState().theme).toBe('light');
    });

    it('should set theme to dark', () => {
      useAppStore.getState().setTheme('dark');
      expect(useAppStore.getState().theme).toBe('dark');
    });

    it('should set theme to system', () => {
      useAppStore.getState().setTheme('light');
      useAppStore.getState().setTheme('system');
      expect(useAppStore.getState().theme).toBe('system');
    });
  });

  describe('setLoading', () => {
    it('should set spaces loading state', () => {
      useAppStore.getState().setLoading('spaces', true);
      expect(useAppStore.getState().loading.spaces).toBe(true);

      useAppStore.getState().setLoading('spaces', false);
      expect(useAppStore.getState().loading.spaces).toBe(false);
    });

    it('should set servers loading state', () => {
      useAppStore.getState().setLoading('servers', true);
      expect(useAppStore.getState().loading.servers).toBe(true);
    });

    it('should not affect other loading states', () => {
      useAppStore.getState().setLoading('spaces', true);
      useAppStore.getState().setLoading('servers', true);
      useAppStore.getState().setLoading('spaces', false);

      expect(useAppStore.getState().loading.servers).toBe(true);
    });
  });
});
