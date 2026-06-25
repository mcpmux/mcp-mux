import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Sun, Moon, Download, X } from 'lucide-react';
import { AppShell, Sidebar, SidebarItem, SidebarSection } from '@mcpmux/ui';
import { ThemeProvider } from '@/components/ThemeProvider';
import { OAuthConsentModal } from '@/components/OAuthConsentModal';
import { ServerInstallModal } from '@/components/ServerInstallModal';
import { SpaceSwitcher } from '@/components/SpaceSwitcher';
import { useDataSync } from '@/hooks/useDataSync';
import { useAnalytics } from '@/hooks/useAnalytics';
import { startMetaToolActivityListener } from '@/stores/metaToolActivityStore';
import { initAnalytics, capture, optIn, optOut } from '@/lib/analytics';
import {
  useAppStore,
  useViewSpace,
  useTheme,
  useAnalyticsEnabled,
  useActiveNav,
  useNavigateTo,
  useSetPendingSettingsSection,
} from '@/stores';
import { NAV_ZONES, NAV_SETTINGS } from '@/lib/navigation';
import { spaceAccentColor } from '@/lib/spaceAccent';
import { HomePage } from '@/features/home';
import { RegistryPage } from '@/features/registry';
import { FeatureSetsPage } from '@/features/featuresets';
import { ClientsPage } from '@/features/clients';
import { ServersPage } from '@/features/servers';
import { SpacesPage } from '@/features/spaces';
import { WorkspacesPage } from '@/features/workspaces';
import { SettingsPage } from '@/features/settings';
import { BuiltinServersPage } from '@/features/builtinServers';
import { AutoStartConflictResolver } from '@/features/gateway/AutoStartConflictResolver';
import { WorkspaceBindingSheet } from '@/features/workspaces';
import { MetaToolApprovalDialog } from '@/features/metaTools';
import { useGatewayEvents } from '@/hooks/useDomainEvents';

/** McpMux title-bar icon — miniature cat icon */
function McpMuxGlyph({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
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
      <path
        d="M 16 25.3 C 8.7 25.3 4.9 21.3 4.9 17.2 C 4.9 14 6.3 13.4 8.3 15.4 C 8.1 10.3 6.1 5 7.2 4.4 C 8.9 3.4 11.8 8.2 13.4 12.2 C 14.3 10.7 14.9 10.3 16 10.3 C 17.1 10.3 17.7 10.7 18.6 12.2 C 20.2 8.2 23.1 3.4 24.8 4.4 C 25.9 5 23.9 10.3 23.7 15.4 C 25.7 13.4 27.1 14 27.1 17.2 C 27.1 21.3 23.3 25.3 16 25.3 Z"
        fill="white"
        opacity="0.88"
        mask="url(#glyph-m)"
      />
      {/* Smile */}
      <path
        d="M 13.9 22.2 Q 16 24.3 18.1 22.2"
        stroke="white"
        strokeWidth="0.9"
        strokeLinecap="round"
        fill="none"
        opacity="0.95"
      />
      {/* Whiskers left */}
      <path
        d="M 14 21 C 12.8 20.4 11 20.2 9.5 20.4 C 9 20.4 8.7 20.2 8.6 19.9"
        stroke="white"
        strokeWidth="0.9"
        strokeLinecap="round"
        fill="none"
        opacity="0.7"
      />
      <circle cx="8.6" cy="19.9" r="1.1" fill="white" opacity="0.75" />
      {/* Whiskers right */}
      <path
        d="M 18 21 C 19.2 20.4 21 20.2 22.5 20.4 C 23 20.4 23.3 20.2 23.4 19.9"
        stroke="white"
        strokeWidth="0.9"
        strokeLinecap="round"
        fill="none"
        opacity="0.7"
      />
      <circle cx="23.4" cy="19.9" r="1.1" fill="white" opacity="0.75" />
    </svg>
  );
}

