import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import { immer } from 'zustand/middleware/immer';
import { AppStore, AppState } from './types';

const initialState: AppState = {
  spaces: [],
  viewSpaceId: null,
  activeNav: 'home',
  pendingClientId: null,
  pendingSettingsSection: null,
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
          // Validate persisted viewSpaceId still exists; reset to default if not
          const viewExists = state.viewSpaceId
            ? spaces.some((s) => s.id === state.viewSpaceId)
            : false;
          if (!viewExists && spaces.length > 0) {
            const defaultSpace = spaces.find((s) => s.is_default);
            state.viewSpaceId = defaultSpace?.id ?? spaces[0].id;
          }
        }),

      setViewSpace: (id) =>
        set((state) => {
          state.viewSpaceId = id;
        }),

      addSpace: (space) =>
        set((state) => {
          state.spaces.push(space);
          if (!state.viewSpaceId || space.is_default) {
            state.viewSpaceId = space.id;
          }
        }),

      removeSpace: (id) =>
        set((state) => {
          state.spaces = state.spaces.filter((s) => s.id !== id);
          if (state.viewSpaceId === id) {
            const fallback =
              state.spaces.find((s) => s.is_default) ?? state.spaces[0];
            state.viewSpaceId = fallback?.id ?? null;
          }
        }),

      updateSpace: (id, updates) =>
        set((state) => {
          const index = state.spaces.findIndex((s) => s.id === id);
          if (index !== -1) {
            state.spaces[index] = { ...state.spaces[index], ...updates };
          }
        }),

      // Navigation
      navigateTo: (nav) =>
        set((state) => {
          state.activeNav = nav;
        }),

      setPendingClientId: (id) =>
        set((state) => {
          state.pendingClientId = id;
        }),

      setPendingSettingsSection: (section) =>
        set((state) => {
          state.pendingSettingsSection = section;
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
        viewSpaceId: state.viewSpaceId,
        sidebarCollapsed: state.sidebarCollapsed,
        theme: state.theme,
        analyticsEnabled: state.analyticsEnabled,
      }),
    }
  )
);
