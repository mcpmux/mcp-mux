import { useTranslation } from 'react-i18next';
import {
  MoreVertical,
  Settings,
  RefreshCw,
  RotateCcw,
  FileText,
  Code,
  Trash2,
  Copy,
  Download,
  ArrowUpCircle,
  Search,
  Lock,
} from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuAction,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@mcpmux/ui';
import type { UpdatePolicy } from '@/lib/api/settings';

export interface ServerActionMenuProps {
  serverId: string;
  serverName: string;
  /** Whether the server has credential / config inputs. Servers with no inputs still show
   *  Configure so the display name can be edited. */
  hasInputs: boolean;
  isOAuth: boolean;
  isEnabled: boolean;
  isConnected: boolean;
  /** npx/uvx stdio transport — eligible for package update actions. */
  isPackageManaged?: boolean;
  /** Per-server update policy from installed state. */
  updatePolicy?: UpdatePolicy;
  /** Whether a newer package version is available. */
  hasUpdateAvailable?: boolean;
  /** Latest registry version when an update is available. */
  latestVersion?: string | null;
  onConfigure: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onUpdateNow?: () => void;
  onCheckForUpdate?: () => void;
  onLockToCurrentVersion?: () => void;
  onViewLogs: () => void;
  onViewDefinition: () => void;
  /** Whether the server's config is stored locally and can be edited (UserSpace source). */
  canEditDefinition?: boolean;
  onCloneAccount?: () => void;
  onUninstall: () => void;
}

/**
 * Overflow menu for per-server actions (configure, logs, uninstall, etc.).
 */
export function ServerActionMenu({
  serverId,
  serverName: _serverName,
  hasInputs,
  isOAuth,
  isEnabled,
  isConnected: _isConnected,
  isPackageManaged = false,
  updatePolicy = 'notify',
  hasUpdateAvailable = false,
  latestVersion,
  onConfigure,
  onRefresh,
  onReconnect,
  onUpdateNow,
  onCheckForUpdate,
  onLockToCurrentVersion,
  onViewLogs,
  onViewDefinition,
  canEditDefinition = false,
  onCloneAccount,
  onUninstall,
}: ServerActionMenuProps) {
  const { t } = useTranslation('servers');
  const showUpdateNow =
    isPackageManaged &&
    isEnabled &&
    onUpdateNow != null &&
    (updatePolicy === 'auto' || hasUpdateAvailable);
  const showCheckForUpdate =
    isPackageManaged && onCheckForUpdate != null && updatePolicy !== 'pinned';
  const showLockToCurrentVersion =
    isPackageManaged && onLockToCurrentVersion != null && updatePolicy !== 'pinned';
  const updateLabel = latestVersion
    ? t('actions.updateAvailableVersion', { version: latestVersion })
    : t('actions.updateAvailable');

  return (
    <DropdownMenu>
      <DropdownMenuTrigger>
        <button
          type="button"
          className="relative p-2 text-sm rounded-lg bg-[rgb(var(--surface-hover))] border border-[rgb(var(--border))] text-[rgb(var(--foreground))]/70 hover:bg-[rgb(var(--surface-elevated))] hover:text-[rgb(var(--foreground))] transition-colors"
          title={t('actions.moreActions')}
          aria-label={t('actions.moreActions')}
          data-testid={`action-menu-${serverId}`}
        >
          <MoreVertical className="h-4 w-4" />
          {hasUpdateAvailable && (
            <span
              className="absolute top-1 right-1 h-2 w-2 rounded-full bg-amber-400 ring-2 ring-[rgb(var(--surface-hover))]"
              aria-label={t('actions.updateBadge')}
              data-testid={`update-badge-${serverId}`}
            />
          )}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56 py-1 p-1">
        {hasUpdateAvailable && showUpdateNow && (
          <DropdownMenuAction
            icon={ArrowUpCircle}
            label={updateLabel}
            onSelect={onUpdateNow}
            data-testid={`update-available-${serverId}`}
          />
        )}
        <DropdownMenuAction
          icon={Settings}
          label={hasInputs ? t('actions.configure') : t('actions.settings')}
          onSelect={onConfigure}
          data-testid={`configure-server-${serverId}`}
        />
        {isEnabled && (
          <DropdownMenuAction icon={RefreshCw} label={t('actions.refresh')} onSelect={onRefresh} />
        )}
        {showUpdateNow && !hasUpdateAvailable && (
          <DropdownMenuAction
            icon={Download}
            label={t('actions.updateNow')}
            onSelect={onUpdateNow}
            data-testid={`update-now-${serverId}`}
          />
        )}
        {showCheckForUpdate && (
          <DropdownMenuAction
            icon={Search}
            label={t('actions.checkForUpdate')}
            onSelect={onCheckForUpdate}
            data-testid={`check-update-${serverId}`}
          />
        )}
        {showLockToCurrentVersion && (
          <DropdownMenuAction
            icon={Lock}
            label={t('actions.lockVersion')}
            onSelect={onLockToCurrentVersion}
            data-testid={`lock-version-${serverId}`}
          />
        )}
        {isOAuth && isEnabled && (
          <DropdownMenuAction
            icon={RotateCcw}
            label={t('actions.reconnect')}
            onSelect={onReconnect}
            variant="warning"
          />
        )}
        <DropdownMenuAction
          icon={FileText}
          label={t('actions.viewLogs')}
          onSelect={onViewLogs}
          data-testid={`view-logs-${serverId}`}
        />
        <DropdownMenuAction
          icon={Code}
          label={canEditDefinition ? t('actions.editDefinition') : t('actions.viewDefinition')}
          onSelect={onViewDefinition}
          data-testid={`view-definition-${serverId}`}
        />
        {onCloneAccount && (
          <DropdownMenuAction
            icon={Copy}
            label={t('actions.cloneAccount')}
            onSelect={onCloneAccount}
            data-testid={`clone-account-${serverId}`}
          />
        )}
        <DropdownMenuSeparator />
        <DropdownMenuAction
          icon={Trash2}
          label={t('actions.uninstall')}
          onSelect={onUninstall}
          variant="danger"
          data-testid={`uninstall-menu-${serverId}`}
        />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
