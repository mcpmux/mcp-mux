import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
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
  useConfirm,
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
  Sparkles,
  Github,
  Bug,
  Lightbulb,
  Package,
  Heart,
  Network,
  Globe,
  RotateCcw,
  AlertCircle,
  ShieldOff,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import {
  useAppStore,
  useTheme,
  useAnalyticsEnabled,
  usePendingSettingsSection,
  useSetPendingSettingsSection,
} from '@/stores';
import { UpdateChecker } from './UpdateChecker';
import { AboutSection } from './AboutSection';
import { ServerUpdatesSection } from './ServerUpdatesSection';
import { getMetaToolsEnabled, setMetaToolsEnabled } from '@/lib/api/metaTools';
import {
  getAdminWebSettings,
  getGatewayPortSettings,
  getLogsPath,
  getStartupSettings,
  openLogsFolder,
  resetGatewayPort,
  setGatewayPort,
  updateAdminWebSettings,
  updateStartupSettings,
  type AdminWebSettings,
  type GatewayPortSettings,
  type StartupSettings,
} from '@/lib/api/settings';
import { getLogRetentionDays, setLogRetentionDays as saveLogRetentionDays } from '@/lib/api/logs';
import { MetaToolAuditLog, MetaToolGrantsPanel } from '@/features/metaTools';
import { useGatewayControl } from '@/features/gateway/useGatewayControl';
import { CONTRIBUTE, openExternal } from '@/lib/contribute';
import { VIEWER_IDENTITY_CHANGED, useViewerIdentity } from '@/hooks/use-viewer-identity.hook';
import {
  getMissingMachineProfileField,
  isMachineProfileComplete,
  toMachineProfilePayload,
} from '@/lib/machine-profile.helpers';
import { isViewingLocally } from '@/lib/viewer-device.helpers';
import { MachineProfileEditor } from '@/components/machine-profile-editor';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';
import { isTauri } from '@/lib/api/transport';
import {
  createMachine,
  deleteMachine,
  getHostname,
  getLocalMachineId,
  listMachines,
  setLocalMachineId,
  updateMachine,
  type Machine,
} from '@/lib/api/machines';
import {
  listWorkspaceBindings,
  deleteWorkspaceBinding,
} from '@/lib/api/workspaceBindings';

interface GatewayPublicUrlSettings {
  configuredPublicBaseUrl: string | null;
  activePublicBaseUrl: string | null;
  localBaseUrl: string | null;
}

