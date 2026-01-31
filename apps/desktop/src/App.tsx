import { useState, useEffect } from 'react';
import {
  Home,
  Server,
  Globe,
  Wrench,
  Monitor,
  Settings,
  Sun,
  Moon,
  Zap,
  Plus,
  Trash2,
  Check,
  Loader2,
  FolderOpen,
  FileText,
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
} from '@mcmux/ui';
import { ThemeProvider } from '@/components/ThemeProvider';
import { OAuthConsentModal } from '@/components/OAuthConsentModal';
import { SpaceSwitcher } from '@/components/SpaceSwitcher';
import { useDataSync } from '@/hooks/useDataSync';
import { useAppStore, useActiveSpace, useViewSpace, useSpaces, useIsLoading, useTheme } from '@/stores';
import { createSpace, deleteSpace, setActiveSpace as setActiveSpaceAPI } from '@/lib/api/spaces';
import { RegistryPage } from '@/features/registry';
import { FeatureSetsPage } from '@/features/featuresets';
import { ClientsPage } from '@/features/clients';
import { ServersPage } from '@/features/servers';
import { SpacesPage } from '@/features/spaces';
import { useGatewayEvents, useServerStatusEvents } from '@/hooks/useDomainEvents';

type NavItem = 'home' | 'registry' | 'servers' | 'spaces' | 'featuresets' | 'clients' | 'settings';

