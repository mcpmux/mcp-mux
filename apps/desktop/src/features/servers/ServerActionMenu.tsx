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
  /** Show "Add another account…" for registry/manual installs (not clones-of-clones). */
  canCloneAccount?: boolean;
  onConfigure: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onUpdateNow?: () => void;
  onCheckForUpdate?: () => void;
  onLockToCurrentVersion?: () => void;
  onViewLogs: () => void;
  onViewDefinition: () => void;
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
  canCloneAccount = false,
  onConfigure,
  onRefresh,
  onReconnect,
  onUpdateNow,
  onCheckForUpdate,
  onLockToCurrentVersion,
  onViewLogs,
  onViewDefinition,
  onCloneAccount,
  onUninstall,
}: ServerActionMenuProps) {
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
    ? `Update available: v${latestVersion}`
    : 'Update available';

  return (
    <DropdownMenu>
      <DropdownMenuTrigger>
        <button
          type="button"
          className="relative p-2 text-sm rounded-lg bg-[rgb(var(--surface-hover))] border border-[rgb(var(--border))] text-[rgb(var(--foreground))]/70 hover:bg-[rgb(var(--surface-elevated))] hover:text-[rgb(var(--foreground))] transition-colors"
          title="More actions"
          aria-label="More actions"
          data-testid={`action-menu-${serverId}`}
        >
          <MoreVertical className="h-4 w-4" />
          {hasUpdateAvailable && (
            <span
              className="absolute top-1 right-1 h-2 w-2 rounded-full bg-amber-400 ring-2 ring-[rgb(var(--surface-hover))]"
              aria-label="Update available"
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
          label={hasInputs ? 'Configure' : 'Settings'}
          onSelect={onConfigure}
          data-testid={`configure-server-${serverId}`}
        />
        {isEnabled && (
          <DropdownMenuAction icon={RefreshCw} label="Refresh" onSelect={onRefresh} />
        )}
        {showUpdateNow && !hasUpdateAvailable && (
          <DropdownMenuAction
            icon={Download}
            label="Update now"
            onSelect={onUpdateNow}
            data-testid={`update-now-${serverId}`}
          />
        )}
        {showCheckForUpdate && (
          <DropdownMenuAction
            icon={Search}
            label="Check for update"
            onSelect={onCheckForUpdate}
            data-testid={`check-update-${serverId}`}
          />
        )}
        {showLockToCurrentVersion && (
          <DropdownMenuAction
            icon={Lock}
            label="Lock to current version"
            onSelect={onLockToCurrentVersion}
            data-testid={`lock-version-${serverId}`}
          />
        )}
        {isOAuth && isEnabled && (
          <DropdownMenuAction
            icon={RotateCcw}
            label="Reconnect"
            onSelect={onReconnect}
            variant="warning"
          />
        )}
        <DropdownMenuAction
          icon={FileText}
          label="View logs"
          onSelect={onViewLogs}
          data-testid={`view-logs-${serverId}`}
        />
        <DropdownMenuAction
          icon={Code}
          label="View definition"
          onSelect={onViewDefinition}
          data-testid={`view-definition-${serverId}`}
        />
        {canCloneAccount && onCloneAccount && (
          <DropdownMenuAction
            icon={Copy}
            label="Add another account…"
            onSelect={onCloneAccount}
            data-testid={`clone-account-${serverId}`}
          />
        )}
        <DropdownMenuSeparator />
        <DropdownMenuAction
          icon={Trash2}
          label="Uninstall"
          onSelect={onUninstall}
          variant="danger"
          data-testid={`uninstall-menu-${serverId}`}
        />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
