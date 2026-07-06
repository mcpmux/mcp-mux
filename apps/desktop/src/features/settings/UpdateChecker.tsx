import { useState, useEffect } from 'react';
import { Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import {
  checkForUpdate,
  getUpdateChannel,
  setUpdateChannel,
  type UpdateChannel,
} from '@/lib/updates';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  Switch,
} from '@mcpmux/ui';
import { Download, Loader2, CheckCircle, AlertCircle, RefreshCw, RotateCcw } from 'lucide-react';
import { call as invoke } from '@/lib/transport';

interface DownloadEvent {
  event: 'Started' | 'Progress' | 'Finished';
  data?: {
    contentLength?: number;
    chunkLength?: number;
  };
}

export function UpdateChecker() {
  const [checking, setChecking] = useState(false);
  const [downloading, setDownloading] = useState(false);
  // True once the download finishes and the installer takes over. On Windows
  // the app is killed during this phase, so we surface a clear "restarting"
  // notice first — otherwise the window vanishing reads as a crash.
  const [installing, setInstalling] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] = useState({ downloaded: 0, total: 0 });
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string>('');
  const [bundleVersionMismatch, setBundleVersionMismatch] = useState<string | null>(null);
  const [autoInstall, setAutoInstall] = useState<boolean | null>(null);
  const [channel, setChannel] = useState<UpdateChannel | null>(null);

  // Load current version on mount
  useState(() => {
    invoke<string>('get_version')
      .then(setCurrentVersion)
      .catch((err) => console.error('Failed to get version:', err));
  });

  // Load the auto-install preference (default on).
  useEffect(() => {
    invoke<boolean>('get_auto_install_updates')
      .then(setAutoInstall)
      .catch(() => setAutoInstall(true));
  }, []);

  const handleToggleAutoInstall = async (next: boolean) => {
    const prev = autoInstall;
    setAutoInstall(next);
    try {
      await invoke('set_auto_install_updates', { enabled: next });
    } catch (err) {
      setAutoInstall(prev);
      setMessage({ type: 'error', text: `Failed to save setting: ${err}` });
    }
  };

  // Load the current update channel (default stable).
  useEffect(() => {
    getUpdateChannel()
      .then(setChannel)
      .catch(() => setChannel('stable'));
  }, []);

  const handleSelectChannel = async (next: UpdateChannel) => {
    if (next === channel) return;
    const prev = channel;
    setChannel(next);
    // A channel switch invalidates any update found on the previous channel.
    setUpdateInfo(null);
    setMessage(null);
    try {
      await setUpdateChannel(next);
    } catch (err) {
      setChannel(prev);
      setMessage({ type: 'error', text: `Failed to switch channel: ${err}` });
    }
  };

  // Check if the on-disk bundle version differs from the running version (Homebrew Cask upgrades)
  useEffect(() => {
    if (!currentVersion) return;
    invoke<string | null>('get_bundle_version')
      .then((bundleVersion) => {
        if (bundleVersion && bundleVersion !== currentVersion) {
          console.log(
            `[Updater] Bundle version mismatch: running=${currentVersion}, on-disk=${bundleVersion}`
          );
          setBundleVersionMismatch(bundleVersion);
        }
      })
      .catch(() => {
        // Expected to return null on non-macOS platforms
      });
  }, [currentVersion]);

  const checkForUpdates = async () => {
    setChecking(true);
    setMessage(null);
    setUpdateInfo(null);

    try {
      console.log(`[Updater] Checking for updates (channel: ${channel ?? 'stable'})...`);
      const update = await checkForUpdate();

      if (update) {
        console.log(
          `[Updater] Update available: ${update.version} from ${update.date || 'N/A'}`
        );
        setUpdateInfo(update);
        setMessage({
          type: 'success',
          text: `Version ${update.version} is available!`,
        });
      } else {
        console.log('[Updater] No updates available');
        setMessage({
          type: 'success',
          text: "You're running the latest version!",
        });
      }
    } catch (error) {
      console.error('[Updater] Check failed:', error);
      setMessage({
        type: 'error',
        text: `Failed to check for updates: ${error}`,
      });
    } finally {
      setChecking(false);
    }
  };

  const installUpdate = async () => {
    if (!updateInfo) return;

    setDownloading(true);
    setInstalling(false);
    setDownloadProgress({ downloaded: 0, total: 0 });
    setMessage(null);

    try {
      console.log('[Updater] Starting download and install...');

      await updateInfo.downloadAndInstall((event: DownloadEvent) => {
        switch (event.event) {
          case 'Started':
            console.log(`[Updater] Downloading ${event.data?.contentLength || 0} bytes`);
            setDownloadProgress({
              downloaded: 0,
              total: event.data?.contentLength || 0,
            });
            break;
          case 'Progress':
            setDownloadProgress((prev) => ({
              ...prev,
              downloaded: prev.downloaded + (event.data?.chunkLength || 0),
            }));
            break;
          case 'Finished':
            console.log('[Updater] Download finished, installing...');
            // Hand-off to the installer. On Windows the app is killed here,
            // so flip to the restart notice now (before the window closes)
            // so the disappearance is expected, not a surprise.
            setInstalling(true);
            break;
        }
      });

      console.log('[Updater] Update installed successfully, relaunching app...');
      // Note: On Windows, the app will exit automatically before this point
      await relaunch();
    } catch (error) {
      console.error('[Updater] Installation failed:', error);
      setMessage({
        type: 'error',
        text: `Failed to install update: ${error}`,
      });
      setDownloading(false);
      setInstalling(false);
    }
  };

  const formatBytes = (bytes: number): string => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`;
  };

  const progressPercent =
    downloadProgress.total > 0
      ? Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)
      : 0;

  return (
    <Card data-testid="update-checker">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <RefreshCw className="h-5 w-5" />
          Software Updates
        </CardTitle>
        <CardDescription>
          Keep your application up to date with the latest features and fixes.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="space-y-4">
          {/* Current Version */}
          <div>
            <label className="text-sm font-medium">Current Version</label>
            <p className="text-sm text-[rgb(var(--muted))] mt-1" data-testid="current-version">
              v{currentVersion || '0.0.5'}
            </p>
          </div>

          {/* Update channel */}
          <div className="flex items-start justify-between gap-4 rounded-lg border p-3">
            <div className="space-y-0.5">
              <p className="text-sm font-medium">Update channel</p>
              <p className="text-xs text-[rgb(var(--muted))]">
                {channel === 'prerelease'
                  ? 'Pre-release: early builds from every change merged to main. Newer, but may be unstable.'
                  : 'Stable: published releases only. Recommended for most users.'}
              </p>
            </div>
            <div
              className="inline-flex flex-shrink-0 rounded-md border p-0.5"
              role="group"
              aria-label="Update channel"
              data-testid="update-channel-selector"
            >
              {(['stable', 'prerelease'] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  disabled={channel === null}
                  aria-pressed={channel === value}
                  onClick={() => handleSelectChannel(value)}
                  data-testid={`update-channel-${value}`}
                  className={`rounded px-3 py-1 text-xs font-medium transition-colors disabled:opacity-50 ${
                    channel === value
                      ? 'bg-primary-500 text-white'
                      : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]'
                  }`}
                >
                  {value === 'stable' ? 'Stable' : 'Pre-release'}
                </button>
              ))}
            </div>
          </div>

          {/* Auto-install preference */}
          <div className="flex items-start justify-between gap-4 rounded-lg border p-3">
            <div className="space-y-0.5">
              <p className="text-sm font-medium">Install updates automatically</p>
              <p className="text-xs text-[rgb(var(--muted))]">
                Download and apply new versions on launch, then restart into the update. Turn off to
                review each update before installing.
              </p>
            </div>
            <Switch
              checked={autoInstall ?? true}
              disabled={autoInstall === null}
              onCheckedChange={handleToggleAutoInstall}
              data-testid="auto-install-updates-toggle"
            />
          </div>

          {/* Bundle version mismatch (e.g., after brew upgrade) */}
          {bundleVersionMismatch && (
            <div
              className="border rounded-lg p-4 space-y-3 bg-surface-secondary"
              data-testid="restart-required"
            >
              <div>
                <p className="font-medium text-lg">Restart Required</p>
                <p className="text-sm text-[rgb(var(--muted))] mt-1">
                  Version v{bundleVersionMismatch} has been installed on disk, but you are still
                  running v{currentVersion}. Restart to apply the update.
                </p>
              </div>
              <Button
                onClick={() => relaunch()}
                variant="primary"
                data-testid="restart-now-btn"
              >
                <RotateCcw className="h-4 w-4 mr-2" />
                Restart Now
              </Button>
            </div>
          )}

          {/* Check Button */}
          {!updateInfo && !bundleVersionMismatch && (
            <Button
              onClick={checkForUpdates}
              disabled={checking || downloading}
              variant="secondary"
              data-testid="check-updates-btn"
            >
              {checking ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin mr-2" />
                  Checking for Updates...
                </>
              ) : (
                <>
                  <RefreshCw className="h-4 w-4 mr-2" />
                  Check for Updates
                </>
              )}
            </Button>
          )}

          {/* Status Message */}
          {message && !updateInfo && (
            <div
              className={`flex items-start gap-2 p-3 rounded-lg text-sm ${
                message.type === 'success'
                  ? 'bg-green-500/10 text-green-600 dark:text-green-400'
                  : 'bg-red-500/10 text-red-600 dark:text-red-400'
              }`}
              data-testid="update-message"
            >
              {message.type === 'success' ? (
                <CheckCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
              ) : (
                <AlertCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
              )}
              <span>{message.text}</span>
            </div>
          )}

          {/* Update Available Card */}
          {updateInfo && (
            <div className="border rounded-lg p-4 space-y-3 bg-surface-secondary" data-testid="update-available">
              <div>
                <p className="font-medium text-lg">
                  Update Available: v{updateInfo.version}
                </p>
                {updateInfo.date && (
                  <p className="text-xs text-[rgb(var(--muted))]">
                    Released: {new Date(updateInfo.date).toLocaleDateString()}
                  </p>
                )}
              </div>

              {/* Release Notes */}
              {updateInfo.body && (
                <div className="text-sm">
                  <p className="font-medium mb-1">What's New:</p>
                  <div className="text-[rgb(var(--muted))] whitespace-pre-wrap max-h-32 overflow-y-auto">
                    {updateInfo.body}
                  </div>
                </div>
              )}

              {/* Download Progress */}
              {downloading && downloadProgress.total > 0 && (
                <div className="space-y-2">
                  <div className="flex justify-between text-xs text-[rgb(var(--muted))]">
                    <span>Downloading...</span>
                    <span>
                      {formatBytes(downloadProgress.downloaded)} / {formatBytes(downloadProgress.total)} ({progressPercent}%)
                    </span>
                  </div>
                  <div className="w-full bg-surface-secondary rounded-full h-2 overflow-hidden">
                    <div
                      className="bg-primary-500 h-full transition-all duration-300"
                      style={{ width: `${progressPercent}%` }}
                    />
                  </div>
                </div>
              )}

              {/* Install Button */}
              <div className="flex gap-2">
                <Button
                  onClick={installUpdate}
                  disabled={downloading}
                  variant="primary"
                  data-testid="install-update-btn"
                >
                  {downloading ? (
                    <>
                      <Loader2 className="h-4 w-4 animate-spin mr-2" />
                      {installing
                        ? 'Restarting…'
                        : downloadProgress.total > 0
                          ? 'Downloading...'
                          : 'Installing...'}
                    </>
                  ) : (
                    <>
                      <Download className="h-4 w-4 mr-2" />
                      Download and Install
                    </>
                  )}
                </Button>
                {!downloading && (
                  <Button
                    onClick={() => {
                      setUpdateInfo(null);
                      setMessage(null);
                    }}
                    variant="secondary"
                    data-testid="dismiss-update-btn"
                  >
                    Remind Me Later
                  </Button>
                )}
              </div>

              {downloading &&
                (installing ? (
                  <div
                    className="flex items-start gap-2 rounded-lg bg-blue-500/10 p-3 text-sm text-blue-600 dark:text-blue-400"
                    data-testid="update-restarting"
                  >
                    <RotateCcw className="mt-0.5 h-4 w-4 flex-shrink-0 animate-spin" />
                    <span>
                      Installing v{updateInfo.version} — McpMux will close and reopen automatically.
                      This is expected; the app isn't crashing.
                    </span>
                  </div>
                ) : (
                  <p className="text-xs text-[rgb(var(--muted))]">
                    <strong>Note:</strong> When the download finishes, McpMux closes briefly to
                    install the update, then reopens on its own.
                  </p>
                ))}
            </div>
          )}

          {/* Error Message for Update Available State */}
          {message && updateInfo && message.type === 'error' && (
            <div
              className="flex items-start gap-2 p-3 rounded-lg text-sm bg-red-500/10 text-red-600 dark:text-red-400"
              data-testid="update-error"
            >
              <AlertCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
              <span>{message.text}</span>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
