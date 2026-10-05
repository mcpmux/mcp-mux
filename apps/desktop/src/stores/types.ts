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

/** A mapping the Mapping tab should open on arrival (e.g. from a client's panel). */
export interface PendingMapping {
  /** The mapping key — a client id / label for `id`, a folder for `path`. */
  key: string;
  bindingType: 'path' | 'id';
}

/** Another tab sent the user to FeatureSets to create one. */
export interface PendingFeatureSetCreate {
  /** Tab to offer a way back to once the feature set exists. */
  returnTo: NavItem;
  /** Space the user was viewing before the jump — restored on the way back. */
  restoreSpaceId?: string | null;
}

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
  /** Mapping to open (or create, if missing) when the Mapping tab mounts. */
  pendingMapping: PendingMapping | null;
  /** Set while the user is on FeatureSets because another tab sent them to create one. */
  pendingFeatureSetCreate: PendingFeatureSetCreate | null;

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
  setPendingMapping: (m: PendingMapping | null) => void;
  setPendingFeatureSetCreate: (p: PendingFeatureSetCreate | null) => void;

  // UI
  toggleSidebar: () => void;
  setTheme: (theme: 'light' | 'dark' | 'system') => void;
  setAnalyticsEnabled: (enabled: boolean) => void;

  // Loading
  setLoading: (key: keyof AppState['loading'], value: boolean) => void;
}

export type AppStore = AppState & AppActions;
