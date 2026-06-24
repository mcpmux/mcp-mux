import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@mcpmux/ui';
import { Loader2, Package, RefreshCw } from 'lucide-react';
import {
  checkAllServerUpdates,
  getServerUpdateSettings,
  updateServerUpdateSettings,
  type ServerUpdateSettings,
  type UpdatePolicy,
} from '@/lib/api/settings';
import { discoverServers, listInstalledServers } from '@/lib/api/registry';
import { updateServerPackage } from '@/lib/api/serverManager';
import {
  buildPendingServerUpdates,
  type ServerPendingUpdate,
} from '@/features/servers/server-pending-updates.helpers';
import { useDomainEvents } from '@/hooks/useDomainEvents';
import { pendingUpdateKey } from '@/features/servers/server-pending-updates.helpers';
import { ServerPendingUpdatesList } from './ServerPendingUpdatesList';
import { getUpdatePolicyOptions } from '@/features/servers/server-update-policy.helpers';

/** Format an ISO timestamp for display in settings. */
function formatCheckedAt(value: string | null | undefined): string | null {
  if (!value) {
    return null;
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return null;
  }
  return parsed.toLocaleString();
}

interface ServerUpdatesSectionProps {
  onSuccess?: (title: string, message?: string) => void;
  onError?: (title: string, message?: string) => void;
  onInfo?: (title: string, message?: string) => void;
}

/**
 * Settings section for the app-wide default server update policy.
 */
