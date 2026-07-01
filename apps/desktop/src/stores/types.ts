import { Space } from '@/lib/api/spaces';

export type NavItem =
  | 'dashboard'
  | 'registry'
  | 'servers'
  | 'spaces'
  | 'featuresets'
  | 'workspaces'
  | 'clients'
  | 'builtin-servers'
  | 'settings';

export interface AppState {
  // Spaces
  spaces: Space[];
  /**
   * The space the user is currently viewing in the desktop app. Pure
   * UI navigation state — has no effect on gateway routing, which always
   * resolves via reported workspace root → WorkspaceBinding (or the
   * built-in default Space when no binding matches).
   */
  viewSpaceId: string | null;

  /** Section to scroll to + flash when navigating to Settings (e.g. 'gateway'). */
  pendingSettingsSection: string | null;
  /** When true, the Workspaces page opens the create binding panel on arrival. */
  pendingWorkspaceNew: boolean;

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
  setViewSpace: (id: string | null) => void;
  addSpace: (space: Space) => void;
  removeSpace: (id: string) => void;
  updateSpace: (id: string, updates: Partial<Space>) => void;

  setPendingSettingsSection: (section: string | null) => void;
  setPendingWorkspaceNew: (v: boolean) => void;

  // UI
  toggleSidebar: () => void;
  setTheme: (theme: 'light' | 'dark' | 'system') => void;
  setAnalyticsEnabled: (enabled: boolean) => void;

  // Loading
  setLoading: (key: keyof AppState['loading'], value: boolean) => void;
}

export type AppStore = AppState & AppActions;
