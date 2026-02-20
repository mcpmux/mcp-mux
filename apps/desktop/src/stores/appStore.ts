import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import { immer } from 'zustand/middleware/immer';
import { AppStore, AppState } from './types';

const initialState: AppState = {
  spaces: [],
  activeSpaceId: null,
  viewSpaceId: null,
  sidebarCollapsed: false,
  theme: 'system',
  analyticsEnabled: true,
  loading: {
    spaces: false,
    servers: false,
  },
};

export const useAppStore = create<AppStore>()(
  persist(
    immer((set) => ({
      ...initialState,

      // Spaces
      setSpaces: (spaces) =>
        set((state) => {
          state.spaces = spaces;
          // Validate persisted activeSpaceId still exists, reset to default if not
          const activeExists = state.activeSpaceId
            ? spaces.some((s) => s.id === state.activeSpaceId)
            : false;
          if (!activeExists && spaces.length > 0) {
            const defaultSpace = spaces.find((s) => s.is_default);
            state.activeSpaceId = defaultSpace?.id ?? spaces[0].id;
          }
          const viewExists = state.viewSpaceId
            ? spaces.some((s) => s.id === state.viewSpaceId)
            : false;
          if (!viewExists) {
            state.viewSpaceId = state.activeSpaceId;
          }
        }),

      setActiveSpace: (id) =>
        set((state) => {
          const shouldFollow = !state.viewSpaceId || state.viewSpaceId === state.activeSpaceId;
          state.activeSpaceId = id;
          if (shouldFollow) {
            state.viewSpaceId = id;
          }
        }),

      setViewSpace: (id) =>
        set((state) => {
          state.viewSpaceId = id;
        }),

      addSpace: (space) =>
        set((state) => {
          state.spaces.push(space);
          if (space.is_default || state.spaces.length === 1) {
            state.activeSpaceId = space.id;
          }
          if (!state.viewSpaceId) {
            state.viewSpaceId = state.activeSpaceId;
          }
        }),

      removeSpace: (id) =>
        set((state) => {
          state.spaces = state.spaces.filter((s) => s.id !== id);
          if (state.activeSpaceId === id) {
            state.activeSpaceId = state.spaces[0]?.id ?? null;
          }
          if (state.viewSpaceId === id) {
            state.viewSpaceId = state.activeSpaceId;
          }
        }),

      updateSpace: (id, updates) =>
        set((state) => {
          const index = state.spaces.findIndex((s) => s.id === id);
          if (index !== -1) {
            state.spaces[index] = { ...state.spaces[index], ...updates };
          }
        }),

      // UI
      toggleSidebar: () =>
        set((state) => {
          state.sidebarCollapsed = !state.sidebarCollapsed;
        }),

      setTheme: (theme) =>
        set((state) => {
          state.theme = theme;
        }),

      setAnalyticsEnabled: (enabled) =>
        set((state) => {
          state.analyticsEnabled = enabled;
        }),

      // Loading
      setLoading: (key, value) =>
        set((state) => {
          state.loading[key] = value;
        }),
    })),
    {
      name: 'mcpmux-storage',
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        // Only persist these fields
        // Note: viewSpaceId is NOT persisted - always starts as activeSpaceId on launch
        activeSpaceId: state.activeSpaceId,
        sidebarCollapsed: state.sidebarCollapsed,
        theme: state.theme,
        analyticsEnabled: state.analyticsEnabled,
      }),
    }
  )
);

