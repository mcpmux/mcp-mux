import { useState } from 'react';
import { check, Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@mcpmux/ui';
import { Download, Loader2, CheckCircle, AlertCircle, RefreshCw } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

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
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [downloadProgress, setDownloadProgress] = useState({ downloaded: 0, total: 0 });
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [currentVersion, setCurrentVersion] = useState<string>('');

  // Load current version on mount
  useState(() => {
    invoke<string>('get_version')
      .then(setCurrentVersion)
      .catch((err) => console.error('Failed to get version:', err));
  });

  const checkForUpdates = async () => {
    setChecking(true);
    setMessage(null);
    setUpdateInfo(null);

    try {
      console.log('[Updater] Checking for updates...');
      const update = await check();

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
      // Note: On Windows, the app will exit automatically before this point
      await relaunch();
    } catch (error) {
      console.error('[Updater] Installation failed:', error);
      setMessage({
        type: 'error',
        text: `Failed to install update: ${error}`,
      });
      setDownloading(false);
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

          {/* Check Button */}
          {!updateInfo && (
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
                      {downloadProgress.total > 0 ? 'Downloading...' : 'Installing...'}
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

              {downloading && (
                <p className="text-xs text-[rgb(var(--muted))]">
                  <strong>Note:</strong> On Windows, the app will close automatically to install the update.
                </p>
              )}
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
