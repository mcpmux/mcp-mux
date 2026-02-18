import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

// ---------- Hoisted mock functions (available before vi.mock factories run) ----------

const { mockInvoke, mockCheck, mockGetGatewayStatus } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockCheck: vi.fn(),
  mockGetGatewayStatus: vi.fn(),
}));

// ---------- Module mocks ----------

// Override Tauri core mock from setup.ts with our local reference
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
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

// Mock non-essential components
vi.mock('@/components/OAuthConsentModal', () => ({
  OAuthConsentModal: () => null,
}));
vi.mock('@/components/ServerInstallModal', () => ({
  ServerInstallModal: () => null,
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

type GatewayPayload = { action: string; url?: string; port?: number };
let gatewayEventCallbacks: ((payload: GatewayPayload) => void)[] = [];

vi.mock('@/hooks/useDomainEvents', () => ({
  useGatewayEvents: vi.fn((cb: (payload: GatewayPayload) => void) => {
    gatewayEventCallbacks.push(cb);
  }),
  useServerStatusEvents: vi.fn(),
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

    render(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent('McpMux v1.2.3');
    });
  });

  it('should display "McpMux" without version suffix while loading', () => {
    // invoke never resolves
    mockInvoke.mockImplementation(() => new Promise(() => {}));

    render(<App />);

    const sidebar = screen.getByTestId('sidebar');
    expect(sidebar).toHaveTextContent('McpMux');
    expect(sidebar).not.toHaveTextContent('McpMux v');
  });

  it('should display "McpMux" without crashing when version fetch fails', async () => {
    setupInvoke({ get_version: new Error('command failed') });

    render(<App />);

    // Wait for the rejected promise to be handled
    await waitFor(() => {
      const sidebar = screen.getByTestId('sidebar');
      expect(sidebar).toHaveTextContent('McpMux');
    });

    // Should not show a version number
    expect(screen.getByTestId('sidebar')).not.toHaveTextContent('McpMux v');
  });
});

describe('App – dynamic gateway URL display', () => {
  beforeEach(() => {
    gatewayEventCallbacks = [];
    setupInvoke({ get_version: '0.1.2' });
  });

  it('should show "Not running" as default gateway state', async () => {
    setupGateway({ running: false, url: null });

    render(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent('Gateway: Not running');
    });
  });

  it('should show "Not running" when gateway is running but url is null', async () => {
    setupGateway({ running: true, url: null });

    render(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent('Gateway: Not running');
    });
  });

  it('should update URL when gateway-started event fires', async () => {
    setupGateway({ running: false, url: null });

    render(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent('Gateway: Not running');
    });

    // Simulate gateway started event
    act(() => {
      fireGatewayEvent({ action: 'started', url: 'http://localhost:9999' });
    });

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent(
        'Gateway: http://localhost:9999'
      );
    });
  });

  it('should show "Not running" when gateway-stopped event fires', async () => {
    setupGateway({ running: false, url: null });

    render(<App />);

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent('Gateway: Not running');
    });

    // Start the gateway via event, then stop it
    act(() => {
      fireGatewayEvent({ action: 'started', url: 'http://localhost:45818' });
    });

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent(
        'Gateway: http://localhost:45818'
      );
    });

    // Simulate gateway stopped event
    act(() => {
      fireGatewayEvent({ action: 'stopped' });
    });

    await waitFor(() => {
      expect(screen.getByTestId('sidebar')).toHaveTextContent('Gateway: Not running');
    });
  });
});

describe('App – update banner', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    gatewayEventCallbacks = [];
    setupInvoke({ get_version: '0.1.2' });
    setupGateway({ running: false, url: null });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('should show update banner when update is available', async () => {
    mockCheck.mockResolvedValue({ version: '2.0.0', body: 'New features' });

    render(<App />);

    // Banner should not be visible before the 5s delay
    expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();

    // Trigger the setTimeout, then switch to real timers so waitFor can poll
    vi.advanceTimersByTime(5000);
    vi.useRealTimers();

    await waitFor(() => {
      const banner = screen.getByTestId('update-banner');
      expect(banner).toBeInTheDocument();
      expect(banner).toHaveTextContent('v2.0.0');
      expect(banner).toHaveTextContent('is available');
    });
  });

  it('should not show banner when no update is available', async () => {
    mockCheck.mockResolvedValue(null);

    render(<App />);

    vi.advanceTimersByTime(5000);
    vi.useRealTimers();

    // Give the async check time to resolve and confirm no banner appears
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });
    expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
  });

  it('should not show banner when update check fails', async () => {
    mockCheck.mockRejectedValue(new Error('network error'));

    render(<App />);

    vi.advanceTimersByTime(5000);
    vi.useRealTimers();

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });
    expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
  });

  it('should dismiss banner when X button is clicked', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();

    mockCheck.mockResolvedValue({ version: '2.0.0', body: '' });

    render(<App />);

    // Wait for the 5s setTimeout + async check to complete
    await waitFor(
      () => {
        expect(screen.getByTestId('update-banner')).toBeInTheDocument();
      },
      { timeout: 7000 }
    );

    // Click dismiss
    await user.click(screen.getByTestId('dismiss-update-banner'));

    await waitFor(() => {
      expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
    });
  });

  it('should navigate to Settings and hide banner when "Update now" is clicked', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();

    mockCheck.mockResolvedValue({ version: '2.0.0', body: '' });

    render(<App />);

    await waitFor(
      () => {
        expect(screen.getByTestId('update-banner')).toBeInTheDocument();
      },
      { timeout: 7000 }
    );

    // Click "Update now"
    await user.click(screen.getByText('Update now'));

    await waitFor(() => {
      // Banner should be gone
      expect(screen.queryByTestId('update-banner')).not.toBeInTheDocument();
      // Settings page should be rendered
      expect(screen.getByTestId('settings-page')).toBeInTheDocument();
    });
  });
});
