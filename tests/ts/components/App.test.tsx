import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useAppStore } from '@/stores';
import { renderWithI18n } from '../render-with-i18n.helpers';

// ---------- Hoisted mock functions (available before vi.mock factories run) ----------

const { mockInvoke, mockCheck, mockGetGatewayStatus, mockCheckForUpdate, mockGetAutoInstallUpdates } =
  vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockCheck: vi.fn(),
  mockGetGatewayStatus: vi.fn(),
  mockCheckForUpdate: vi.fn(),
  mockGetAutoInstallUpdates: vi.fn().mockResolvedValue(false),
}));

// ---------- Module mocks ----------

// Override Tauri core mock from setup.ts with our local reference
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
}));

vi.mock('@/lib/updates', () => ({
  checkForUpdate: mockCheckForUpdate,
  getAutoInstallUpdates: mockGetAutoInstallUpdates,
}));

// Mock page components as lightweight stubs
vi.mock('@/features/registry', () => ({
  RegistryPage: () => <div data-testid="registry-page" />,
}));
vi.mock('@/features/featuresets', () => ({
  FeatureSetsPage: () => <div data-testid="featuresets-page" />,
}));
vi.mock('@/features/clients', () => ({
  ClientsPage: () => <div data-testid="clients-page" />,
}));
vi.mock('@/features/servers', () => ({
  ServersPage: () => <div data-testid="servers-page" />,
}));
vi.mock('@/features/spaces', () => ({
  SpacesPage: () => <div data-testid="spaces-page" />,
}));
vi.mock('@/features/settings', () => ({
  SettingsPage: () => <div data-testid="settings-page" />,
}));
vi.mock('@/features/workspaces', () => ({
  WorkspacesPage: () => <div data-testid="workspaces-page" />,
  WorkspaceBindingSheet: () => null,
}));

// Mock non-essential components
vi.mock('@/components/OAuthConsentModal', () => ({
  OAuthConsentModal: () => null,
}));
vi.mock('@/components/ServerInstallModal', () => ({
  ServerInstallModal: () => null,
}));
vi.mock('@/features/metaTools', () => ({
  MetaToolApprovalDialog: () => null,
}));
vi.mock('@/components/SpaceSwitcher', () => ({
  SpaceSwitcher: () => <div data-testid="space-switcher" />,
}));
vi.mock('@/components/ThemeProvider', () => ({
  ThemeProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

// Mock hooks
vi.mock('@/hooks/useDataSync', () => ({
  useDataSync: vi.fn(),
}));

vi.mock('@/hooks/useMetaToolEvents', () => ({
  useMetaToolEventListener: vi.fn(),
  useMetaToolEvents: vi.fn(() => ({ subscribe: vi.fn(() => vi.fn()) })),
}));

type GatewayPayload = { action: string; url?: string; port?: number };
let gatewayEventCallbacks: ((payload: GatewayPayload) => void)[] = [];

vi.mock('@/hooks/useDomainEvents', () => ({
  useGatewayEvents: vi.fn((cb: (payload: GatewayPayload) => void) => {
    gatewayEventCallbacks.push(cb);
  }),
  useServerStatusEvents: vi.fn(),
  useDomainEvents: vi.fn(() => ({ subscribe: vi.fn(() => vi.fn()) })),
}));

vi.mock('@/lib/analytics', () => ({
  initAnalytics: vi.fn(),
  capture: vi.fn(),
  optOut: vi.fn(),
  optIn: vi.fn(),
  hasOptedOut: vi.fn(() => false),
}));

function fireGatewayEvent(payload: GatewayPayload) {
  gatewayEventCallbacks.forEach((cb) => cb(payload));
}

// Mock API modules (used via dynamic import in DashboardView and AppContent)
vi.mock('@/lib/api/gateway', () => ({
  getGatewayStatus: mockGetGatewayStatus,
  startGateway: vi.fn().mockResolvedValue('http://localhost:45818'),
  stopGateway: vi.fn().mockResolvedValue(undefined),
  restartGateway: vi.fn().mockResolvedValue(undefined),
  // AutoStartConflictResolver polls these on mount; default to a
  // "no conflict" state so the resolver no-ops in tests.
  takePendingPortConflict: vi.fn().mockResolvedValue(null),
  probeGatewayStart: vi.fn().mockResolvedValue({
    preferred_port: 45818,
    preferred_available: true,
    source: 'Default',
  }),
  getGatewayPortSettings: vi.fn().mockResolvedValue({
    configured_port: null,
    default_port: 45818,
    active_port: null,
  }),
  openUrl: vi.fn().mockResolvedValue(undefined),
  parsePortInUseError: vi.fn().mockReturnValue(null),
}));
vi.mock('@/lib/api/clients', () => ({
  listClients: vi.fn().mockResolvedValue([]),
}));
vi.mock('@/lib/api/featureSets', () => ({
  listFeatureSets: vi.fn().mockResolvedValue([]),
  listFeatureSetsBySpace: vi.fn().mockResolvedValue([]),
}));
vi.mock('@/lib/api/registry', () => ({
  listInstalledServers: vi.fn().mockResolvedValue([]),
}));
vi.mock('@/lib/api/workspaceBindings', () => ({
  listWorkspaceBindings: vi.fn().mockResolvedValue([]),
}));
vi.mock('@/lib/api/serverManager', () => ({
  getServerStatuses: vi.fn().mockResolvedValue({}),
}));
vi.mock('@/lib/api/spaces', () => ({
  listSpaces: vi.fn().mockResolvedValue([]),
}));

// Mock window API for WindowButton
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({
    minimize: vi.fn(),
    maximize: vi.fn(),
    close: vi.fn(),
  })),
}));

