import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Home,
  Server,
  Globe,
  Wrench,
  Monitor,
  Settings,
  Sun,
  Moon,
  Loader2,
  FolderOpen,
  FileText,
  Download,
  X,
} from 'lucide-react';
import {
  AppShell,
  Sidebar,
  SidebarItem,
  SidebarSection,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  Button,
} from '@mcpmux/ui';
import { ThemeProvider } from '@/components/ThemeProvider';
import { OAuthConsentModal } from '@/components/OAuthConsentModal';
import { ServerInstallModal } from '@/components/ServerInstallModal';
import { SpaceSwitcher } from '@/components/SpaceSwitcher';
import { ConnectIDEs } from '@/components/ConnectIDEs';
import { useDataSync } from '@/hooks/useDataSync';
import { useAppStore, useActiveSpace, useViewSpace, useTheme } from '@/stores';
import { RegistryPage } from '@/features/registry';
import { FeatureSetsPage } from '@/features/featuresets';
import { ClientsPage } from '@/features/clients';
import { ServersPage } from '@/features/servers';
import { SpacesPage } from '@/features/spaces';
import { SettingsPage } from '@/features/settings';
import { useGatewayEvents, useServerStatusEvents } from '@/hooks/useDomainEvents';

/** McpMux title-bar icon — miniature cat icon */
function McpMuxGlyph({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id="glyph-bg" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="var(--brand)" />
          <stop offset="100%" stopColor="var(--brand-dark)" />
        </linearGradient>
        <mask id="glyph-m">
          <rect width="32" height="32" fill="white" />
          <circle cx="12" cy="17.5" r="1.75" fill="black" />
          <circle cx="20" cy="17.5" r="1.75" fill="black" />
          <ellipse cx="16" cy="20.6" rx="1" ry="0.75" fill="black" />
        </mask>
      </defs>
      <rect width="32" height="32" rx="7" fill="url(#glyph-bg)" />
      {/* Cat silhouette with transparent eyes/nose */}
      <path d="M 16 25.3 C 8.7 25.3 4.9 21.3 4.9 17.2 C 4.9 14 6.3 13.4 8.3 15.4 C 8.1 10.3 6.1 5 7.2 4.4 C 8.9 3.4 11.8 8.2 13.4 12.2 C 14.3 10.7 14.9 10.3 16 10.3 C 17.1 10.3 17.7 10.7 18.6 12.2 C 20.2 8.2 23.1 3.4 24.8 4.4 C 25.9 5 23.9 10.3 23.7 15.4 C 25.7 13.4 27.1 14 27.1 17.2 C 27.1 21.3 23.3 25.3 16 25.3 Z" fill="white" opacity="0.88" mask="url(#glyph-m)" />
      {/* Smile */}
      <path d="M 13.9 22.2 Q 16 24.3 18.1 22.2" stroke="white" strokeWidth="0.9" strokeLinecap="round" fill="none" opacity="0.95" />
      {/* Whiskers left */}
      <path d="M 14 21 C 12.8 20.4 11 20.2 9.5 20.4 C 9 20.4 8.7 20.2 8.6 19.9" stroke="white" strokeWidth="0.9" strokeLinecap="round" fill="none" opacity="0.7" />
      <circle cx="8.6" cy="19.9" r="1.1" fill="white" opacity="0.75" />
      {/* Whiskers right */}
      <path d="M 18 21 C 19.2 20.4 21 20.2 22.5 20.4 C 23 20.4 23.3 20.2 23.4 19.9" stroke="white" strokeWidth="0.9" strokeLinecap="round" fill="none" opacity="0.7" />
      <circle cx="23.4" cy="19.9" r="1.1" fill="white" opacity="0.75" />
    </svg>
  );
}

type NavItem = 'home' | 'registry' | 'servers' | 'spaces' | 'featuresets' | 'clients' | 'settings';

