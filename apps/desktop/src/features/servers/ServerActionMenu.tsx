/**
 * ServerActionMenu - Overflow menu for server actions
 * 
 * Actions:
 * - Configure: Edit server inputs
 * - Refresh: Quick reconnect with existing credentials
 * - Reconnect: Logout + re-authenticate (OAuth only)
 * - View Logs: Open log viewer
 * - View Definition: View server definition JSON
 * - Uninstall: Remove server
 */

import { useState, useRef, useEffect } from 'react';
import { MoreVertical, Settings, RefreshCw, RotateCcw, FileText, Code, Trash2 } from 'lucide-react';

export interface ServerActionMenuProps {
  serverId: string;
  serverName: string;
  hasInputs: boolean;
  isOAuth: boolean;
  isEnabled: boolean;
  isConnected: boolean;
  onConfigure: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onViewLogs: () => void;
  onViewDefinition: () => void;
  onUninstall: () => void;
}

export function ServerActionMenu({
  serverId,
  serverName: _serverName,
  hasInputs,
  isOAuth,
  isEnabled,
  isConnected: _isConnected,
  onConfigure,
  onRefresh,
  onReconnect,
  onViewLogs,
  onViewDefinition,
  onUninstall,
}: ServerActionMenuProps) {
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  // Close menu when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (
        menuRef.current &&
        !menuRef.current.contains(event.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    }

    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);
      return () => document.removeEventListener('mousedown', handleClickOutside);
    }
  }, [isOpen]);

  // Close menu on escape
  useEffect(() => {
    function handleEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    }

    if (isOpen) {
      document.addEventListener('keydown', handleEscape);
      return () => document.removeEventListener('keydown', handleEscape);
    }
  }, [isOpen]);

  const handleAction = (action: () => void) => {
    setIsOpen(false);
    action();
  };

  return (
    <div className="relative">
      <button
        ref={buttonRef}
        onClick={() => setIsOpen(!isOpen)}
        className="p-2 text-sm rounded-lg bg-[rgb(var(--surface-hover))] border border-[rgb(var(--border))] text-[rgb(var(--foreground))]/70 hover:bg-[rgb(var(--surface-elevated))] hover:text-[rgb(var(--foreground))] transition-colors"
        title="More actions"
        aria-label="More actions"
        aria-expanded={isOpen}
        aria-haspopup="menu"
        data-testid={`action-menu-${serverId}`}
      >
        <MoreVertical className="h-4 w-4" />
      </button>

      {isOpen && (
        <div
          ref={menuRef}
          className="absolute right-0 mt-1 w-48 py-1 bg-[rgb(var(--surface-elevated))] border border-[rgb(var(--border))] rounded-lg shadow-lg z-50 animate-in fade-in slide-in-from-top-1 duration-150"
          role="menu"
        >
          {/* Configure - visible if server has inputs */}
          {hasInputs && (
            <button
              onClick={() => handleAction(onConfigure)}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
              role="menuitem"
            >
              <Settings className="h-4 w-4 text-[rgb(var(--muted))]" />
              Configure
            </button>
          )}

          {/* Refresh - visible when enabled (quick reconnect with existing creds) */}
          {isEnabled && (
            <button
              onClick={() => handleAction(onRefresh)}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
              role="menuitem"
            >
              <RefreshCw className="h-4 w-4 text-[rgb(var(--muted))]" />
              Refresh
            </button>
          )}

          {/* Reconnect - OAuth only (logout + re-auth) */}
          {isOAuth && isEnabled && (
            <button
              onClick={() => handleAction(onReconnect)}
              className="w-full flex items-center gap-2 px-3 py-2 text-sm text-[rgb(var(--warning))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
              role="menuitem"
            >
              <RotateCcw className="h-4 w-4" />
              Reconnect
            </button>
          )}

          {/* View Logs - always visible */}
          <button
            onClick={() => handleAction(onViewLogs)}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
            role="menuitem"
            data-testid={`view-logs-${serverId}`}
          >
            <FileText className="h-4 w-4 text-[rgb(var(--muted))]" />
            View Logs
          </button>

          {/* View Definition - always visible */}
          <button
            onClick={() => handleAction(onViewDefinition)}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
            role="menuitem"
            data-testid={`view-definition-${serverId}`}
          >
            <Code className="h-4 w-4 text-[rgb(var(--muted))]" />
            View Definition
          </button>

          {/* Separator */}
          <div className="my-1 border-t border-[rgb(var(--border-subtle))]" />

          {/* Uninstall - always visible, destructive */}
          <button
            onClick={() => handleAction(onUninstall)}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-[rgb(var(--error))] hover:bg-[rgb(var(--error))]/10 transition-colors"
            role="menuitem"
            data-testid={`uninstall-menu-${serverId}`}
          >
            <Trash2 className="h-4 w-4" />
            Uninstall
          </button>
        </div>
      )}
    </div>
  );
}