function AppContent() {
  // Sync data from backend on mount
  useDataSync();

  const [activeNav, setActiveNav] = useState<NavItem>('home');

  // Get state from store
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const activeSpace = useActiveSpace();

  // Toggle dark mode
  const toggleDarkMode = () => {
    setTheme(theme === 'dark' ? 'light' : 'dark');
  };

  const sidebar = (
    <Sidebar
      header={
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Zap className="h-6 w-6 text-primary-500" />
              <span className="font-bold text-lg">MCP Mux</span>
            </div>
            <button
              onClick={toggleDarkMode}
              className="p-1.5 rounded-lg hover:bg-surface-hover transition-colors"
              title={theme === 'dark' ? 'Light mode' : 'Dark mode'}
            >
              {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
            </button>
          </div>
          {/* Space Switcher in sidebar */}
          <SpaceSwitcher className="w-full" />
        </div>
      }
      footer={
        <div className="text-xs text-[rgb(var(--muted))]">
          <div>MCP Mux v0.1.0</div>
          <div>Gateway: localhost:9315</div>
        </div>
      }
    >
      <SidebarSection>
        <SidebarItem
          icon={<Home className="h-4 w-4" />}
          label="Dashboard"
          active={activeNav === 'home'}
          onClick={() => setActiveNav('home')}
        />
        <SidebarItem
          icon={<Zap className="h-4 w-4" />}
          label="My Servers"
          active={activeNav === 'servers'}
          onClick={() => setActiveNav('servers')}
        />
        <SidebarItem
          icon={<Server className="h-4 w-4" />}
          label="Discover"
          active={activeNav === 'registry'}
          onClick={() => setActiveNav('registry')}
        />
      </SidebarSection>

      <SidebarSection title="Workspaces">
        <SidebarItem
          icon={<Globe className="h-4 w-4" />}
          label="Spaces"
          active={activeNav === 'spaces'}
          onClick={() => setActiveNav('spaces')}
        />
        <SidebarItem
          icon={<Wrench className="h-4 w-4" />}
          label="FeatureSets"
          active={activeNav === 'featuresets'}
          onClick={() => setActiveNav('featuresets')}
        />
      </SidebarSection>

      <SidebarSection title="Connections">
        <SidebarItem
          icon={<Monitor className="h-4 w-4" />}
          label="Clients"
          active={activeNav === 'clients'}
          onClick={() => setActiveNav('clients')}
        />
      </SidebarSection>

      <SidebarSection>
        <SidebarItem
          icon={<Settings className="h-4 w-4" />}
          label="Settings"
          active={activeNav === 'settings'}
          onClick={() => setActiveNav('settings')}
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

  return (
    <AppShell sidebar={sidebar} statusBar={statusBar}>
      <div className="animate-fade-in">
        {activeNav === 'home' && <DashboardView />}
        {activeNav === 'registry' && <RegistryPage />}
        {activeNav === 'servers' && <ServersPage />}
        {activeNav === 'spaces' && <SpacesPage />}
        {activeNav === 'featuresets' && <FeatureSetsPage />}
        {activeNav === 'clients' && <ClientsPage />}
        {activeNav === 'settings' && <SettingsView />}
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
  const [exportSuccess, setExportSuccess] = useState<string | null>(null);
  const viewSpace = useViewSpace();

  // Load stats on mount and when gateway changes
  const loadStats = async () => {
    try {
      const [clients, featureSets, gateway, installedServers] = await Promise.all([
        import('@/lib/api/clients').then((m) => m.listClients()),
        import('@/lib/api/featureSets').then((m) =>
          viewSpace?.id ? m.listFeatureSetsBySpace(viewSpace.id) : m.listFeatureSets()
        ),
        import('@/lib/api/gateway').then((m) => m.getGatewayStatus()),
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
          Welcome to MCP Mux - your centralized MCP server manager.
        </p>
      </div>

      {/* Gateway Status Banner */}
      <Card className={gatewayStatus.running ? 'border-green-500' : 'border-orange-500'}>
        <CardContent className="flex items-center justify-between py-3">
          <div className="flex items-center gap-3">
            <span
              className={`h-3 w-3 rounded-full ${
                gatewayStatus.running ? 'bg-green-500' : 'bg-orange-500'
              }`}
            />
            <div>
              <span className="font-medium">
                Gateway: {gatewayStatus.running ? 'Running' : 'Stopped'}
              </span>
              {gatewayStatus.url && (
                <span className="text-sm text-[rgb(var(--muted))] ml-2">
                  {gatewayStatus.url}
                </span>
              )}
            </div>
          </div>
          <Button
            variant={gatewayStatus.running ? 'ghost' : 'primary'}
            size="sm"
            onClick={handleToggleGateway}
          >
            {gatewayStatus.running ? 'Stop' : 'Start'}
          </Button>
        </CardContent>
      </Card>

      {/* Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Server className="h-5 w-5 text-primary-500" />
              Servers
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold">{stats.connectedServers}/{stats.installedServers}</div>
            <div className="text-sm text-[rgb(var(--muted))]">Connected / Installed</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Wrench className="h-5 w-5 text-primary-500" />
              FeatureSets
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold">{stats.featureSets}</div>
            <div className="text-sm text-[rgb(var(--muted))]">Permission bundles</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Monitor className="h-5 w-5 text-primary-500" />
              Clients
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold">{stats.clients}</div>
            <div className="text-sm text-[rgb(var(--muted))]">Registered AI clients</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Globe className="h-5 w-5 text-primary-500" />
              Active Space
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-xl font-bold truncate">
              {viewSpace?.icon} {viewSpace?.name || 'None'}
            </div>
            <div className="text-sm text-[rgb(var(--muted))]">Current context</div>
          </CardContent>
        </Card>
      </div>

      {/* Client Config */}
      <Card>
        <CardHeader>
          <CardTitle>Connect Your Client</CardTitle>
          <CardDescription>
            Add this server configuration to your MCP client settings (e.g., inside mcpServers section).
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            {/* Gateway URL */}
            <div className="flex items-center gap-2 text-sm">
              <span className={`h-2 w-2 rounded-full ${gatewayStatus.running ? 'bg-green-500' : 'bg-orange-500'}`} />
              <span className="text-[rgb(var(--muted))]">Gateway:</span>
              <code className="bg-[var(--surface)] px-2 py-1 rounded text-cyan-500">
                {gatewayStatus.url || 'http://localhost:3100'}
              </code>
              {!gatewayStatus.running && (
                <span className="text-orange-500 text-xs">(not running)</span>
              )}
            </div>

            {/* Config Display */}
            <div className="relative">
              <pre className="bg-slate-900 text-slate-100 p-4 rounded-lg text-sm overflow-x-auto font-mono">
{`"mcpmux": {
  "url": "${gatewayStatus.url || 'http://localhost:3100'}/mcp"
}`}
              </pre>
              <Button
                variant="secondary"
                size="sm"
                className="absolute top-2 right-2"
                onClick={async () => {
                  const config = `"mcpmux": {\n  "url": "${gatewayStatus.url || 'http://localhost:3100'}/mcp"\n}`;
                  await navigator.clipboard.writeText(config);
                  setExportSuccess('Config copied to clipboard!');
                  setTimeout(() => setExportSuccess(null), 2000);
                }}
              >
                📋 Copy
              </Button>
            </div>

            {exportSuccess && (
              <div className="text-sm text-green-600 dark:text-green-400 flex items-center gap-1">
                <Check className="h-4 w-4" />
                {exportSuccess}
              </div>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function SettingsView() {
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const [logsPath, setLogsPath] = useState<string>('');
  const [openingLogs, setOpeningLogs] = useState(false);

  // Load logs path on mount
  useEffect(() => {
    const loadLogsPath = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const path = await invoke<string>('get_logs_path');
        setLogsPath(path);
      } catch (error) {
        console.error('Failed to get logs path:', error);
      }
    };
    loadLogsPath();
  }, []);

  const handleOpenLogs = async () => {
    setOpeningLogs(true);
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('open_logs_folder');
    } catch (error) {
      console.error('Failed to open logs folder:', error);
    } finally {
      setOpeningLogs(false);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Settings</h1>
        <p className="text-[rgb(var(--muted))]">Configure MCP Mux preferences.</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
          <CardDescription>Customize the look and feel of MCP Mux.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">Theme</label>
              <div className="flex gap-2 mt-2">
                <Button
                  variant={theme === 'light' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('light')}
                >
                  <Sun className="h-4 w-4 mr-2" />
                  Light
                </Button>
                <Button
                  variant={theme === 'dark' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('dark')}
                >
                  <Moon className="h-4 w-4 mr-2" />
                  Dark
                </Button>
                <Button
                  variant={theme === 'system' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('system')}
                >
                  <Monitor className="h-4 w-4 mr-2" />
                  System
                </Button>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            Logs
          </CardTitle>
          <CardDescription>View application logs for debugging and troubleshooting.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">Log Files Location</label>
              <p className="text-sm text-[rgb(var(--muted))] mt-1 font-mono bg-surface-secondary rounded px-2 py-1">
                {logsPath || 'Loading...'}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={handleOpenLogs}
                disabled={openingLogs}
              >
                {openingLogs ? (
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                ) : (
                  <FolderOpen className="h-4 w-4 mr-2" />
                )}
                Open Logs Folder
              </Button>
            </div>
            <p className="text-xs text-[rgb(var(--muted))]">
              Logs are rotated daily. Each file contains detailed debug information including thread IDs and source locations.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

export default App;
