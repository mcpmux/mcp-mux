import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  Button,
  Switch,
  useToast,
  ToastContainer,
} from '@mcpmux/ui';
import {
  Sun,
  Moon,
  Monitor,
  FileText,
  FolderOpen,
  Loader2,
  Power,
  Minimize2,
  XCircle,
  Trash2,
  BarChart3,
  Github,
  Bug,
  Lightbulb,
  Package,
  Heart,
} from 'lucide-react';
import { useAppStore, useTheme, useAnalyticsEnabled } from '@/stores';
import { UpdateChecker } from './UpdateChecker';
import { CONTRIBUTE, openExternal } from '@/lib/contribute';

interface StartupSettings {
  autoLaunch: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
}

export function SettingsPage() {
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const analyticsEnabled = useAnalyticsEnabled();
  const setAnalyticsEnabled = useAppStore((state) => state.setAnalyticsEnabled);
  const [logsPath, setLogsPath] = useState<string>('');
  const [openingLogs, setOpeningLogs] = useState(false);
  const { toasts, success, error } = useToast();

  // Startup settings state
  const [startupSettings, setStartupSettings] = useState<StartupSettings>({
    autoLaunch: false,
    startMinimized: false,
    closeToTray: true,
  });
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [savingSettings, setSavingSettings] = useState(false);

  // Log retention state
  const [logRetentionDays, setLogRetentionDays] = useState<number>(30);
  const [savingRetention, setSavingRetention] = useState(false);

  // Load logs path on mount
  useEffect(() => {
    const loadLogsPath = async () => {
      try {
        const path = await invoke<string>('get_logs_path');
        setLogsPath(path);
      } catch (error) {
        console.error('Failed to get logs path:', error);
      }
    };
    loadLogsPath();
  }, []);

  // Load log retention setting on mount
  useEffect(() => {
    const loadRetention = async () => {
      try {
        const days = await invoke<number>('get_log_retention_days');
        setLogRetentionDays(days);
      } catch (err) {
        console.error('Failed to load log retention setting:', err);
      }
    };
    loadRetention();
  }, []);

  // Load startup settings on mount
  useEffect(() => {
    const loadStartupSettings = async () => {
      try {
        const settings = await invoke<StartupSettings>('get_startup_settings');
        setStartupSettings(settings);
      } catch (error) {
        console.error('Failed to load startup settings:', error);
      } finally {
        setLoadingSettings(false);
      }
    };
    loadStartupSettings();
  }, []);

  // Save startup settings when they change
  const updateStartupSetting = async (
    key: keyof StartupSettings,
    value: boolean
  ) => {
    console.log(`[Settings] Updating ${key} to ${value}`);
    
    // Save old state for rollback
    const oldSettings = { ...startupSettings };
    const newSettings = { ...startupSettings, [key]: value };
    
    // Update UI immediately for better UX
    setStartupSettings(newSettings);
    setSavingSettings(true);
    
    try {
      console.log('[Settings] Invoking update_startup_settings:', newSettings);
      await invoke('update_startup_settings', { settings: newSettings });
      console.log('[Settings] Successfully saved:', newSettings);
      
      // Show success toast
      success('Settings saved', 'Your preferences have been updated');
    } catch (err) {
      console.error('[Settings] Failed to save:', err);
      // Show error toast
      const errorMessage = err instanceof Error ? err.message : 'Unknown error';
      error('Failed to save settings', errorMessage);
      // Revert on error
      setStartupSettings(oldSettings);
    } finally {
      setSavingSettings(false);
    }
  };

  const handleRetentionChange = async (days: number) => {
    const oldDays = logRetentionDays;
    setLogRetentionDays(days);
    setSavingRetention(true);
    try {
      await invoke('set_log_retention_days', { days });
      success('Settings saved', `Log retention set to ${days === 0 ? 'keep forever' : `${days} days`}`);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Unknown error';
      error('Failed to save setting', errorMessage);
      setLogRetentionDays(oldDays);
    } finally {
      setSavingRetention(false);
    }
  };

  const handleOpenLogs = async () => {
    setOpeningLogs(true);
    try {
      await invoke('open_logs_folder');
    } catch (error) {
      console.error('Failed to open logs folder:', error);
    } finally {
      setOpeningLogs(false);
    }
  };

  return (
    <>
      <ToastContainer toasts={toasts} onClose={(id) => toasts.find(t => t.id === id)?.onClose(id)} />
      <div className="space-y-6">
        <div>
          <h1 className="text-2xl font-bold">Settings</h1>
          <p className="text-[rgb(var(--muted))]">Configure McpMux preferences.</p>
        </div>

      {/* Updates Section */}
      <UpdateChecker />

      {/* Startup & System Tray Section - always show toggles so e2e and slow backends see the section */}
      <Card data-testid="settings-startup-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Power className="h-5 w-5" />
            Startup & System Tray
          </CardTitle>
          <CardDescription>
            Control how McpMux starts and behaves with the system tray.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {loadingSettings ? (
            <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))] mb-4">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading…
            </div>
          ) : null}
          <div className="space-y-6">
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-3 flex-1 min-w-0">
                  <Power className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
                  <div>
                    <label className="text-sm font-medium">Launch at Startup</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      Start McpMux automatically when you log in to your system
                    </p>
                  </div>
                </div>
                <Switch
                  checked={startupSettings.autoLaunch}
                  onCheckedChange={(checked) => {
                    console.log('Auto-launch toggled:', checked);
                    updateStartupSetting('autoLaunch', checked);
                  }}
                  disabled={savingSettings}
                  data-testid="auto-launch-switch"
                />
              </div>

              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-3 flex-1 min-w-0">
                  <Minimize2 className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
                  <div>
                    <label className="text-sm font-medium">Start Minimized</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      Launch in background to system tray (requires auto-launch enabled)
                    </p>
                  </div>
                </div>
                <Switch
                  checked={startupSettings.startMinimized}
                  onCheckedChange={(checked) => {
                    console.log('Start minimized toggled:', checked);
                    updateStartupSetting('startMinimized', checked);
                  }}
                  disabled={savingSettings || !startupSettings.autoLaunch}
                  data-testid="start-minimized-switch"
                />
              </div>

              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-3 flex-1 min-w-0">
                  <XCircle className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
                  <div>
                    <label className="text-sm font-medium">Close to Tray</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      Keep running in system tray when window is closed (use "Quit" from tray to exit)
                    </p>
                  </div>
                </div>
                <Switch
                  checked={startupSettings.closeToTray}
                  onCheckedChange={(checked) => {
                    console.log('Close to tray toggled:', checked);
                    updateStartupSetting('closeToTray', checked);
                  }}
                  disabled={savingSettings}
                  data-testid="close-to-tray-switch"
                />
              </div>

              {savingSettings && (
                <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Saving settings...
                </div>
              )}
          </div>
        </CardContent>
      </Card>

      {/* Appearance Section */}
      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
          <CardDescription>Customize the look and feel of McpMux.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">Theme</label>
              <div className="flex gap-2 mt-2" data-testid="theme-buttons">
                <Button
                  variant={theme === 'light' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('light')}
                  data-testid="theme-light-btn"
                >
                  <Sun className="h-4 w-4 mr-2" />
                  Light
                </Button>
                <Button
                  variant={theme === 'dark' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('dark')}
                  data-testid="theme-dark-btn"
                >
                  <Moon className="h-4 w-4 mr-2" />
                  Dark
                </Button>
                <Button
                  variant={theme === 'system' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('system')}
                  data-testid="theme-system-btn"
                >
                  <Monitor className="h-4 w-4 mr-2" />
                  System
                </Button>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Analytics Section */}
      <Card data-testid="settings-analytics-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BarChart3 className="h-5 w-5" />
            Analytics
          </CardTitle>
          <CardDescription>
            Help improve McpMux by sharing anonymous usage data. No personal information is collected.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-start gap-3 flex-1 min-w-0">
              <BarChart3 className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
              <div>
                <label className="text-sm font-medium">Share Usage Data</label>
                <p className="text-xs text-[rgb(var(--muted))] mt-1">
                  Sends anonymous data like app version, OS, and feature usage to help us prioritize improvements.
                  Location is approximated from IP by PostHog. No credentials or server configurations are shared.
                </p>
              </div>
            </div>
            <Switch
              checked={analyticsEnabled}
              onCheckedChange={setAnalyticsEnabled}
              data-testid="analytics-switch"
            />
          </div>
        </CardContent>
      </Card>

      {/* Contribute & feedback — the single global "help make mcpmux
          better" card. Mirrors the items in <ContributeMenu> so power
          users have quick access without digging into GitHub. */}
      <Card data-testid="settings-contribute-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Heart className="h-5 w-5" />
            Contribute &amp; feedback
          </CardTitle>
          <CardDescription>
            mcpmux is open source. Request a server, report a bug, suggest a feature, or jump
            straight to the source.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <ContributeRow
              icon={Package}
              title="Request a new server"
              subtitle="Ask the community to add an MCP server to the registry"
              onClick={() => openExternal(CONTRIBUTE.requestServer())}
              testId="contribute-request-server"
            />
            <ContributeRow
              icon={Bug}
              title="Report a bug"
              subtitle="Something broken in the desktop app or gateway"
              onClick={() => openExternal(CONTRIBUTE.bug)}
              testId="contribute-report-bug"
            />
            <ContributeRow
              icon={Lightbulb}
              title="Suggest a feature"
              subtitle="An idea for mcpmux itself"
              onClick={() => openExternal(CONTRIBUTE.featureRequest)}
              testId="contribute-feature-request"
            />
            <ContributeRow
              icon={Github}
              title="Open on GitHub"
              subtitle="Browse source, issues, pull requests"
              onClick={() => openExternal(CONTRIBUTE.repo)}
              testId="contribute-open-github"
            />
          </div>
        </CardContent>
      </Card>

      {/* Logs Section */}
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
              <p className="text-sm text-[rgb(var(--muted))] mt-1 font-mono bg-surface-secondary rounded px-2 py-1" data-testid="logs-path">
                {logsPath || 'Loading...'}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={handleOpenLogs}
                disabled={openingLogs}
                data-testid="open-logs-btn"
              >
                {openingLogs ? (
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                ) : (
                  <FolderOpen className="h-4 w-4 mr-2" />
                )}
                Open Logs Folder
              </Button>
            </div>
            <div className="border-t border-[rgb(var(--border))] pt-4">
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-3 flex-1 min-w-0">
                  <Trash2 className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
                  <div>
                    <label className="text-sm font-medium">Auto-Cleanup</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      Automatically delete log files older than the selected period
                    </p>
                  </div>
                </div>
                <select
                  value={logRetentionDays}
                  onChange={(e) => handleRetentionChange(Number(e.target.value))}
                  disabled={savingRetention}
                  className="px-3 py-1.5 text-sm border border-[rgb(var(--border))] rounded-lg bg-[rgb(var(--surface))] text-[rgb(var(--foreground))]"
                  data-testid="log-retention-select"
                >
                  <option value={7}>7 days</option>
                  <option value={14}>14 days</option>
                  <option value={30}>30 days</option>
                  <option value={60}>60 days</option>
                  <option value={90}>90 days</option>
                  <option value={0}>Keep forever</option>
                </select>
              </div>
            </div>
            <p className="text-xs text-[rgb(var(--muted))]">
              Logs are rotated daily. Each file contains detailed debug information including thread IDs and source locations.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
    </>
  );
}

/**
 * Flat row used inside the Contribute card. Local to the Settings page — if
 * we ever need this elsewhere, promote it into @mcpmux/ui.
 */
function ContributeRow({
  icon: Icon,
  title,
  subtitle,
  onClick,
  testId,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  subtitle: string;
  onClick: () => void;
  testId?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="text-left flex items-start gap-3 p-3 rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] hover:border-primary-400/60 hover:bg-primary-500/5 transition-colors"
      data-testid={testId}
    >
      <Icon className="h-4 w-4 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
      <div className="min-w-0">
        <p className="text-sm font-medium">{title}</p>
        <p className="text-[11px] text-[rgb(var(--muted))] leading-snug mt-0.5">{subtitle}</p>
      </div>
    </button>
  );
}