function AppContent() {
  // Sync data from backend on mount
  useDataSync();

  const [activeNav, setActiveNav] = useState<NavItem>('home');
  const [availableUpdate, setAvailableUpdate] = useState<{ version: string } | null>(null);

  // Auto-check for updates on startup (silent check after 5 seconds)
  useEffect(() => {
    const checkForUpdates = async () => {
      try {
        const { check } = await import('@tauri-apps/plugin-updater');
        const update = await check();
        if (update) {
          console.log(`[Auto-Update] Update available: ${update.version}`);
          setAvailableUpdate({ version: update.version });
        }
      } catch (error) {
        console.error('[Auto-Update] Failed to check for updates:', error);
      }
    };

    const timer = setTimeout(checkForUpdates, 5000);
    return () => clearTimeout(timer);
  }, []);

  // Get state from store
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const activeSpace = useActiveSpace();
  const viewSpace = useViewSpace();

  // App version from Rust backend
  const [appVersion, setAppVersion] = useState('');
  useEffect(() => {
    invoke<string>('get_version')
      .then(setAppVersion)
      .catch((err) => console.error('Failed to get version:', err));
  }, []);

  // Gateway status for sidebar footer
  const [gatewayUrl, setGatewayUrl] = useState<string | null>(null);
  const loadGatewayUrl = useCallback(async () => {
    try {
      const { getGatewayStatus } = await import('@/lib/api/gateway');
      const status = await getGatewayStatus(viewSpace?.id);
      setGatewayUrl(status.running && status.url ? status.url : null);
    } catch {
      setGatewayUrl(null);
    }
  }, [viewSpace?.id]);

  useEffect(() => {
    loadGatewayUrl();
  }, [loadGatewayUrl]);

  useGatewayEvents((payload) => {
    if (payload.action === 'started') {
      setGatewayUrl(payload.url || null);
    } else if (payload.action === 'stopped') {
      setGatewayUrl(null);
    }
  });

  // Toggle dark mode
  const toggleDarkMode = () => {
    setTheme(theme === 'dark' ? 'light' : 'dark');
  };

  const sidebar = (
    <Sidebar
      header={
        <SpaceSwitcher />
      }
      footer={
        <div className="text-xs text-[rgb(var(--muted))]">
          <div>McpMux{appVersion ? ` v${appVersion}` : ''}</div>
          <div>Gateway: {gatewayUrl ?? 'Not running'}</div>
        </div>
      }
    >
      <SidebarSection>
        <SidebarItem
          icon={<Home className="h-4 w-4" />}
          label="Dashboard"
          active={activeNav === 'home'}
          onClick={() => setActiveNav('home')}
          data-testid="nav-dashboard"
        />
        <SidebarItem
          icon={<Server className="h-4 w-4" />}
          label="My Servers"
          active={activeNav === 'servers'}
          onClick={() => setActiveNav('servers')}
          data-testid="nav-my-servers"
        />
        <SidebarItem
          icon={<Server className="h-4 w-4" />}
          label="Discover"
          active={activeNav === 'registry'}
          onClick={() => setActiveNav('registry')}
          data-testid="nav-discover"
        />
      </SidebarSection>

      <SidebarSection title="Workspaces">
        <SidebarItem
          icon={<Globe className="h-4 w-4" />}
          label="Spaces"
          active={activeNav === 'spaces'}
          onClick={() => setActiveNav('spaces')}
          data-testid="nav-spaces"
        />
        <SidebarItem
          icon={<Wrench className="h-4 w-4" />}
          label="FeatureSets"
          active={activeNav === 'featuresets'}
          onClick={() => setActiveNav('featuresets')}
          data-testid="nav-featuresets"
        />
      </SidebarSection>

      <SidebarSection title="Connections">
        <SidebarItem
          icon={<Monitor className="h-4 w-4" />}
          label="Clients"
          active={activeNav === 'clients'}
          onClick={() => setActiveNav('clients')}
          data-testid="nav-clients"
        />
      </SidebarSection>

      <SidebarSection>
        <SidebarItem
          icon={<Settings className="h-4 w-4" />}
          label="Settings"
          active={activeNav === 'settings'}
          onClick={() => setActiveNav('settings')}
          data-testid="nav-settings"
        />
      </SidebarSection>
    </Sidebar>
  );

  const statusBar = (
    <div className="flex h-full items-center justify-between text-xs text-[rgb(var(--muted))]">
      <div className="flex items-center gap-4">
        <span className="flex items-center gap-1.5">
          <span className="h-2 w-2 rounded-full bg-green-500" />
          Gateway Active
        </span>
        <span>Active Space: {activeSpace?.name || 'None'}</span>
      </div>
      <div className="flex items-center gap-4">
        <span>5 Servers • 97 Tools</span>
      </div>
    </div>
  );

  const titleBar = (
    <div className="flex items-center gap-1.5 pl-3">
      <McpMuxGlyph className="h-4 w-4 shrink-0" />
      <span className="text-sm font-bold tracking-tight select-none">
        <span style={{ color: 'var(--brand-light)' }}>Mcp</span>
        <span style={{ color: 'var(--brand-dark)' }}>Mux</span>
      </span>
      <div className="mx-2 h-4 w-px bg-[rgb(var(--border))]" />
      <button
        onClick={toggleDarkMode}
        className="p-1 rounded-md hover:bg-[rgb(var(--surface-hover))] transition-colors"
        title={theme === 'dark' ? 'Light mode' : 'Dark mode'}
      >
        {theme === 'dark' ? <Sun className="h-3.5 w-3.5 text-[rgb(var(--muted))]" /> : <Moon className="h-3.5 w-3.5 text-[rgb(var(--muted))]" />}
      </button>
    </div>
  );

  return (
    <AppShell
      sidebar={sidebar}
      statusBar={statusBar}
      titleBar={titleBar}
      windowControls={
        <div className="flex items-center">
          <WindowButton action="minimize" />
          <WindowButton action="maximize" />
          <WindowButton action="close" />
        </div>
      }
    >
      <div className="animate-fade-in">
        {availableUpdate && (
          <div
            className="flex items-center justify-between gap-3 px-4 py-2.5 bg-blue-500/10 border-b border-blue-500/20 text-sm"
            data-testid="update-banner"
          >
            <div className="flex items-center gap-2">
              <Download className="h-4 w-4 text-blue-500 flex-shrink-0" />
              <span>
                McpMux <strong>v{availableUpdate.version}</strong> is available.
              </span>
              <button
                onClick={() => {
                  setActiveNav('settings');
                  setAvailableUpdate(null);
                }}
                className="text-blue-500 hover:text-blue-400 font-medium underline underline-offset-2"
              >
                Update now
              </button>
            </div>
            <button
              onClick={() => setAvailableUpdate(null)}
              className="text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] transition-colors flex-shrink-0"
              aria-label="Dismiss update notification"
              data-testid="dismiss-update-banner"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        )}
        {activeNav === 'home' && <DashboardView />}
        {activeNav === 'registry' && <RegistryPage />}
        {activeNav === 'servers' && <ServersPage />}
        {activeNav === 'spaces' && <SpacesPage />}
        {activeNav === 'featuresets' && <FeatureSetsPage />}
        {activeNav === 'clients' && <ClientsPage />}
        {activeNav === 'settings' && <SettingsPage />}
      </div>
    </AppShell>
  );
}

