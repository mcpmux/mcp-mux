import { Space } from '@/lib/api/spaces';

export type NavItem =
  | 'home'
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

  // Navigation
  activeNav: NavItem;
  /** Client ID to auto-select when navigating to Clients page */
  pendingClientId: string | null;
  /** Section to scroll to + flash when navigating to Settings (e.g. 'security'). */
  pendingSettingsSection: string | null;
  /** When true, the Workspaces page opens the New-mapping walkthrough on arrival. */
  pendingWorkspaceNew: boolean;
  /**
   * When true, the FeatureSets page opens its Create dialog on arrival. Set by
   * the "Create a new feature set" shortcut on the Mapping surfaces so a user
   * who only has the auto-seeded Starter set can jump straight into making a
   * new one for the Space they're mapping.
   */
  pendingFeatureSetNew: boolean;
  /**
   * A `workspace_root` (folder path or client-id key) to auto-select in the
   * Workspaces Mapping inspector on arrival. Set when another surface (e.g. the
   * Clients page "Open this client's mapping" link) wants to deep-link to a
   * specific binding rather than the generic Mapping tab.
   */
  pendingWorkspaceRoot: string | null;

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

  // Navigation
  navigateTo: (nav: NavItem) => void;
  setPendingClientId: (id: string | null) => void;
  setPendingSettingsSection: (section: string | null) => void;
  setPendingWorkspaceNew: (v: boolean) => void;
  setPendingWorkspaceRoot: (root: string | null) => void;
  setPendingFeatureSetNew: (v: boolean) => void;

  // UI
  toggleSidebar: () => void;
  setTheme: (theme: 'light' | 'dark' | 'system') => void;
  setAnalyticsEnabled: (enabled: boolean) => void;

  // Loading
  setLoading: (key: keyof AppState['loading'], value: boolean) => void;
}

export type AppStore = AppState & AppActions;
