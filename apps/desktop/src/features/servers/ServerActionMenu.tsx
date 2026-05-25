import { MoreVertical, Settings, RefreshCw, RotateCcw, FileText, Code, Trash2, Copy } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuAction,
  DropdownMenuContent,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@mcpmux/ui';

export interface ServerActionMenuProps {
  serverId: string;
  serverName: string;
  /** Whether the server has credential / config inputs. Servers with no inputs still show
   *  Configure so the display name can be edited. */
  hasInputs: boolean;
  isOAuth: boolean;
  isEnabled: boolean;
  isConnected: boolean;
  /** Show "Add another account…" for registry/manual installs (not clones-of-clones). */
  canCloneAccount?: boolean;
  onConfigure: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
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
  canCloneAccount = false,
  onConfigure,
  onRefresh,
  onReconnect,
  onViewLogs,
  onViewDefinition,
  onCloneAccount,
  onUninstall,
}: ServerActionMenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger>
        <button
          type="button"
          className="p-2 text-sm rounded-lg bg-[rgb(var(--surface-hover))] border border-[rgb(var(--border))] text-[rgb(var(--foreground))]/70 hover:bg-[rgb(var(--surface-elevated))] hover:text-[rgb(var(--foreground))] transition-colors"
          title="More actions"
          aria-label="More actions"
          data-testid={`action-menu-${serverId}`}
        >
          <MoreVertical className="h-4 w-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48 py-1 p-1">
        <DropdownMenuAction
          icon={Settings}
          label={hasInputs ? 'Configure' : 'Settings'}
          onSelect={onConfigure}
        />
        {isEnabled && (
          <DropdownMenuAction icon={RefreshCw} label="Refresh" onSelect={onRefresh} />
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
          label="View Logs"
          onSelect={onViewLogs}
          data-testid={`view-logs-${serverId}`}
        />
        <DropdownMenuAction
          icon={Code}
          label="View Definition"
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