function AppContent() {
  // Sync data from backend on mount
  useDataSync();

  const activeNav = useActiveNav();
  const navigateTo = useNavigateTo();
  const setPendingSettingsSection = useSetPendingSettingsSection();
  const [availableUpdate, setAvailableUpdate] = useState<{ version: string } | null>(null);

  // Auto-check for updates on startup (silent check after 5 seconds).
  // When auto-install is enabled (the default), download + install + relaunch
  // into the new version — so a restart picks up updates with no clicks.
  // Otherwise just surface the dismissible banner for a manual install.
  useEffect(() => {
    // Never auto-update under `pnpm dev`. A dev build would otherwise detect a
    // newer published release, install it over this build, and relaunch — so
    // your local changes would vanish before you could see them. Production
    // builds (import.meta.env.DEV === false) are unaffected.
    if (import.meta.env.DEV) return;

    const checkForUpdates = async () => {
      try {
        const { checkForUpdate } = await import('@/lib/updates');
        const update = await checkForUpdate();
        if (!update) return;
        console.log(`[Auto-Update] Update available: ${update.version}`);

        // Default to auto-install; honor the persisted opt-out.
        let autoInstall = true;
        try {
          autoInstall = await invoke<boolean>('get_auto_install_updates');
        } catch {
          /* setting unavailable → keep the auto-install default */
        }

        if (autoInstall) {
          console.log('[Auto-Update] Auto-installing update and relaunching…');
          await update.downloadAndInstall();
          const { relaunch } = await import('@tauri-apps/plugin-process');
          await relaunch();
        } else {
          setAvailableUpdate({ version: update.version });
        }
      } catch (error) {
        console.error('[Auto-Update] Failed to check/install updates:', error);
      }
    };

    const timer = setTimeout(checkForUpdates, 5000);
    return () => clearTimeout(timer);
  }, []);

  // Get state from store
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const viewSpace = useViewSpace();
  const analyticsEnabled = useAnalyticsEnabled();

  // App version from Rust backend
  const [appVersion, setAppVersion] = useState('');
  useEffect(() => {
    invoke<string>('get_version')
      .then(setAppVersion)
      .catch((err) => console.error('Failed to get version:', err));
  }, []);

  // Initialize analytics once we have the app version
  useEffect(() => {
    if (!appVersion) return;
    initAnalytics(appVersion);
    if (analyticsEnabled) {
      optIn();
      capture('app_opened');
    } else {
      optOut();
    }
  }, [appVersion]); // eslint-disable-line react-hooks/exhaustive-deps

  // Start the app-wide meta-tool activity listener once at launch so the
  // "Recent meta-tool activity" panel accumulates rows for the whole session
  // and survives tab changes (the listener is idempotent and app-scoped).
  useEffect(() => {
    startMetaToolActivityListener();
  }, []);

  // Sync opt-in/out when user toggles analytics
  useEffect(() => {
    if (!appVersion) return;
    if (analyticsEnabled) {
      optIn();
    } else {
      optOut();
    }
  }, [analyticsEnabled, appVersion]);

  // Track domain events (server install/uninstall)
  useAnalytics();

  // Track page navigation
  useEffect(() => {
    capture('page_viewed', { page: activeNav });
  }, [activeNav]);

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

  const gatewayRunning = gatewayUrl !== null;
  const gatewayPort = (() => {
    if (!gatewayUrl) return null;
    try {
      return new URL(gatewayUrl).port || null;
    } catch {
      return null;
    }
  })();

  // Sidebar renders entirely from the navigation model (lib/navigation.ts) —
  // future surfaces (Chat, Agents, Models) are config additions, not layout work.
  const sidebar = (
    <Sidebar
      header={<SpaceSwitcher />}
      footer={
        <SidebarItem
          icon={<NAV_SETTINGS.icon className="h-4 w-4" />}
          label={NAV_SETTINGS.label}
          hint={NAV_SETTINGS.hint}
          active={activeNav === NAV_SETTINGS.key}
          onClick={() => navigateTo(NAV_SETTINGS.key)}
          data-testid={NAV_SETTINGS.testId}
        />
      }
    >
      {NAV_ZONES.map((zone, i) => (
        <SidebarSection key={zone.title ?? `zone-${i}`} title={zone.title}>
          {zone.entries.map((entry) => (
            <SidebarItem
              key={entry.key}
              icon={<entry.icon className="h-4 w-4" />}
              label={entry.label}
              hint={entry.hint}
              active={activeNav === entry.key}
              onClick={() => navigateTo(entry.key)}
              data-testid={entry.testId}
            />
          ))}
        </SidebarSection>
      ))}
    </Sidebar>
  );

  const statusBar = (
    <div className="flex h-full items-center justify-between text-xs text-[rgb(var(--muted))]">
      <div className="flex items-center gap-4">
        <button
          type="button"
          onClick={() => navigateTo('home')}
          className="flex items-center gap-1.5 transition-colors hover:text-[rgb(var(--foreground))]"
          data-testid="statusbar-gateway"
          title="View connection details on the dashboard"
        >
          <span
            className={`h-2 w-2 rounded-full ${
              gatewayRunning ? 'bg-green-500' : 'bg-zinc-400 dark:bg-zinc-600'
            }`}
          />
          {gatewayRunning ? `Gateway${gatewayPort ? ` · :${gatewayPort}` : ''}` : 'Gateway stopped'}
        </button>
        <span className="flex items-center gap-1.5">
          <span
            aria-hidden
            className="h-2 w-2 rounded-full"
            style={{ backgroundColor: spaceAccentColor(viewSpace?.id) }}
          />
          Space: {viewSpace?.name || 'None'}
        </span>
      </div>
      {appVersion && (
        <span className="opacity-70" data-testid="statusbar-version">
          v{appVersion}
        </span>
      )}
    </div>
  );

  const titleBar = (
    <div className="flex h-full items-center gap-1.5 pl-3" data-tauri-drag-region>
      <McpMuxGlyph className="h-4 w-4 shrink-0" />
      <span className="select-none text-sm font-bold tracking-tight" data-tauri-drag-region>
        <span style={{ color: 'var(--brand-light)' }}>Mcp</span>
        <span style={{ color: 'var(--brand-dark)' }}>Mux</span>
      </span>
      <div className="mx-2 h-4 w-px bg-[rgb(var(--border))]" data-tauri-drag-region />
      <button
        onClick={toggleDarkMode}
        className="no-drag rounded-md p-1 transition-colors hover:bg-[rgb(var(--surface-hover))]"
        title={theme === 'dark' ? 'Light mode' : 'Dark mode'}
      >
        {theme === 'dark' ? (
          <Sun className="h-3.5 w-3.5 text-[rgb(var(--muted))]" />
        ) : (
          <Moon className="h-3.5 w-3.5 text-[rgb(var(--muted))]" />
        )}
      </button>
    </div>
  );

  return (
    <AppShell
      sidebar={sidebar}
      statusBar={statusBar}
      titleBar={titleBar}
      windowControls={
        <div className="no-drag flex h-full items-center">
          <WindowButton action="minimize" />
          <WindowButton action="maximize" />
          <WindowButton action="close" />
        </div>
      }
    >
      <div className="animate-fade-in">
        {availableUpdate && (
          <div
            className="flex items-center justify-between gap-3 border-b border-blue-500/20 bg-blue-500/10 px-4 py-2.5 text-sm"
            data-testid="update-banner"
          >
            <div className="flex items-center gap-2">
              <Download className="h-4 w-4 flex-shrink-0 text-blue-500" />
              <span>
                McpMux <strong>v{availableUpdate.version}</strong> is available.
              </span>
              <button
                onClick={() => {
                  // Land on (and flash) the Updates section, not the top of Settings.
                  setPendingSettingsSection('updates');
                  navigateTo('settings');
                  setAvailableUpdate(null);
                }}
                className="font-medium text-blue-500 underline underline-offset-2 hover:text-blue-400"
              >
                Update now
              </button>
            </div>
            <button
              onClick={() => setAvailableUpdate(null)}
              className="flex-shrink-0 text-[rgb(var(--muted))] transition-colors hover:text-[rgb(var(--foreground))]"
              aria-label="Dismiss update notification"
              data-testid="dismiss-update-banner"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        )}
        {activeNav === 'home' && <HomePage />}
        {activeNav === 'registry' && <RegistryPage />}
        {activeNav === 'servers' && <ServersPage />}
        {activeNav === 'spaces' && <SpacesPage />}
        {activeNav === 'featuresets' && <FeatureSetsPage />}
        {activeNav === 'workspaces' && <WorkspacesPage />}
        {activeNav === 'clients' && <ClientsPage />}
        {activeNav === 'builtin-servers' && <BuiltinServersPage />}
        {activeNav === 'settings' && <SettingsPage />}
      </div>
    </AppShell>
  );
}

