import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { checkAppUpdate, relaunchApp, type Update } from '@/lib/backend/shell';
import { getBundleVersion, getVersion } from '@/lib/backend';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@mcpmux/ui';
import { Download, Loader2, CheckCircle, AlertCircle, RefreshCw, RotateCcw } from 'lucide-react';
import { BuildStampPanel } from './BuildStampPanel';

interface DownloadEvent {
  event: 'Started' | 'Progress' | 'Finished';
  data?: {
    contentLength?: number;
    chunkLength?: number;
  };
}

/**
 * Desktop settings card for checking and installing application updates.
 */
export function UpdateChecker() {
  const { t } = useTranslation('settings');
  const [checking, setChecking] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] = useState({ downloaded: 0, total: 0 });
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string>('');
  const [bundleVersionMismatch, setBundleVersionMismatch] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setCurrentVersion)
      .catch((err) => console.error('Failed to get version:', err));
  }, []);

  useEffect(() => {
    if (!currentVersion) return;
    getBundleVersion()
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

  /**
   * Query the updater plugin for a newer release.
   */
  const checkForUpdates = async () => {
    setChecking(true);
    setMessage(null);
    setUpdateInfo(null);

    try {
      console.log('[Updater] Checking for updates...');
      const update = await checkAppUpdate();

      if (update) {
        console.log(
          `[Updater] Update available: ${update.version} from ${update.date || 'N/A'}`
        );
        setUpdateInfo(update);
        setMessage({
          type: 'success',
          text: t('updates.versionAvailable', { version: update.version }),
        });
      } else {
        console.log('[Updater] No updates available');
        setMessage({
          type: 'success',
          text: t('updates.latestVersion'),
        });
      }
    } catch (error) {
      console.error('[Updater] Check failed:', error);
      setMessage({
        type: 'error',
        text: t('updates.checkFailed', { error: String(error) }),
      });
    } finally {
      setChecking(false);
    }
  };

  /**
   * Download and install the available update, then relaunch the app.
   */
  const installUpdate = async () => {
    if (!updateInfo) return;

    setDownloading(true);
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
            break;
        }
      });

      console.log('[Updater] Update installed successfully, relaunching app...');
      await relaunchApp();
    } catch (error) {
      console.error('[Updater] Installation failed:', error);
      setMessage({
        type: 'error',
        text: t('updates.installFailed', { error: String(error) }),
      });
      setDownloading(false);
    }
  };

  /**
   * Format byte counts for the download progress bar.
   */
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
          {t('updates.title')}
        </CardTitle>
        <CardDescription>{t('updates.description')}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="space-y-4">
          <div>
            <label className="text-sm font-medium">{t('updates.currentVersion')}</label>
            <p className="text-sm text-[rgb(var(--muted))] mt-1" data-testid="current-version">
              v{currentVersion || '0.0.5'}
            </p>
            <BuildStampPanel context="desktop" />
          </div>

          {bundleVersionMismatch && (
            <div
              className="border rounded-lg p-4 space-y-3 bg-surface-secondary"
              data-testid="restart-required"
            >
              <div>
                <p className="font-medium text-lg">{t('updates.restartRequired')}</p>
                <p className="text-sm text-[rgb(var(--muted))] mt-1">
                  {t('updates.restartDescription', {
                    bundleVersion: bundleVersionMismatch,
                    currentVersion,
                  })}
                </p>
              </div>
              <Button
                onClick={() => void relaunchApp()}
                variant="primary"
                data-testid="restart-now-btn"
              >
                <RotateCcw className="h-4 w-4 mr-2" />
                {t('updates.restartNow')}
              </Button>
            </div>
          )}

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
                  {t('updates.checking')}
                </>
              ) : (
                <>
                  <RefreshCw className="h-4 w-4 mr-2" />
                  {t('updates.checkForUpdates')}
                </>
              )}
            </Button>
          )}

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

          {updateInfo && (
            <div className="border rounded-lg p-4 space-y-3 bg-surface-secondary" data-testid="update-available">
              <div>
                <p className="font-medium text-lg">
                  {t('updates.updateAvailableTitle', { version: updateInfo.version })}
                </p>
                {updateInfo.date && (
                  <p className="text-xs text-[rgb(var(--muted))]">
                    {t('updates.released', {
                      date: new Date(updateInfo.date).toLocaleDateString(),
                    })}
                  </p>
                )}
              </div>

              {updateInfo.body && (
                <div className="text-sm">
                  <p className="font-medium mb-1">{t('updates.whatsNew')}</p>
                  <div className="text-[rgb(var(--muted))] whitespace-pre-wrap max-h-32 overflow-y-auto">
                    {updateInfo.body}
                  </div>
                </div>
              )}

              {downloading && downloadProgress.total > 0 && (
                <div className="space-y-2">
                  <div className="flex justify-between text-xs text-[rgb(var(--muted))]">
                    <span>{t('updates.downloading')}</span>
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
                      {downloadProgress.total > 0 ? t('updates.downloading') : t('updates.installing')}
                    </>
                  ) : (
                    <>
                      <Download className="h-4 w-4 mr-2" />
                      {t('updates.downloadAndInstall')}
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
                    {t('updates.remindLater')}
                  </Button>
                )}
              </div>

              {downloading && (
                <p className="text-xs text-[rgb(var(--muted))]">{t('updates.windowsNote')}</p>
              )}
            </div>
          )}

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