export function ServerUpdatesSection({
  onSuccess,
  onError,
  onInfo,
}: ServerUpdatesSectionProps) {
  const [settings, setSettings] = useState<ServerUpdateSettings>({
    defaultUpdatePolicy: 'notify',
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [checkingAll, setCheckingAll] = useState(false);
  const [pendingUpdates, setPendingUpdates] = useState<ServerPendingUpdate[]>([]);
  const [loadingPending, setLoadingPending] = useState(false);
  const [updatingServerKey, setUpdatingServerKey] = useState<string | null>(null);
  const [updatingAll, setUpdatingAll] = useState(false);
  const { subscribe } = useDomainEvents();

  const policyOptions = useMemo(() => getUpdatePolicyOptions(), []);

  /** Load installed servers and derive which have newer packages available. */
  const refreshPendingUpdates = useCallback(async () => {
    setLoadingPending(true);
    try {
      const [installedResult, definitionsResult] = await Promise.allSettled([
        listInstalledServers(),
        discoverServers(),
      ]);
      const installed = installedResult.status === 'fulfilled' ? installedResult.value : [];
      const definitions =
        definitionsResult.status === 'fulfilled' ? definitionsResult.value : [];
      setPendingUpdates(buildPendingServerUpdates(installed, definitions));
    } catch (err) {
      console.error('[Settings] Failed to load pending server updates:', err);
    } finally {
      setLoadingPending(false);
    }
  }, []);

  useEffect(() => {
    const load = async () => {
      try {
        const loaded = await getServerUpdateSettings();
        setSettings(loaded);
      } catch (err) {
        console.error('[Settings] Failed to load server update settings:', err);
      } finally {
        setLoading(false);
      }
    };
    void load();
    void refreshPendingUpdates();
  }, [refreshPendingUpdates]);

  useEffect(() => {
    return subscribe('server-update-available', () => {
      void refreshPendingUpdates();
    });
  }, [refreshPendingUpdates, subscribe]);

  useEffect(() => {
    return subscribe('server-changed', () => {
      void refreshPendingUpdates();
    });
  }, [refreshPendingUpdates, subscribe]);

  /** Persist a new default update policy for newly installed servers. */
  const handlePolicyChange = async (policy: UpdatePolicy) => {
    const previous = settings;
    const next = { ...settings, defaultUpdatePolicy: policy };
    setSettings(next);
    setSaving(true);
    try {
      await updateServerUpdateSettings(next);
    } catch (err) {
      console.error('[Settings] Failed to save server update settings:', err);
      setSettings(previous);
    } finally {
      setSaving(false);
    }
  };

  /** Reconnect one server so transport resolution picks up the latest package. */
  const handleUpdateOne = async (update: ServerPendingUpdate) => {
    const rowKey = pendingUpdateKey(update);
    setUpdatingServerKey(rowKey);
    try {
      await updateServerPackage(update.spaceId, update.serverId);
      onSuccess?.(
        `Updated ${update.name}`,
        `Reconnecting with v${update.latestVersion}…`
      );
      await refreshPendingUpdates();
    } catch (err) {
      console.error('[Settings] Failed to update server:', err);
      onError?.(`Failed to update ${update.name}`, String(err));
    } finally {
      setUpdatingServerKey(null);
    }
  };

  /** Reconnect every enabled server that has a pending package update. */
  const handleUpdateAll = async () => {
    const targets = pendingUpdates.filter((update) => update.enabled);
    if (targets.length === 0) {
      onInfo?.('No enabled servers to update', 'Enable a server first, then update.');
      return;
    }

    setUpdatingAll(true);
    let succeeded = 0;
    const failures: string[] = [];

    for (const update of targets) {
      try {
        await updateServerPackage(update.spaceId, update.serverId);
        succeeded += 1;
      } catch (err) {
        failures.push(update.name);
        console.error(`[Settings] Failed to update ${update.name}:`, err);
      }
    }

    await refreshPendingUpdates();
    setUpdatingAll(false);

    if (failures.length === 0) {
      onSuccess?.(
        `${succeeded} server${succeeded === 1 ? '' : 's'} updated`,
        'Reconnecting with latest packages…'
      );
      return;
    }

    if (succeeded > 0) {
      onInfo?.(
        `${succeeded} of ${targets.length} servers updated`,
        `Failed: ${failures.join(', ')}`
      );
      return;
    }

    onError?.('All updates failed', failures.join(', '));
  };

  /** Trigger a bulk npm/uv version probe across eligible servers. */
  const handleCheckAll = async () => {
    setCheckingAll(true);
    try {
      const result = await checkAllServerUpdates();
      setSettings((current) => ({
        ...current,
        lastCheckedAt: result.checkedAt,
      }));
      await refreshPendingUpdates();

      if (result.checked === 0) {
        onInfo?.('No eligible servers', 'Only npx/uvx servers with notify policy are probed.');
      } else if (result.updatesAvailable > 0) {
        onInfo?.(
          `${result.updatesAvailable} update${result.updatesAvailable === 1 ? '' : 's'} available`,
          'Update from the list below.'
        );
      } else {
        onSuccess?.(
          'All servers are up to date',
          `Checked ${result.checked} server${result.checked === 1 ? '' : 's'}.`
        );
      }
    } catch (err) {
      console.error('[Settings] Failed to check all server updates:', err);
      onError?.('Version check failed', String(err));
    } finally {
      setCheckingAll(false);
    }
  };

  const lastCheckedLabel = formatCheckedAt(settings.lastCheckedAt);

  return (
    <Card data-testid="settings-server-updates-section">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Package className="h-5 w-5" />
          Server updates
        </CardTitle>
        <CardDescription>
          Control how McpMux handles package updates for npx/uvx servers.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {loading ? (
          <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading…
          </div>
        ) : (
          <>
            <div className="flex items-center justify-between gap-4">
              <div className="flex-1 min-w-0">
                <label className="text-sm font-medium" htmlFor="default-update-policy">
                  Default update policy
                </label>
                <p className="text-xs text-[rgb(var(--muted))] mt-1">
                  {
                    policyOptions.find((option) => option.value === settings.defaultUpdatePolicy)
                      ?.description
                  }
                </p>
              </div>
              <select
                id="default-update-policy"
                value={settings.defaultUpdatePolicy}
                onChange={(e) => handlePolicyChange(e.target.value as UpdatePolicy)}
                disabled={saving}
                className="px-3 py-1.5 text-sm border border-[rgb(var(--border))] rounded-lg bg-[rgb(var(--surface))] text-[rgb(var(--foreground))]"
                data-testid="default-update-policy-select"
              >
                {policyOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="flex items-center justify-between gap-4 border-t border-[rgb(var(--border-subtle))] pt-4">
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium">Check for updates</p>
                <p className="text-xs text-[rgb(var(--muted))] mt-1">
                  {lastCheckedLabel ? `Last checked: ${lastCheckedLabel}` : 'Never checked'}
                </p>
              </div>
              <button
                type="button"
                onClick={handleCheckAll}
                disabled={checkingAll}
                className="inline-flex items-center gap-2 px-3 py-1.5 text-sm rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))] disabled:opacity-50"
                data-testid="check-all-server-updates-btn"
              >
                {checkingAll ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="h-4 w-4" />
                )}
                Check all
              </button>
            </div>

            {loadingPending ? (
              <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))]">
                <Loader2 className="h-4 w-4 animate-spin" />
                Loading pending updates…
              </div>
            ) : (
              <ServerPendingUpdatesList
                updates={pendingUpdates}
                updatingServerKey={updatingServerKey}
                updatingAll={updatingAll}
                onUpdateOne={handleUpdateOne}
                onUpdateAll={handleUpdateAll}
              />
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