// ---------- Import after mocks ----------
import App from '@/App';

// ---------- Helpers ----------

function setupInvoke(responses: Record<string, unknown>) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd in responses) {
      const val = responses[cmd];
      if (val instanceof Error) return Promise.reject(val);
      return Promise.resolve(val);
    }
    return Promise.resolve(undefined);
  });
}

function setupGateway(status: { running: boolean; url: string | null }) {
  mockGetGatewayStatus.mockResolvedValue({
    running: status.running,
    url: status.url,
    active_sessions: 0,
    connected_backends: 0,
  });
}

// ---------- Tests ----------

describe('App – dynamic version display', () => {
  beforeEach(() => {
    gatewayEventCallbacks = [];
    setupGateway({ running: false, url: null });
  });

  it('should display version from get_version command', async () => {
    setupInvoke({ get_version: '1.2.3' });

    renderWithI18n(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-version')).toHaveTextContent('v1.2.3');
    });
  });

  it('should hide the version span while loading', () => {
    // invoke never resolves
    mockInvoke.mockImplementation(() => new Promise(() => {}));

    renderWithI18n(<App />);

    // The span only renders once `appVersion` has a value.
    expect(screen.queryByTestId('statusbar-version')).toBeNull();
  });

  it('should not crash and should omit the version when fetch fails', async () => {
    setupInvoke({ get_version: new Error('command failed') });

    renderWithI18n(<App />);

    // App still renders (sidebar is present) even if version lookup errored.
    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('statusbar-version')).toBeNull();
  });
});