function App() {
  return (
    <ThemeProvider>
      <AppContent />
      {/* Resolves deferred auto-start port conflicts — runs once on mount */}
      <AutoStartConflictResolver />
      {/* OAuth consent modal - shown when MCP clients request authorization */}
      <OAuthConsentModal />
      {/* Workspace binding sheet - slides in when a session reports a root
          that has no binding yet and resolved via the Space default */}
      <WorkspaceBindingSheet />
      {/* Server install modal - shown when install deep link is received */}
      <ServerInstallModal />
      {/* Meta-tool approval dialog — gates every mcpmux_* write tool */}
      <MetaToolApprovalDialog />
    </ThemeProvider>
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
      className={`no-drag flex h-9 w-11 items-center justify-center transition-colors ${
        action === 'close'
          ? 'hover:bg-red-500 hover:text-white'
          : 'hover:bg-[rgb(var(--surface-hover))]'
      }`}
    >
      {action === 'minimize' && (
        <svg width="10" height="1" viewBox="0 0 10 1">
          <rect width="10" height="1" fill="currentColor" />
        </svg>
      )}
      {action === 'maximize' && (
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" strokeWidth="1" />
        </svg>
      )}
      {action === 'close' && (
        <svg width="10" height="10" viewBox="0 0 10 10">
          <path d="M0 0L10 10M10 0L0 10" stroke="currentColor" strokeWidth="1.2" />
        </svg>
      )}
    </button>
  );
}

export default App;