function App() {
  return (
    <ThemeProvider>
      <AppContent />
      {/* OAuth consent modal - shown when MCP clients request authorization */}
      <OAuthConsentModal />
      {/* Server install modal - shown when install deep link is received */}
      <ServerInstallModal />
    </ThemeProvider>
  );
}

function DashboardView() {
  const [stats, setStats] = useState({
    installedServers: 0,
    connectedServers: 0,
    tools: 0,
    clients: 0,
    featureSets: 0,
  });
  const [gatewayStatus, setGatewayStatus] = useState<{
    running: boolean;
    url: string | null;
  }>({ running: false, url: null });
  const viewSpace = useViewSpace();

  // Load stats on mount and when gateway changes
  const loadStats = async () => {
    try {
      const [clients, featureSets, gateway, installedServers] = await Promise.all([
        import('@/lib/api/clients').then((m) => m.listClients()),
        import('@/lib/api/featureSets').then((m) =>
          viewSpace?.id ? m.listFeatureSetsBySpace(viewSpace.id) : m.listFeatureSets()
        ),
        import('@/lib/api/gateway').then((m) => m.getGatewayStatus(viewSpace?.id)),
        import('@/lib/api/registry').then((m) => m.listInstalledServers(viewSpace?.id)),
      ]);
      console.log('[Dashboard] Gateway status received:', gateway);
      setStats({
        installedServers: installedServers.length,
        connectedServers: gateway.connected_backends,
        tools: 0, // Will be populated when servers report tools
        clients: clients.length,
        featureSets: featureSets.length,
      });
      setGatewayStatus({ running: gateway.running, url: gateway.url });
    } catch (e) {
      console.error('Failed to load dashboard stats:', e);
    }
  };

  // Load stats on mount and when viewing space changes
  useEffect(() => {
    loadStats();
  }, [viewSpace?.id]);

  // Subscribe to gateway events for reactive updates (no polling!)
  useGatewayEvents((payload) => {
    if (payload.action === 'started') {
      setGatewayStatus({ running: true, url: payload.url || null });
      // Reload stats to get updated counts
      loadStats();
    } else if (payload.action === 'stopped') {
      setGatewayStatus({ running: false, url: null });
      setStats({ installedServers: 0, connectedServers: 0, tools: 0, clients: 0, featureSets: 0 });
    }
  });

  // Subscribe to server status changes to update connected count
  useServerStatusEvents((payload) => {
    if (payload.status === 'connected' || payload.status === 'disconnected') {
      loadStats();
    }
  });

  const handleToggleGateway = async () => {
    try {
      if (gatewayStatus.running) {
        const { stopGateway } = await import('@/lib/api/gateway');
        await stopGateway();
        setGatewayStatus({ running: false, url: null });
      } else {
        const { startGateway } = await import('@/lib/api/gateway');
        const url = await startGateway();
        setGatewayStatus({ running: true, url });
        // After starting gateway, reload stats to get updated connected count
        setTimeout(loadStats, 500);
      }
    } catch (e) {
      console.error('Gateway toggle failed:', e);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Dashboard</h1>
        <p className="text-[rgb(var(--muted))]">
          Welcome to McpMux - your centralized MCP server manager.
        </p>
      </div>

      {/* Gateway Status Banner */}
      <Card className={gatewayStatus.running ? 'border-green-500' : 'border-orange-500'} data-testid="gateway-status-card">
        <CardContent className="flex items-center justify-between py-3">
          <div className="flex items-center gap-3">
            <span
              className={`h-3 w-3 rounded-full ${
                gatewayStatus.running ? 'bg-green-500' : 'bg-orange-500'
              }`}
              data-testid="gateway-status-indicator"
            />
            <div>
              <span className="font-medium" data-testid="gateway-status-text">
                Gateway: {gatewayStatus.running ? 'Running' : 'Stopped'}
              </span>
              {gatewayStatus.url && (
                <span className="text-sm text-[rgb(var(--muted))] ml-2" data-testid="gateway-url">
                  {gatewayStatus.url}
                </span>
              )}
            </div>
          </div>
          <Button
            variant={gatewayStatus.running ? 'ghost' : 'primary'}
            size="sm"
            onClick={handleToggleGateway}
            data-testid="gateway-toggle-btn"
          >
            {gatewayStatus.running ? 'Stop' : 'Start'}
          </Button>
        </CardContent>
      </Card>

      {/* Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4" data-testid="dashboard-stats-grid">
        <Card data-testid="stat-servers">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Server className="h-5 w-5 text-primary-500" />
              Servers
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold" data-testid="stat-servers-value">{stats.connectedServers}/{stats.installedServers}</div>
            <div className="text-sm text-[rgb(var(--muted))]">Connected / Installed</div>
          </CardContent>
        </Card>

        <Card data-testid="stat-featuresets">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Wrench className="h-5 w-5 text-primary-500" />
              FeatureSets
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold" data-testid="stat-featuresets-value">{stats.featureSets}</div>
            <div className="text-sm text-[rgb(var(--muted))]">Permission bundles</div>
          </CardContent>
        </Card>

        <Card data-testid="stat-clients">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Monitor className="h-5 w-5 text-primary-500" />
              Clients
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold" data-testid="stat-clients-value">{stats.clients}</div>
            <div className="text-sm text-[rgb(var(--muted))]">Registered AI clients</div>
          </CardContent>
        </Card>

        <Card data-testid="stat-active-space">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Globe className="h-5 w-5 text-primary-500" />
              Active Space
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-xl font-bold truncate" data-testid="stat-active-space-value">
              {viewSpace?.icon} {viewSpace?.name || 'None'}
            </div>
            <div className="text-sm text-[rgb(var(--muted))]">Current context</div>
          </CardContent>
        </Card>
      </div>

      {/* Connect IDEs — one-click install */}
      <ConnectIDEs
        gatewayUrl={gatewayStatus.url || 'http://localhost:3100'}
        gatewayRunning={gatewayStatus.running}
      />
    </div>
  );
}

/** Window control button for custom title bar */
function WindowButton({ action }: { action: 'minimize' | 'maximize' | 'close' }) {
  const handleClick = async () => {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const appWindow = getCurrentWindow();
    if (action === 'minimize') appWindow.minimize();
    else if (action === 'maximize') appWindow.toggleMaximize();
    else appWindow.close();
  };

  return (
    <button
      onClick={handleClick}
      className={`h-9 w-11 flex items-center justify-center transition-colors ${
        action === 'close'
          ? 'hover:bg-red-500 hover:text-white'
          : 'hover:bg-[rgb(var(--surface-hover))]'
      }`}
    >
      {action === 'minimize' && (
        <svg width="10" height="1" viewBox="0 0 10 1"><rect width="10" height="1" fill="currentColor" /></svg>
      )}
      {action === 'maximize' && (
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" strokeWidth="1" /></svg>
      )}
      {action === 'close' && (
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0L10 10M10 0L0 10" stroke="currentColor" strokeWidth="1.2" /></svg>
      )}
    </button>
  );
}

export default App;