describe('App – dynamic gateway URL display', () => {
  beforeEach(() => {
    gatewayEventCallbacks = [];
    setupInvoke({ get_version: '0.1.2' });
  });

  it('should show "Gateway stopped" as default gateway state', async () => {
    setupGateway({ running: false, url: null });

    renderWithI18n(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).toHaveTextContent(
        'Gateway stopped'
      );
    });
  });

  it('should show "Gateway stopped" when gateway is running but url is null', async () => {
    setupGateway({ running: true, url: null });

    renderWithI18n(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).toHaveTextContent(
        'Gateway stopped'
      );
    });
  });

  it('should flip to running state when gateway-started event fires', async () => {
    setupGateway({ running: false, url: null });

    renderWithI18n(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).toHaveTextContent(
        'Gateway stopped'
      );
    });

    // Simulate gateway started event with a port.
    act(() => {
      fireGatewayEvent({ action: 'started', url: 'http://localhost:9999', port: 9999 });
    });

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).toHaveTextContent('Gateway');
      expect(screen.getByTestId('statusbar-gateway')).not.toHaveTextContent(
        'stopped'
      );
    });
  });

  it('should flip back to stopped when gateway-stopped event fires', async () => {
    setupGateway({ running: false, url: null });

    renderWithI18n(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).toHaveTextContent(
        'Gateway stopped'
      );
    });

    act(() => {
      fireGatewayEvent({
        action: 'started',
        url: 'http://localhost:45818',
        port: 45818,
      });
    });

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).not.toHaveTextContent(
        'stopped'
      );
    });

    act(() => {
      fireGatewayEvent({ action: 'stopped' });
    });

    await waitFor(() => {
      expect(screen.getByTestId('statusbar-gateway')).toHaveTextContent(
        'Gateway stopped'
      );
    });
  });
});

describe('App – update banner', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    gatewayEventCallbacks = [];
    setupInvoke({ get_version: '0.1.2' });
    mockGetAutoInstallUpdates.mockResolvedValue(false);
    setupGateway({ running: false, url: null });
    mockCheckForUpdate.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should show update banner when update is available', async () => {
    mockCheckForUpdate.mockResolvedValue({
      version: '2.0.0',
      downloadAndInstall: vi.fn(),
    });

    renderWithI18n(<App />);

    // Banner should not be visible before the 5s delay
    expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();

    // Trigger the 5s delay and flush the async update check
    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
      await Promise.resolve();
    });
    vi.useRealTimers();

    await waitFor(() => {
      expect(mockCheckForUpdate).toHaveBeenCalled();
      const banner = screen.getByTestId('update-banner');
      expect(banner).toBeInTheDocument();
      expect(banner).toHaveTextContent('v2.0.0');
      expect(banner).toHaveTextContent('is available');
    });
  });

  it('should not show banner when no update is available', async () => {
    mockCheckForUpdate.mockResolvedValue(null);

    renderWithI18n(<App />);

    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
      await Promise.resolve();
    });
    vi.useRealTimers();

    // Give the async check time to resolve and confirm no banner appears
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });
    expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
  });

  it('should not show banner when update check fails', async () => {
    mockCheckForUpdate.mockRejectedValue(new Error('network error'));

    renderWithI18n(<App />);

    vi.advanceTimersByTime(5000);
    vi.useRealTimers();

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });
    expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
  });

  it('should dismiss banner when X button is clicked', async () => {
    const user = userEvent.setup();

    mockCheckForUpdate.mockResolvedValue({
      version: '2.0.0',
      downloadAndInstall: vi.fn(),
    });

    renderWithI18n(<App />);

    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
      await Promise.resolve();
    });
    vi.useRealTimers();

    await waitFor(() => {
      expect(screen.getByTestId('update-banner')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('dismiss-update-banner'));

    await waitFor(() => {
      expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
    });
  });

  it('should navigate to Settings and hide banner when "Update now" is clicked', async () => {
    const user = userEvent.setup();

    mockCheckForUpdate.mockResolvedValue({
      version: '2.0.0',
      downloadAndInstall: vi.fn(),
    });

    renderWithI18n(<App />);

    await act(async () => {
      vi.advanceTimersByTime(5000);
      await Promise.resolve();
      await Promise.resolve();
    });
    vi.useRealTimers();

    await waitFor(() => {
      expect(screen.getByTestId('update-banner')).toBeInTheDocument();
    });

    await user.click(screen.getByText('Update now'));

    await waitFor(() => {
      // Banner should be gone
      expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
      // Settings page should be rendered
      expect(screen.getByTestId('settings-page')).toBeInTheDocument();
    });
    // ...and it targets the Updates section so SettingsPage scrolls/flashes
    // there rather than dumping the user at the top of the page.
    expect(useAppStore.getState().pendingSettingsSection).toBe('updates');
  });
});