export function SettingsPage() {
  const { t } = useTranslation(['settings', 'common']);
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const analyticsEnabled = useAnalyticsEnabled();
  const setAnalyticsEnabled = useAppStore((state) => state.setAnalyticsEnabled);
  const [logsPath, setLogsPath] = useState<string>('');
  const [openingLogs, setOpeningLogs] = useState(false);
  const { toasts, success, error, info } = useToast();
  const { confirm, ConfirmDialogElement: machineConfirmDialog } = useConfirm();
  const gatewayControl = useGatewayControl();

  // Deep-link: when another surface routes here for a specific section, scroll
  // it into view and briefly flash it so the user lands on the right control.
  // Generic over section keys — any surface can target a section by calling
  // `setPendingSettingsSection('<key>')` before `navigateTo('settings')`. A
  // section becomes targetable by wrapping its card with `registerSection` +
  // `sectionFlashClass` (see `<SECTION_KEYS>` below).
  const pendingSection = usePendingSettingsSection();
  const clearPendingSection = useSetPendingSettingsSection();
  const sectionEls = useRef<Record<string, HTMLDivElement | null>>({});
  const [flashedSection, setFlashedSection] = useState<string | null>(null);

  const registerSection = (key: string) => (el: HTMLDivElement | null) => {
    sectionEls.current[key] = el;
  };
  const sectionFlashClass = (key: string) =>
    flashedSection === key
      ? 'rounded-xl ring-2 ring-primary-500 ring-offset-2 ring-offset-[rgb(var(--background))] transition-shadow duration-500'
      : 'rounded-xl ring-0 transition-shadow duration-500';

  useEffect(() => {
    if (!pendingSection) return;
    const el = sectionEls.current[pendingSection];
    // Unknown or not-yet-mounted section: drop the request so a stale value
    // doesn't fire the flash on a later, unrelated render.
    if (!el) {
      clearPendingSection(null);
      return;
    }
    el.scrollIntoView({ behavior: 'smooth', block: 'center' });
    setFlashedSection(pendingSection);
    clearPendingSection(null);
    const t = setTimeout(() => setFlashedSection(null), 2200);
    return () => clearTimeout(t);
  }, [pendingSection, clearPendingSection]);

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

  // Workspace mapping prompt — pops the "map this folder?" sheet when a client
  // opens an unmapped folder. On by default.
  const [mappingPromptEnabled, setMappingPromptEnabled] = useState(true);
  const [savingMappingPrompt, setSavingMappingPrompt] = useState(false);

  // System-wide inbound auth toggle. When disabled, local apps connect to the
  // gateway with no access key — used by the one-click per-workspace install.
  const [authDisabled, setAuthDisabled] = useState(false);
  const [savingAuthDisabled, setSavingAuthDisabled] = useState(false);
  const [networkAccess, setNetworkAccess] = useState(false);
  const [savingNetworkAccess, setSavingNetworkAccess] = useState(false);

  // Meta-tools master switch — gates the entire `mcpmux_*` namespace.
  const [metaToolsEnabled, setMetaToolsEnabledState] = useState<boolean>(true);
  const [loadingMetaTools, setLoadingMetaTools] = useState(true);

  // Gateway port — persisted user override, the default the app ships
  // with, and the port the currently-running gateway is bound to. When
  // saved ≠ active, the user has to restart the gateway to apply.
  const [portSettings, setPortSettings] = useState<GatewayPortSettings | null>(null);
  const [portDraft, setPortDraft] = useState<string>('');
  const [portError, setPortError] = useState<string | null>(null);
  const [savingPort, setSavingPort] = useState(false);
  const [resettingPort, setResettingPort] = useState(false);

  // Public base URL — optional external HTTPS origin advertised to remote MCP
  // clients through OAuth metadata. Leave blank for local-only localhost mode.
  const [publicUrlSettings, setPublicUrlSettings] = useState<GatewayPublicUrlSettings | null>(null);
  const [publicUrlDraft, setPublicUrlDraft] = useState<string>('');
  const [publicUrlError, setPublicUrlError] = useState<string | null>(null);
  const [savingPublicUrl, setSavingPublicUrl] = useState(false);
  const [resettingPublicUrl, setResettingPublicUrl] = useState(false);

  const [adminWeb, setAdminWeb] = useState<AdminWebSettings | null>(null);
  const [adminPortDraft, setAdminPortDraft] = useState('45819');
  const [adminCfDomainDraft, setAdminCfDomainDraft] = useState('');
  const [loadingAdminWeb, setLoadingAdminWeb] = useState(true);
  const [savingAdminWeb, setSavingAdminWeb] = useState(false);

  const loadPortSettings = async () => {
    try {
      const s = await getGatewayPortSettings();
      setPortSettings(s);
      setPortDraft(String(s.configuredPort ?? s.defaultPort));
      setPublicUrlDraft(s.publicUrl ?? '');
      setPortError(null);
      setPublicUrlError(null);
    } catch (err) {
      console.error('Failed to load gateway port settings:', err);
    }
  };

  const loadPublicUrlSettings = async () => {
    try {
      const s = await invoke<GatewayPublicUrlSettings>('get_gateway_public_url_settings');
      setPublicUrlSettings(s);
      setPublicUrlDraft(s.configuredPublicBaseUrl ?? '');
      setPublicUrlError(null);
    } catch (err) {
      console.error('Failed to load gateway public URL settings:', err);
    }
  };

  useEffect(() => {
    loadPortSettings();
    loadPublicUrlSettings();
  }, []);

  const loadAdminWebSettings = async () => {
    try {
      const s = await getAdminWebSettings();
      setAdminWeb(s);
      setAdminPortDraft(String(s.port));
      setAdminCfDomainDraft(s.cfTeamDomain);
    } catch (err) {
      console.error('Failed to load web admin settings:', err);
    } finally {
      setLoadingAdminWeb(false);
    }
  };

  useEffect(() => {
    if (!isTauri()) {
      setLoadingAdminWeb(false);
      return;
    }
    loadAdminWebSettings();
  }, []);

  const persistAdminWeb = async (next: AdminWebSettings) => {
    setSavingAdminWeb(true);
    try {
      await updateAdminWebSettings(next);
      setAdminWeb(next);
      setAdminPortDraft(String(next.port));
      setAdminCfDomainDraft(next.cfTeamDomain);
      success(
        t('toast.webAdminUpdated'),
        next.enabled
          ? t('toast.webAdminEnabled', { port: next.port })
          : t('toast.webAdminStopped')
      );
    } catch (err) {
      error(t('toast.failedWebAdmin'), String(err));
    } finally {
      setSavingAdminWeb(false);
    }
  };

  const handleSaveAdminPort = async () => {
    if (!adminWeb) return;
    const parsed = validatePort(adminPortDraft);
    if ('error' in parsed) {
      error(t('toast.invalidAdminPort'), parsed.error);
      return;
    }
    await persistAdminWeb({ ...adminWeb, port: parsed.port });
  };

  const validatePort = (raw: string): { port: number } | { error: string } => {
    const trimmed = raw.trim();
    if (!trimmed) return { error: t('validation.enterPort') };
    if (!/^\d+$/.test(trimmed)) return { error: t('validation.portMustBeNumber') };
    const n = Number(trimmed);
    if (n < 1024 || n > 65535) {
      return { error: t('validation.portRange') };
    }
    return { port: n };
  };

  const validatePublicBaseUrl = (
    raw: string
  ): { publicBaseUrl: string | null } | { error: string } => {
    const trimmed = raw.trim();
    if (!trimmed) return { publicBaseUrl: null };

    let url: URL;
    try {
      url = new URL(trimmed);
    } catch {
      return { error: 'Enter a valid HTTPS origin, for example https://mcp.example.com' };
    }

    if (url.protocol !== 'https:') return { error: 'Public base URL must start with https://' };
    if (url.username || url.password)
      return { error: 'Public base URL must not include credentials' };
    if (url.search || url.hash)
      return { error: 'Public base URL must not include a query string or fragment' };
    if (url.pathname !== '/') {
      return { error: 'Use the origin only, for example https://mcp.example.com, not a /mcp path' };
    }

    return { publicBaseUrl: url.origin };
  };

  const handleSavePort = async () => {
    const parsed = validatePort(portDraft);
    if ('error' in parsed) {
      setPortError(parsed.error);
      return;
    }
    setPortError(null);
    setSavingPort(true);
    try {
      await setGatewayPort(parsed.port);
      await loadPortSettings();
      success(
        t('toast.gatewayPortSaved'),
        portSettings?.activePort && portSettings.activePort !== parsed.port
          ? t('toast.gatewayRestartForPort', { port: parsed.port })
          : t('toast.gatewayNextStartPort', { port: parsed.port })
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setPortError(msg);
      error(t('toast.failedSavePort'), msg);
    } finally {
      setSavingPort(false);
    }
  };

  const handleResetPort = async () => {
    setResettingPort(true);
    try {
      await resetGatewayPort();
      await loadPortSettings();
      success(
        t('toast.resetToDefault'),
        portSettings && portSettings.activePort !== portSettings.defaultPort
          ? t('toast.gatewayRestartForPort', { port: portSettings.defaultPort })
          : t('toast.gatewayNextStartPort', { port: portSettings?.defaultPort ?? '' })
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      error(t('toast.failedResetPort'), msg);
    } finally {
      setResettingPort(false);
    }
  };

  const handleSavePublicUrl = async () => {
    const parsed = validatePublicBaseUrl(publicUrlDraft);
    if ('error' in parsed) {
      setPublicUrlError(parsed.error);
      return;
    }

    setPublicUrlError(null);
    setSavingPublicUrl(true);
    try {
      await invoke('set_gateway_public_base_url', { publicBaseUrl: parsed.publicBaseUrl });
      const activeAdvertised = publicUrlSettings?.activePublicBaseUrl ?? null;
      const desiredAdvertised = parsed.publicBaseUrl ?? publicUrlSettings?.localBaseUrl ?? null;
      await loadPublicUrlSettings();
      success(
        parsed.publicBaseUrl ? 'Public URL saved' : 'Public URL cleared',
        activeAdvertised && desiredAdvertised && activeAdvertised !== desiredAdvertised
          ? 'Restart the gateway for OAuth metadata to advertise the new URL.'
          : parsed.publicBaseUrl
            ? `Remote clients should use ${parsed.publicBaseUrl}/mcp.`
            : 'Next gateway start will advertise the local localhost URL.'
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setPublicUrlError(msg);
      error('Failed to save public URL', msg);
    } finally {
      setSavingPublicUrl(false);
    }
  };

  const handleResetPublicUrl = async () => {
    setResettingPublicUrl(true);
    try {
      await invoke('reset_gateway_public_base_url');
      await loadPublicUrlSettings();
      success('Public URL cleared', 'Restart the gateway to return OAuth metadata to localhost.');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      error('Failed to clear public URL', msg);
    } finally {
      setResettingPublicUrl(false);
    }
  };

  const handleRestartGateway = async () => {
    try {
      const outcome = await gatewayControl.restart();
      await loadPortSettings();
      await loadPublicUrlSettings();
      if (outcome.status === 'cancelled') return;
      success(
        t('toast.gatewayRestarted'),
        outcome.fellBackToDynamic
          ? t('toast.gatewayFellBack', { port: outcome.port })
          : t('toast.gatewayPortActive')
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      error(t('toast.failedRestartGateway'), msg);
    }
  };

  useEffect(() => {
    getMetaToolsEnabled()
      .then((v) => setMetaToolsEnabledState(v))
      .catch((e) => console.error('Failed to load meta_tools_enabled', e))
      .finally(() => setLoadingMetaTools(false));
  }, []);

  const handleToggleMetaTools = async (next: boolean) => {
    const previous = metaToolsEnabled;
    setMetaToolsEnabledState(next);
    try {
      await setMetaToolsEnabled(next);
      success(
        next ? t('toast.metaToolsEnabled') : t('toast.metaToolsDisabled'),
        next ? t('toast.metaToolsEnabledDesc') : t('toast.metaToolsDisabledDesc')
      );
    } catch (e) {
      setMetaToolsEnabledState(previous);
      error(t('toast.failedToSaveSetting'), e instanceof Error ? e.message : String(e));
    }
  };

  // Load logs path on mount
  useEffect(() => {
    const loadLogsPath = async () => {
      try {
        const path = await getLogsPath();
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
        const days = await getLogRetentionDays();
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
        const settings = await getStartupSettings();
        setStartupSettings(settings);
      } catch (error) {
        console.error('Failed to load startup settings:', error);
      } finally {
        setLoadingSettings(false);
      }
    };
    loadStartupSettings();
  }, []);

  // Load workspace mapping-prompt setting on mount.
  useEffect(() => {
    invoke<boolean>('get_workspace_mapping_prompt_enabled')
      .then(setMappingPromptEnabled)
      .catch((err) => console.error('Failed to load mapping prompt setting:', err));
  }, []);

  const updateMappingPrompt = async (enabled: boolean) => {
    const prev = mappingPromptEnabled;
    setMappingPromptEnabled(enabled);
    setSavingMappingPrompt(true);
    try {
      await invoke('set_workspace_mapping_prompt_enabled', { enabled });
      success(
        t('toast.settingsSaved'),
        enabled ? t('toast.mappingPromptOnHint') : t('toast.mappingPromptOffHint')
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Unknown error';
      error(t('toast.failedToSaveSetting'), msg);
      setMappingPromptEnabled(prev);
    } finally {
      setSavingMappingPrompt(false);
    }
  };

  // Load the system-wide inbound-auth toggle on mount.
  useEffect(() => {
    invoke<boolean>('get_gateway_auth_disabled')
      .then(setAuthDisabled)
      .catch((err) => console.error('Failed to load auth setting:', err));
  }, []);

  // Load the network-access (0.0.0.0 bind) toggle on mount.
  useEffect(() => {
    invoke<boolean>('get_gateway_network_access')
      .then(setNetworkAccess)
      .catch((err) => console.error('Failed to load network-access setting:', err));
  }, []);

  const updateNetworkAccess = async (enabled: boolean) => {
    const prev = networkAccess;
    setNetworkAccess(enabled);
    setSavingNetworkAccess(true);
    try {
      await invoke('set_gateway_network_access', { enabled });
      success(
        t('toast.settingsSaved'),
        enabled ? t('toast.networkAccessOnHint') : t('toast.networkAccessOffHint')
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Unknown error';
      error(t('toast.failedUpdateNetworkAccess'), msg);
      setNetworkAccess(prev);
    } finally {
      setSavingNetworkAccess(false);
    }
  };

  const updateAuthDisabled = async (disabled: boolean) => {
    const prev = authDisabled;
    setAuthDisabled(disabled);
    setSavingAuthDisabled(true);
    try {
      await invoke('set_gateway_auth_disabled', { disabled });
      success(
        t('toast.settingsSaved'),
        disabled ? t('toast.authDisabledHint') : t('toast.authRequiredHint')
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Unknown error';
      error(t('toast.failedToSaveSetting'), msg);
      setAuthDisabled(prev);
    } finally {
      setSavingAuthDisabled(false);
    }
  };

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
      await updateStartupSettings(newSettings);
      console.log('[Settings] Successfully saved:', newSettings);
      
      // Show success toast
      success(t('toast.settingsSaved'), t('toast.preferencesUpdated'));
    } catch (err) {
      console.error('[Settings] Failed to save:', err);
      // Show error toast
      const errorMessage = err instanceof Error ? err.message : 'Unknown error';
      error(t('toast.failedToSaveSettings'), errorMessage);
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
      await saveLogRetentionDays(days);
      success(
        t('toast.settingsSaved'),
        t('toast.logRetention', {
          value: days === 0 ? t('toast.keepForever') : t('toast.days', { count: days }),
        })
      );
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Unknown error';
      error(t('toast.failedToSaveSetting'), errorMessage);
      setLogRetentionDays(oldDays);
    } finally {
      setSavingRetention(false);
    }
  };

  const handleOpenLogs = async () => {
    setOpeningLogs(true);
    try {
      await openLogsFolder();
    } catch (error) {
      console.error('Failed to open logs folder:', error);
    } finally {
      setOpeningLogs(false);
    }
  };

  return (
    <>
      <ToastContainer toasts={toasts} onClose={(id) => toasts.find(t => t.id === id)?.onClose(id)} />
      {gatewayControl.ConfirmDialogElement}
      {machineConfirmDialog}
      <div className="space-y-6" data-testid="settings-page">
        <div>
          <h1 className="text-2xl font-bold" data-testid="settings-title">
            {t('title')}
          </h1>
          <p className="text-[rgb(var(--muted))]">{t('subtitle')}</p>
        </div>

        {/* Updates / About — desktop shows updater; web-admin shows build info */}
        <div ref={registerSection('updates')} className={sectionFlashClass('updates')}>
          {isTauri() ? <UpdateChecker /> : <AboutSection />}
        </div>

      <ServerUpdatesSection onSuccess={success} onError={error} onInfo={info} />

      {/* Startup & System Tray Section - always show toggles so e2e and slow backends see the section */}
      <Card data-testid="settings-startup-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Power className="h-5 w-5" />
            {t('startup.title')}
          </CardTitle>
          <CardDescription>{t('startup.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          {loadingSettings ? (
            <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))] mb-4">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t('loading')}
            </div>
          ) : null}
          <div className="space-y-6">
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-3 flex-1 min-w-0">
                  <Power className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
                  <div>
                    <label className="text-sm font-medium">{t('startup.autoLaunch')}</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      {t('startup.autoLaunchDesc')}
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
                    <label className="text-sm font-medium">{t('startup.startMinimized')}</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      {t('startup.startMinimizedDesc')}
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
                    <label className="text-sm font-medium">{t('startup.closeToTray')}</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      {t('startup.closeToTrayDesc')}
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
                  {t('savingSettings')}
                </div>
              )}
          </div>
        </CardContent>
      </Card>

      {/* Gateway Section — port override + reset to default */}
      <div ref={registerSection('gateway')} className={sectionFlashClass('gateway')}>
        <Card data-testid="settings-gateway-section">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Network className="h-5 w-5" />
              Gateway
            </CardTitle>
            <CardDescription>
              The local port every AI client connects to. Changing it takes effect on the next
              gateway start — existing IDE configs pointing at the old port will need updating.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {portSettings === null ? (
              <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
                <Loader2 className="h-4 w-4 animate-spin" />
                Loading…
              </div>
            ) : (
              <div className="space-y-4">
                <div className="flex items-start gap-3">
                  <Network className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
                  <div className="min-w-0 flex-1">
                    <label htmlFor="gateway-port-input" className="text-sm font-medium">
                      Gateway port
                    </label>
                    <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                      Default is <span className="font-mono">{portSettings.defaultPort}</span>.
                      Use a port between 1024 and 65535.
                      {portSettings.activePort !== null ? (
                        <>
                          {' '}
                          Currently running on{' '}
                          <span className="font-mono" data-testid="gateway-active-port">
                            :{portSettings.activePort}
                          </span>
                          .
                        </>
                      ) : (
                        ' Gateway is stopped.'
                      )}
                    </p>
                    <div className="mt-3 flex flex-wrap items-center gap-2">
                      <input
                        id="gateway-port-input"
                        type="number"
                        inputMode="numeric"
                        min={1024}
                        max={65535}
                        value={portDraft}
                        onChange={(e) => {
                          setPortDraft(e.target.value);
                          if (portError) setPortError(null);
                        }}
                        disabled={savingPort || resettingPort}
                        className="focus:ring-primary-500/40 w-28 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 font-mono text-sm text-[rgb(var(--foreground))] focus:outline-none focus:ring-2"
                        data-testid="gateway-port-input"
                      />
                      <Button
                        variant="primary"
                        size="sm"
                        onClick={handleSavePort}
                        disabled={
                          savingPort ||
                          resettingPort ||
                          portDraft.trim() ===
                            String(portSettings.configuredPort ?? portSettings.defaultPort)
                        }
                        data-testid="gateway-port-save-btn"
                      >
                        {savingPort ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
                        Save
                      </Button>
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={handleResetPort}
                        disabled={
                          savingPort || resettingPort || portSettings.configuredPort === null
                        }
                        data-testid="gateway-port-reset-btn"
                        title={
                          portSettings.configuredPort === null
                            ? 'Already using the default port'
                            : `Reset to ${portSettings.defaultPort}`
                        }
                      >
                        {resettingPort ? (
                          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                        ) : (
                          <RotateCcw className="mr-2 h-4 w-4" />
                        )}
                        Reset to default
                      </Button>
                    </div>
                    {portError ? (
                      <p
                        className="mt-2 text-xs text-red-600 dark:text-red-400"
                        data-testid="gateway-port-error"
                      >
                        {portError}
                      </p>
                    ) : null}
                  </div>
                </div>

                <div className="border-t border-[rgb(var(--border-subtle))] pt-4">
                  <div className="flex items-start gap-3">
                    <Globe className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
                    <div className="min-w-0 flex-1">
                      <label htmlFor="gateway-public-url-input" className="text-sm font-medium">
                        Public base URL
                      </label>
                      <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                        Optional HTTPS origin to advertise in OAuth metadata when the gateway sits
                        behind a public tunnel. Use{' '}
                        <span className="font-mono">https://mcp.example.com</span>, not{' '}
                        <span className="font-mono">/mcp</span>. Leave blank for local-only mode.
                        {publicUrlSettings?.activePublicBaseUrl ? (
                          <>
                            {' '}
                            Currently advertising{' '}
                            <span className="font-mono" data-testid="gateway-active-public-url">
                              {publicUrlSettings.activePublicBaseUrl}
                            </span>
                            .
                          </>
                        ) : null}
                      </p>
                      <div className="mt-3 flex flex-wrap items-center gap-2">
                        <input
                          id="gateway-public-url-input"
                          type="url"
                          inputMode="url"
                          placeholder="https://mcp.example.com"
                          value={publicUrlDraft}
                          onChange={(e) => {
                            setPublicUrlDraft(e.target.value);
                            if (publicUrlError) setPublicUrlError(null);
                          }}
                          disabled={savingPublicUrl || resettingPublicUrl}
                          className="focus:ring-primary-500/40 min-w-[280px] flex-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 font-mono text-sm text-[rgb(var(--foreground))] focus:outline-none focus:ring-2"
                          data-testid="gateway-public-url-input"
                        />
                        <Button
                          variant="primary"
                          size="sm"
                          onClick={handleSavePublicUrl}
                          disabled={
                            savingPublicUrl ||
                            resettingPublicUrl ||
                            publicUrlDraft.trim().replace(/\/+$/, '') ===
                              (publicUrlSettings?.configuredPublicBaseUrl ?? '')
                          }
                          data-testid="gateway-public-url-save-btn"
                        >
                          {savingPublicUrl ? (
                            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                          ) : null}
                          Save
                        </Button>
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={handleResetPublicUrl}
                          disabled={
                            savingPublicUrl ||
                            resettingPublicUrl ||
                            publicUrlSettings?.configuredPublicBaseUrl === null
                          }
                          data-testid="gateway-public-url-reset-btn"
                        >
                          {resettingPublicUrl ? (
                            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                          ) : (
                            <RotateCcw className="mr-2 h-4 w-4" />
                          )}
                          Clear
                        </Button>
                      </div>
                      {publicUrlError ? (
                        <p
                          className="mt-2 text-xs text-red-600 dark:text-red-400"
                          data-testid="gateway-public-url-error"
                        >
                          {publicUrlError}
                        </p>
                      ) : null}
                    </div>
                  </div>
                </div>

                <div className="border-t border-[rgb(var(--border-subtle))] pt-4">
                  <div className="flex items-center justify-between gap-4">
                    <div className="flex min-w-0 flex-1 items-start gap-3">
                      <Network className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
                      <div className="min-w-0">
                        <label className="text-sm font-medium">
                          Allow access from other devices
                        </label>
                        <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                          Bind the gateway to all network interfaces (
                          <span className="font-mono">0.0.0.0</span>) so other machines on your
                          network can connect to the same MCP servers. Off keeps it on{' '}
                          <span className="font-mono">127.0.0.1</span> (this machine only).
                          Restart the gateway to apply.
                        </p>
                      </div>
                    </div>
                    <Switch
                      checked={networkAccess}
                      onCheckedChange={updateNetworkAccess}
                      disabled={savingNetworkAccess}
                      data-testid="network-access-switch"
                    />
                  </div>

                  {networkAccess ? (
                    <div
                      className={`mt-3 flex items-start gap-2 rounded-lg border p-3 text-xs ${
                        authDisabled
                          ? 'border-red-300 bg-red-50 dark:border-red-700/60 dark:bg-red-900/20'
                          : 'border-amber-300 bg-amber-50 dark:border-amber-700/60 dark:bg-amber-900/20'
                      }`}
                      data-testid="network-access-warning"
                    >
                      <AlertCircle
                        className={`mt-0.5 h-4 w-4 flex-shrink-0 ${
                          authDisabled
                            ? 'text-red-600 dark:text-red-400'
                            : 'text-amber-600 dark:text-amber-400'
                        }`}
                      />
                      <div className="flex-1">
                        {authDisabled ? (
                          <>
                            <p className="font-semibold text-red-800 dark:text-red-200">
                              Exposed without authentication
                            </p>
                            <p className="mt-0.5 text-red-700 dark:text-red-300">
                              Authentication is off and the gateway is reachable on your network —
                              anyone who can reach this machine can use every connected MCP server
                              and its stored credentials. Turn authentication back on under
                              Security, or only enable this on a network you trust.
                            </p>
                          </>
                        ) : (
                          <>
                            <p className="font-semibold text-amber-800 dark:text-amber-200">
                              Reachable on your network
                            </p>
                            <p className="mt-0.5 text-amber-700 dark:text-amber-300">
                              Connecting clients still need to be approved, but traffic is plain
                              HTTP — only enable this on a network you trust. From another device,
                              replace <span className="font-mono">localhost</span> with this
                              machine's LAN IP, e.g.{' '}
                              <span className="font-mono">
                                http://192.168.1.x:
                                {portSettings.activePort ?? portSettings.defaultPort}/mcp
                              </span>
                              .
                            </p>
                            <p className="mt-1 text-amber-700 dark:text-amber-300">
                              Per-client OAuth approval happens on this machine, so a remote
                              client that signs in via OAuth (e.g. ChatGPT) can't finish approval
                              over the network yet — front the gateway with the public URL + a
                              tunnel for that. For plain LAN sharing, pair this with
                              authentication disabled.
                            </p>
                          </>
                        )}
                      </div>
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={handleRestartGateway}
                        data-testid="network-access-restart-btn"
                      >
                        Restart gateway
                      </Button>
                    </div>
                  ) : null}
                </div>

                {publicUrlSettings?.activePublicBaseUrl &&
                (publicUrlSettings.configuredPublicBaseUrl ?? publicUrlSettings.localBaseUrl) &&
                publicUrlSettings.activePublicBaseUrl !==
                  (publicUrlSettings.configuredPublicBaseUrl ??
                    publicUrlSettings.localBaseUrl) ? (
                  <div
                    className="flex items-start gap-2 rounded-lg border border-amber-300 bg-amber-50 p-3 text-xs dark:border-amber-700/60 dark:bg-amber-900/20"
                    data-testid="gateway-public-url-restart-hint"
                  >
                    <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
                    <div className="flex-1">
                      <p className="font-semibold text-amber-800 dark:text-amber-200">
                        Restart required
                      </p>
                      <p className="mt-0.5 text-amber-700 dark:text-amber-300">
                        The saved public URL does not match the URL currently advertised in OAuth
                        metadata. Restart the gateway before reconnecting ChatGPT.
                      </p>
                    </div>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={handleRestartGateway}
                      data-testid="gateway-public-url-restart-btn"
                    >
                      Restart gateway
                    </Button>
                  </div>
                ) : null}

                {portSettings.activePort !== null &&
                portSettings.configuredPort !== null &&
                portSettings.configuredPort !== portSettings.activePort ? (
                  <div
                    className="flex items-start gap-2 rounded-lg border border-amber-300 bg-amber-50 p-3 text-xs dark:border-amber-700/60 dark:bg-amber-900/20"
                    data-testid="gateway-port-restart-hint"
                  >
                    <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
                    <div className="flex-1">
                      <p className="font-semibold text-amber-800 dark:text-amber-200">
                        Restart required
                      </p>
                      <p className="mt-0.5 text-amber-700 dark:text-amber-300">
                        Saved port{' '}
                        <span className="font-mono">:{portSettings.configuredPort}</span> doesn't
                        match the running port{' '}
                        <span className="font-mono">:{portSettings.activePort}</span>. Restart the
                        gateway to apply — your IDE configs will need to point at the new URL.
                      </p>
                    </div>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={handleRestartGateway}
                      data-testid="gateway-restart-btn"
                    >
                      Restart gateway
                    </Button>
                  </div>
                ) : null}

                {isTauri() ? (
                  <div
                    className="mt-2 border-t border-[rgb(var(--border))] pt-4"
                    data-testid="settings-web-admin-section"
                  >
                    <div className="flex items-start gap-3">
                      <Globe className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
                      <div className="min-w-0 flex-1 space-y-4">
                        <div>
                          <p className="text-sm font-medium">{t('gateway.webAdminTitle')}</p>
                          <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                            {t('gateway.webAdminDesc', { port: adminPortDraft })}
                          </p>
                        </div>

                        {loadingAdminWeb || adminWeb === null ? (
                          <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
                            <Loader2 className="h-4 w-4 animate-spin" />
                            {t('loading')}
                          </div>
                        ) : (
                          <>
                            <div className="flex items-center justify-between gap-4">
                              <div>
                                <p className="text-sm font-medium">
                                  {t('gateway.enableWebAdmin')}
                                </p>
                                <p className="text-xs text-[rgb(var(--muted))]">
                                  {t('gateway.enableWebAdminDesc')}
                                </p>
                              </div>
                              <Switch
                                checked={adminWeb.enabled}
                                disabled={savingAdminWeb}
                                onCheckedChange={(enabled) =>
                                  persistAdminWeb({ ...adminWeb, enabled })
                                }
                                data-testid="settings-admin-enabled-switch"
                              />
                            </div>

                            <div>
                              <label htmlFor="admin-port-input" className="text-sm font-medium">
                                {t('gateway.adminPort')}
                              </label>
                              <div className="mt-2 flex flex-wrap items-center gap-2">
                                <input
                                  id="admin-port-input"
                                  type="number"
                                  inputMode="numeric"
                                  min={1024}
                                  max={65535}
                                  value={adminPortDraft}
                                  onChange={(e) => setAdminPortDraft(e.target.value)}
                                  disabled={savingAdminWeb || !adminWeb.enabled}
                                  className="w-28 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 font-mono text-sm"
                                  data-testid="settings-admin-port-input"
                                />
                                <Button
                                  variant="primary"
                                  size="sm"
                                  onClick={handleSaveAdminPort}
                                  disabled={
                                    savingAdminWeb ||
                                    !adminWeb.enabled ||
                                    adminPortDraft.trim() === String(adminWeb.port)
                                  }
                                  data-testid="settings-admin-port-save-btn"
                                >
                                  {t('gateway.savePort')}
                                </Button>
                              </div>
                            </div>

                            <div className="flex items-center justify-between gap-4">
                              <div>
                                <p className="text-sm font-medium">
                                  {t('gateway.trustCfAccess')}
                                </p>
                                <p className="text-xs text-[rgb(var(--muted))]">
                                  {t('gateway.trustCfAccessDesc')}
                                </p>
                              </div>
                              <Switch
                                checked={adminWeb.trustCfAccess}
                                disabled={savingAdminWeb || !adminWeb.enabled}
                                onCheckedChange={(trustCfAccess) =>
                                  persistAdminWeb({ ...adminWeb, trustCfAccess })
                                }
                                data-testid="settings-admin-cf-access-switch"
                              />
                            </div>

                            {adminWeb.enabled ? (
                              <div>
                                <label
                                  htmlFor="admin-cf-domain-input"
                                  className="text-sm font-medium"
                                >
                                  {t('gateway.cfTeamDomain')}
                                </label>
                                <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                                  {t('gateway.cfTeamDomainDesc')}
                                </p>
                                <div className="mt-2 flex flex-wrap items-center gap-2">
                                  <input
                                    id="admin-cf-domain-input"
                                    type="text"
                                    placeholder={t('gateway.cfTeamDomainPlaceholder')}
                                    value={adminCfDomainDraft}
                                    onChange={(e) => setAdminCfDomainDraft(e.target.value)}
                                    disabled={savingAdminWeb || !adminWeb.enabled}
                                    className="min-w-[12rem] flex-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1.5 text-sm"
                                    data-testid="settings-admin-cf-domain-input"
                                  />
                                  <Button
                                    variant="primary"
                                    size="sm"
                                    onClick={() =>
                                      persistAdminWeb({
                                        ...adminWeb,
                                        cfTeamDomain: adminCfDomainDraft.trim(),
                                      })
                                    }
                                    disabled={
                                      savingAdminWeb ||
                                      !adminWeb.enabled ||
                                      adminCfDomainDraft.trim() === adminWeb.cfTeamDomain
                                    }
                                  >
                                    {t('gateway.saveDomain')}
                                  </Button>
                                </div>
                              </div>
                            ) : null}
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                ) : null}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Workspaces Section */}
      <div ref={registerSection('workspaces')} className={sectionFlashClass('workspaces')}>
        <Card data-testid="settings-workspaces-section">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <FolderOpen className="h-5 w-5" />
              Workspaces
            </CardTitle>
            <CardDescription>
              How McpMux handles folders your connected apps open.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-center justify-between gap-4">
              <div className="flex min-w-0 flex-1 items-start gap-3">
                <FolderOpen className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
                <div>
                  <label className="text-sm font-medium">Ask to map new folders</label>
                  <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                    When a connected app opens a folder you haven't mapped, show a prompt to give
                    it a specific feature set. The folder already works with your default Starter
                    set either way.
                  </p>
                </div>
              </div>
              <Switch
                checked={mappingPromptEnabled}
                onCheckedChange={updateMappingPrompt}
                disabled={savingMappingPrompt}
                data-testid="workspace-mapping-prompt-switch"
              />
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Security Section */}
      <div
        ref={registerSection('security')}
        id="settings-security"
        className={sectionFlashClass('security')}
      >
        <Card data-testid="settings-security-section">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <ShieldOff className="h-5 w-5" />
              Security
            </CardTitle>
            <CardDescription>
              How McpMux authenticates apps connecting to the local gateway.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-center justify-between gap-4">
              <div className="flex min-w-0 flex-1 items-start gap-3">
                <ShieldOff className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
                <div>
                  <label className="text-sm font-medium">Disable authentication</label>
                  <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                    Let local apps connect with no access key — just the URL and a workspace
                    header. Quickest setup, but any app on this machine can then reach the
                    gateway.
                  </p>
                </div>
              </div>
              <Switch
                checked={authDisabled}
                onCheckedChange={updateAuthDisabled}
                disabled={savingAuthDisabled}
                data-testid="disable-auth-switch"
              />
            </div>
          </CardContent>
        </Card>
      </div>

      <MachineIdentitySection
        onSuccess={success}
        onError={error}
        confirm={confirm}
        t={t}
      />

      {/* Appearance Section */}
      <Card data-testid="settings-appearance-section">
        <CardHeader>
          <CardTitle>{t('appearance.title')}</CardTitle>
          <CardDescription>{t('appearance.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">{t('appearance.theme')}</label>
              <div className="flex gap-2 mt-2" data-testid="theme-buttons">
                <Button
                  variant={theme === 'light' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('light')}
                  data-testid="theme-light-btn"
                >
                  <Sun className="h-4 w-4 mr-2" />
                  {t('appearance.light')}
                </Button>
                <Button
                  variant={theme === 'dark' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('dark')}
                  data-testid="theme-dark-btn"
                >
                  <Moon className="h-4 w-4 mr-2" />
                  {t('appearance.dark')}
                </Button>
                <Button
                  variant={theme === 'system' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('system')}
                  data-testid="theme-system-btn"
                >
                  <Monitor className="h-4 w-4 mr-2" />
                  {t('appearance.system')}
                </Button>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Self-management meta tools — `mcpmux_*` namespace */}
      <Card data-testid="settings-meta-tools-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Sparkles className="h-5 w-5" />
            {t('metaTools.title')}
          </CardTitle>
          <CardDescription>{t('metaTools.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-start gap-3 flex-1 min-w-0">
              <Sparkles className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
              <div>
                <label className="text-sm font-medium">{t('metaTools.advertise')}</label>
                <p className="text-xs text-[rgb(var(--muted))] mt-1">{t('metaTools.advertiseDesc')}</p>
              </div>
            </div>
            <Switch
              checked={metaToolsEnabled}
              onCheckedChange={handleToggleMetaTools}
              disabled={loadingMetaTools}
              data-testid="meta-tools-enabled-switch"
            />
          </div>
          <MetaToolGrantsPanel />
          <MetaToolAuditLog />
        </CardContent>
      </Card>

      {/* Analytics Section */}
      <Card data-testid="settings-analytics-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BarChart3 className="h-5 w-5" />
            {t('analytics.title')}
          </CardTitle>
          <CardDescription>{t('analytics.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-start gap-3 flex-1 min-w-0">
              <BarChart3 className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
              <div>
                <label className="text-sm font-medium">{t('analytics.share')}</label>
                <p className="text-xs text-[rgb(var(--muted))] mt-1">{t('analytics.shareDesc')}</p>
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
            {t('contribute.title')}
          </CardTitle>
          <CardDescription>{t('contribute.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <ContributeRow
              icon={Package}
              title={t('contribute.requestServer')}
              subtitle={t('contribute.requestServerDesc')}
              onClick={() => openExternal(CONTRIBUTE.requestServer())}
              testId="contribute-request-server"
            />
            <ContributeRow
              icon={Bug}
              title={t('contribute.reportBug')}
              subtitle={t('contribute.reportBugDesc')}
              onClick={() => openExternal(CONTRIBUTE.bug)}
              testId="contribute-report-bug"
            />
            <ContributeRow
              icon={Lightbulb}
              title={t('contribute.suggestFeature')}
              subtitle={t('contribute.suggestFeatureDesc')}
              onClick={() => openExternal(CONTRIBUTE.featureRequest)}
              testId="contribute-feature-request"
            />
            <ContributeRow
              icon={Github}
              title={t('contribute.openGithub')}
              subtitle={t('contribute.openGithubDesc')}
              onClick={() => openExternal(CONTRIBUTE.repo)}
              testId="contribute-open-github"
            />
          </div>
        </CardContent>
      </Card>

      {/* Logs Section */}
      <Card data-testid="settings-logs-section">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            {t('logs.title')}
          </CardTitle>
          <CardDescription>{t('logs.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">{t('logs.location')}</label>
              <p className="text-sm text-[rgb(var(--muted))] mt-1 font-mono bg-surface-secondary rounded px-2 py-1" data-testid="logs-path">
                {logsPath || t('logs.loading')}
              </p>
            </div>
            <div className="flex items-center gap-2">
              {isTauri() ? (
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
                  {t('logs.openFolder')}
                </Button>
              ) : null}
            </div>
            <div className="border-t border-[rgb(var(--border))] pt-4">
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-start gap-3 flex-1 min-w-0">
                  <Trash2 className="h-5 w-5 mt-0.5 text-[rgb(var(--muted))] flex-shrink-0" />
                  <div>
                    <label className="text-sm font-medium">{t('logs.autoCleanup')}</label>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">
                      {t('logs.autoCleanupDesc')}
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
                  <option value={7}>{t('logs.retention7')}</option>
                  <option value={14}>{t('logs.retention14')}</option>
                  <option value={30}>{t('logs.retention30')}</option>
                  <option value={60}>{t('logs.retention60')}</option>
                  <option value={90}>{t('logs.retention90')}</option>
                  <option value={0}>{t('logs.keepForever')}</option>
                </select>
              </div>
            </div>
            <p className="text-xs text-[rgb(var(--muted))]">
              {t('logs.footer')}
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
    </>
  );
}

/**
 * Settings card for this install's machine identity and catalog management.
 */
function MachineIdentitySection({
  onSuccess,
  onError,
  confirm,
  t,
}: {
  onSuccess: (title: string, message?: string) => void;
  onError: (title: string, message?: string) => void;
  confirm: (options: {
    title: string;
    message: string;
    confirmLabel: string;
    cancelLabel: string;
    variant?: 'danger' | 'default';
  }) => Promise<boolean>;
  t: TFunction<['settings', 'common']>;
}) {
  const viewer = useViewerIdentity();
  const viewingLocally = isViewingLocally();
  const [machines, setMachines] = useState<Machine[]>([]);
  const [localMachineId, setLocalMachineIdState] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [registeringGateway, setRegisteringGateway] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const [registerName, setRegisterName] = useState('');
  const [registerIcon, setRegisterIcon] = useState('');
  const [registerHostname, setRegisterHostname] = useState('');
  const [rowDrafts, setRowDrafts] = useState<
    Record<string, { name: string; icon: string; hostname: string }>
  >({});
  const [savingRowId, setSavingRowId] = useState<string | null>(null);
  const [deletingRowId, setDeletingRowId] = useState<string | null>(null);

  const localMachine = localMachineId
    ? machines.find((machine) => machine.id === localMachineId) ?? null
    : null;

  const loadMachines = async () => {
    try {
      const [list, localId] = await Promise.all([listMachines(), getLocalMachineId()]);
      setMachines(list);
      setLocalMachineIdState(localId);
      const drafts: Record<string, { name: string; icon: string; hostname: string }> = {};
      for (const machine of list) {
        drafts[machine.id] = {
          name: machine.name,
          icon: machine.icon ?? '',
          hostname: machine.hostname ?? '',
        };
      }
      setRowDrafts(drafts);
    } catch (err) {
      console.error('Failed to load machines:', err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadMachines();
    const onChanged = () => {
      void loadMachines();
    };
    window.addEventListener(VIEWER_IDENTITY_CHANGED, onChanged);
    return () => window.removeEventListener(VIEWER_IDENTITY_CHANGED, onChanged);
  }, []);

  const handleViewerSave = async () => {
    const ok = await viewer.saveProfile();
    if (ok) {
      onSuccess(t('machineIdentity.toast.saved'), viewer.name ?? undefined);
      return;
    }
    if (viewer.error === 'saveFailed') {
      onError(t('machineIdentity.toast.failedSave'));
      return;
    }
    if (
      viewer.error === 'name' ||
      viewer.error === 'icon' ||
      viewer.error === 'hostname'
    ) {
      onError(t(`machineIdentity.${viewer.error}Required`));
    }
  };

  const handleRegisterGateway = async () => {
    const missingField = getMissingMachineProfileField({
      name: registerName,
      icon: registerIcon,
      hostname: registerHostname,
    });
    if (missingField) {
      onError(t(`machineIdentity.${missingField}Required`));
      return;
    }
    setRegisteringGateway(true);
    try {
      const created = await createMachine(
        toMachineProfilePayload({
          name: registerName,
          icon: registerIcon,
          hostname: registerHostname,
        }),
      );
      await setLocalMachineId(created.id);
      setMachines((prev) => [...prev, created].sort((a, b) => a.name.localeCompare(b.name)));
      setLocalMachineIdState(created.id);
      setRegisterName('');
      setRegisterIcon('');
      setRegisterHostname('');
      window.dispatchEvent(new Event(VIEWER_IDENTITY_CHANGED));
      onSuccess(t('machineIdentity.toast.registered'), created.name);
    } catch (err) {
      onError(
        t('machineIdentity.toast.failedRegister'),
        err instanceof Error ? err.message : String(err),
      );
    } finally {
      setRegisteringGateway(false);
    }
  };

  const handlePrefillRegisterHostname = async () => {
    try {
      const hostname = await getHostname();
      setRegisterHostname(hostname);
    } catch {
      /* hostname hint is optional */
    }
  };

  const handleSaveRow = async (machine: Machine) => {
    const draft = rowDrafts[machine.id];
    if (!draft) return;
    const missingField = getMissingMachineProfileField(draft);
    if (missingField) {
      onError(t(`machineIdentity.${missingField}Required`));
      return;
    }
    setSavingRowId(machine.id);
    try {
      const updated = await updateMachine(machine.id, toMachineProfilePayload(draft));
      setMachines((prev) =>
        prev.map((row) => (row.id === updated.id ? updated : row))
      );
      if (localMachineId === updated.id || viewer.machineId === updated.id) {
        window.dispatchEvent(new Event(VIEWER_IDENTITY_CHANGED));
      }
      onSuccess(t('machineIdentity.toast.saved'), updated.name);
    } catch (err) {
      onError(
        t('machineIdentity.toast.failedSave'),
        err instanceof Error ? err.message : String(err)
      );
    } finally {
      setSavingRowId(null);
    }
  };

  const handleDeleteRow = async (machine: Machine) => {
    const ok = await confirm({
      title: t('machineIdentity.confirmDeleteTitle'),
      message: t('machineIdentity.confirmDeleteMessage', { name: machine.name }),
      confirmLabel: t('machineIdentity.deleteMachine'),
      cancelLabel: t('common:actions.cancel'),
      variant: 'danger',
    });
    if (!ok) return;
    setDeletingRowId(machine.id);
    try {
      const [, allBindings] = await Promise.all([deleteMachine(machine.id), listWorkspaceBindings()]);
      const scopedBindings = allBindings.filter((b) => b.machine_id === machine.id);
      await Promise.all(scopedBindings.map((b) => deleteWorkspaceBinding(b.id)));
      setMachines((prev) => prev.filter((row) => row.id !== machine.id));
      if (localMachineId === machine.id) {
        await setLocalMachineId(null);
        setLocalMachineIdState(null);
        window.dispatchEvent(new Event(VIEWER_IDENTITY_CHANGED));
      }
      setRowDrafts((prev) => {
        const next = { ...prev };
        delete next[machine.id];
        return next;
      });
      onSuccess(t('machineIdentity.toast.deleted'), machine.name);
    } catch (err) {
      onError(
        t('machineIdentity.toast.failedDelete'),
        err instanceof Error ? err.message : String(err)
      );
    } finally {
      setDeletingRowId(null);
    }
  };

  const registerCanSave = isMachineProfileComplete({
    name: registerName,
    icon: registerIcon,
    hostname: registerHostname,
  });

  const viewerSectionLabel = viewingLocally
    ? t('machineIdentity.thisInstall')
    : t('machineIdentity.thisViewer');

  return (
    <Card data-testid="settings-machine-identity-section">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Monitor className="h-5 w-5" />
          {t('machineIdentity.title')}
        </CardTitle>
        <CardDescription>{t('machineIdentity.description')}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        {loading || viewer.isLoading ? (
          <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t('loading')}
          </div>
        ) : (
          <>
            <div className="space-y-4">
              <div>
                <p className="text-sm font-medium">{viewerSectionLabel}</p>
                {!viewingLocally && viewer.hints ? (
                  <p className="mt-1 text-xs text-[rgb(var(--muted))]">{viewer.hints}</p>
                ) : null}
              </div>
              <MachineProfileEditor
                nameDraft={viewer.nameDraft}
                iconDraft={viewer.iconDraft}
                hostnameDraft={viewer.hostnameDraft}
                onNameDraftChange={viewer.setNameDraft}
                onIconDraftChange={viewer.setIconDraft}
                onHostnameDraftChange={viewer.setHostnameDraft}
                onSave={() => void handleViewerSave()}
                isSaving={viewer.isSaving}
                saveDisabled={!viewer.canSaveProfile}
                nameLabel={t('machineIdentity.nameLabel')}
                iconLabel={t('machineIdentity.iconLabel')}
                hostnameLabel={t('machineIdentity.hostnameLabel')}
                saveLabel={t('machineIdentity.save')}
                testIdPrefix="machine-identity-viewer"
              />
            </div>

            {!viewingLocally && !localMachine ? (
                <div className="space-y-4 border-t border-[rgb(var(--border-subtle))] pt-6">
                  <p className="text-sm font-medium">{t('machineIdentity.thisGateway')}</p>
                  <p className="text-sm text-[rgb(var(--muted))]">{t('machineIdentity.notRegistered')}</p>
                  <div className="flex items-center gap-3">
                    <EmojiPickerButton
                      value={registerIcon}
                      onChange={setRegisterIcon}
                      testId="machine-identity-register-icon"
                    />
                    <input
                      type="text"
                      value={registerName}
                      onChange={(e) => setRegisterName(e.target.value)}
                      placeholder={t('machineIdentity.nameLabel')}
                      className="min-w-0 flex-1 h-10 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 text-sm"
                      data-testid="machine-identity-register-name"
                    />
                    <input
                      type="text"
                      value={registerHostname}
                      onChange={(e) => setRegisterHostname(e.target.value)}
                      onFocus={() => {
                        if (!registerHostname) void handlePrefillRegisterHostname();
                      }}
                      placeholder={t('machineIdentity.hostnameLabel')}
                      className="min-w-0 flex-1 h-10 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] font-mono px-3 text-sm"
                      data-testid="machine-identity-register-hostname"
                    />
                  </div>
                  <Button
                    variant="primary"
                    size="sm"
                    onClick={() => void handleRegisterGateway()}
                    disabled={registeringGateway || !registerCanSave}
                    data-testid="machine-identity-register-btn"
                  >
                    {registeringGateway ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
                    {t('machineIdentity.createMachine')}
                  </Button>
                </div>
            ) : null}
          </>
        )}

        <div className="border-t border-[rgb(var(--border))] pt-4">
          <button
            type="button"
            onClick={() => setManageOpen((open) => !open)}
            className="w-full flex items-center justify-between gap-3 text-left"
            data-testid="machine-identity-manage-toggle"
          >
            <div>
              <p className="text-sm font-medium">{t('machineIdentity.manageAll')}</p>
              <p className="text-xs text-[rgb(var(--muted))] mt-0.5">
                {t('machineIdentity.manageAllDesc')}
              </p>
            </div>
            {manageOpen ? (
              <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))] flex-shrink-0" />
            ) : (
              <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))] flex-shrink-0" />
            )}
          </button>

          {manageOpen ? (
            <div className="mt-4 space-y-3">
              {machines.length === 0 ? (
                <p className="text-sm text-[rgb(var(--muted))] italic">
                  {t('machineIdentity.noMachines')}
                </p>
              ) : (
                machines.map((machine) => {
                  const draft = rowDrafts[machine.id] ?? {
                    name: machine.name,
                    icon: machine.icon ?? '',
                    hostname: machine.hostname ?? '',
                  };
                  const dirty =
                    draft.name.trim() !== machine.name ||
                    (draft.icon.trim() || null) !== machine.icon ||
                    (draft.hostname.trim() || null) !== machine.hostname;
                  return (
                    <div
                      key={machine.id}
                      className="rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-3 space-y-2"
                      data-testid={`machine-identity-row-${machine.id}`}
                    >
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
                          {machine.id === localMachineId
                            ? viewingLocally
                              ? t('machineIdentity.thisInstall')
                              : t('machineIdentity.thisGateway')
                            : machine.name}
                        </span>
                        {machine.id === localMachineId ? (
                          <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300 border border-primary-200 dark:border-primary-800">
                            local
                          </span>
                        ) : null}
                      </div>
                      <div className="flex items-center gap-2">
                        <EmojiPickerButton
                          value={draft.icon}
                          onChange={(emoji) =>
                            setRowDrafts((prev) => ({
                              ...prev,
                              [machine.id]: { ...draft, icon: emoji },
                            }))
                          }
                        />
                        <input
                          type="text"
                          value={draft.name}
                          onChange={(e) =>
                            setRowDrafts((prev) => ({
                              ...prev,
                              [machine.id]: { ...draft, name: e.target.value },
                            }))
                          }
                          className="min-w-0 flex-1 h-10 px-3 text-sm border border-[rgb(var(--border))] rounded-lg bg-[rgb(var(--background))]"
                        />
                        <input
                          type="text"
                          value={draft.hostname}
                          onChange={(e) =>
                            setRowDrafts((prev) => ({
                              ...prev,
                              [machine.id]: { ...draft, hostname: e.target.value },
                            }))
                          }
                          placeholder={t('machineIdentity.hostnameLabel')}
                          className="min-w-0 flex-1 h-10 px-3 text-sm font-mono border border-[rgb(var(--border))] rounded-lg bg-[rgb(var(--background))]"
                        />
                      </div>
                      <div className="flex items-center gap-2">
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => void handleSaveRow(machine)}
                          disabled={savingRowId === machine.id || !dirty || !isMachineProfileComplete(draft)}
                        >
                          {savingRowId === machine.id ? (
                            <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                          ) : null}
                          {t('machineIdentity.save')}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => void handleDeleteRow(machine)}
                          disabled={deletingRowId === machine.id}
                          className="text-red-600 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
                        >
                          {deletingRowId === machine.id ? (
                            <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                          ) : (
                            <Trash2 className="h-4 w-4 mr-2" />
                          )}
                          {t('machineIdentity.deleteMachine')}
                        </Button>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          ) : null}
        </div>
      </CardContent>
    </Card>
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
