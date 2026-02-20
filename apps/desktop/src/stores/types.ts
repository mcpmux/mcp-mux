import { Space } from '@/lib/api/spaces';

export interface AppState {
  // Spaces
  spaces: Space[];
  activeSpaceId: string | null;
  viewSpaceId: string | null;

  // UI state
  sidebarCollapsed: boolean;
  theme: 'light' | 'dark' | 'system';
  analyticsEnabled: boolean;

  // Loading states
  loading: {
    spaces: boolean;
    servers: boolean;
  };
}

export interface AppActions {
  // Spaces
  setSpaces: (spaces: Space[]) => void;
  setActiveSpace: (id: string | null) => void;
  setViewSpace: (id: string | null) => void;
  addSpace: (space: Space) => void;
  removeSpace: (id: string) => void;
  updateSpace: (id: string, updates: Partial<Space>) => void;

  // UI
  toggleSidebar: () => void;
  setTheme: (theme: 'light' | 'dark' | 'system') => void;
  setAnalyticsEnabled: (enabled: boolean) => void;

  // Loading
  setLoading: (key: keyof AppState['loading'], value: boolean) => void;
}

export type AppStore = AppState & AppActions;

