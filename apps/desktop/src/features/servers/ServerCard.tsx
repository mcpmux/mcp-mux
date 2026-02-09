/**
 * ServerCard - Event-driven server card component
 *
 * Uses the new ServerManager for:
 * - Real-time status updates via events
 * - Connect/Reconnect/Cancel button logic
 * - Auth progress display during OAuth
 */

import { Loader2, AlertCircle, Clock, Wifi, WifiOff } from "lucide-react";
import type { ServerViewModel } from "../../types/registry";
import type { ConnectionStatus } from "@/lib/api/serverManager";

interface ServerCardProps {
  server: ServerViewModel;
  /** Runtime status from ServerManager (overrides server.connection_status if provided) */
  runtimeStatus?: ConnectionStatus;
  /** Whether user has connected before (for Connect vs Reconnect label) */
  hasConnectedBefore?: boolean;
  /** Auth progress remaining seconds (during OAuth) */
  authRemainingSeconds?: number;
  /** Loading state for actions */
  isLoading?: boolean;
  /** Handlers */
  onEnable?: () => void;
  onDisable?: () => void;
  onConnect?: () => void;
  onCancel?: () => void;
  onRetry?: () => void;
  onConfigure?: () => void;
  onUninstall?: () => void;
}

export function ServerCard({
  server,
  runtimeStatus,
  hasConnectedBefore = false,
  authRemainingSeconds,
  isLoading = false,
  onEnable,
  onDisable,
  onConnect,
  onCancel,
  onRetry,
  onConfigure,
  onUninstall,
}: ServerCardProps) {
  // Determine effective status
  const status: ConnectionStatus = runtimeStatus ?? (server.enabled ? "disconnected" : "disconnected");

  // Status display helpers
  const getStatusIcon = () => {
    switch (status) {
      case "connected":
        return <Wifi className="w-4 h-4 text-[rgb(var(--success))]" />;
      case "connecting":
      case "refreshing":
        return <Loader2 className="w-4 h-4 text-[rgb(var(--primary))] animate-spin" />;
      case "authenticating":
        return <Clock className="w-4 h-4 text-[rgb(var(--warning))]" />;
      case "oauth_required":
        return <AlertCircle className="w-4 h-4 text-[rgb(var(--warning))]" />;
      case "error":
        return <AlertCircle className="w-4 h-4 text-[rgb(var(--error))]" />;
      default:
        return <WifiOff className="w-4 h-4 text-[rgb(var(--muted))]" />;
    }
  };

  const getStatusText = () => {
    switch (status) {
      case "connected":
        return "Connected";
      case "connecting":
        return "Connecting...";
      case "refreshing":
        return "Refreshing...";
      case "authenticating":
        return authRemainingSeconds !== undefined
          ? `Authenticating... (${Math.ceil(authRemainingSeconds / 60)}m remaining)`
          : "Authenticating...";
      case "oauth_required":
        return "Authentication required";
      case "error":
        return server.last_error ?? "Connection error";
      default:
        return server.enabled ? "Disabled" : "Not enabled";
    }
  };

  const getStatusColor = () => {
    switch (status) {
      case "connected":
        return "text-[rgb(var(--success))]";
      case "connecting":
      case "refreshing":
        return "text-[rgb(var(--primary))]";
      case "authenticating":
      case "oauth_required":
        return "text-[rgb(var(--warning))]";
      case "error":
        return "text-[rgb(var(--error))]";
      default:
        return "text-[rgb(var(--muted))]";
    }
  };

  // Primary action button
  const renderPrimaryAction = () => {
    if (!server.enabled) {
      return (
        <button
          onClick={onEnable}
          disabled={isLoading}
          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] transition-colors disabled:opacity-50"
        >
          {isLoading ? "Enabling..." : "Enable"}
        </button>
      );
    }

    // Check for missing required inputs
    if (server.missing_required_inputs && onConfigure) {
      return (
        <button
          onClick={onConfigure}
          disabled={isLoading}
          className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--warning))] text-white hover:bg-[rgb(var(--warning))]/90 transition-colors disabled:opacity-50"
        >
          Configure
        </button>
      );
    }

    switch (status) {
      case "connecting":
      case "refreshing":
        return (
          <button
            disabled
            className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--surface-elevated))] text-[rgb(var(--muted))] cursor-not-allowed flex items-center gap-2"
          >
            <Loader2 className="w-4 h-4 animate-spin" />
            {status === "refreshing" ? "Refreshing..." : "Connecting..."}
          </button>
        );

      case "connected":
        return (
          <button
            onClick={onDisable}
            disabled={isLoading}
            className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors disabled:opacity-50"
          >
            Disconnect
          </button>
        );

      case "oauth_required":
        return (
          <button
            onClick={onConnect}
            disabled={isLoading}
            className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--success))] text-white hover:bg-[rgb(var(--success))]/90 transition-colors disabled:opacity-50"
          >
            {isLoading ? "Connecting..." : hasConnectedBefore ? "Reconnect" : "Connect"}
          </button>
        );

      case "authenticating":
        return (
          <div className="flex items-center gap-2">
            <button
              disabled
              className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--warning))] text-white cursor-not-allowed flex items-center gap-2"
            >
              <Loader2 className="w-4 h-4 animate-spin" />
              Authenticating...
            </button>
            <button
              onClick={onCancel}
              className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
            >
              Cancel
            </button>
          </div>
        );

      case "error":
        return (
          <button
            onClick={onRetry ?? onConnect}
            disabled={isLoading}
            className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--error))] text-white hover:bg-[rgb(var(--error))]/90 transition-colors disabled:opacity-50"
          >
            {isLoading ? "Retrying..." : hasConnectedBefore ? "Reconnect" : "Retry"}
          </button>
        );

      default:
        // Disconnected but enabled - try to connect
        return (
          <button
            onClick={onConnect}
            disabled={isLoading}
            className="px-4 py-2 text-sm rounded-lg bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] hover:bg-[rgb(var(--primary-hover))] transition-colors disabled:opacity-50"
          >
            Connect
          </button>
        );
    }
  };

  return (
    <div className="p-4 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-elevated))] hover:border-[rgb(var(--border))] transition-colors">
      <div className="flex items-center justify-between">
        {/* Server Info */}
        <div className="flex items-center gap-4 min-w-0 flex-1">
          {/* Icon */}
          <div className="w-10 h-10 rounded-lg bg-[rgb(var(--surface-dim))] flex items-center justify-center flex-shrink-0 text-xl">
            {server.icon?.startsWith('http') ? (
              <img src={server.icon} alt="" className="w-7 h-7 object-contain" onError={(e) => { e.currentTarget.style.display = 'none'; e.currentTarget.parentElement!.append(document.createTextNode('📦')); }} />
            ) : (
              server.icon || "🔌"
            )}
          </div>

          {/* Name & Status */}
          <div className="min-w-0 flex-1">
            <h3 className="font-semibold text-[rgb(var(--foreground))] truncate">
              {server.name}
            </h3>
            <div className={`flex items-center gap-2 text-sm ${getStatusColor()}`}>
              {getStatusIcon()}
              <span className="truncate">{getStatusText()}</span>
            </div>
          </div>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2 flex-shrink-0">
          {renderPrimaryAction()}

          {/* Disable button (if enabled and not the primary action) */}
          {server.enabled && status !== "connected" && status !== "disconnected" && (
            <button
              onClick={onDisable}
              disabled={isLoading}
              className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--border))] text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))] transition-colors disabled:opacity-50"
            >
              Disable
            </button>
          )}

          {/* Uninstall button */}
          {onUninstall && (
            <button
              onClick={onUninstall}
              disabled={isLoading}
              className="px-4 py-2 text-sm rounded-lg border border-[rgb(var(--error))]/30 text-[rgb(var(--error))] hover:bg-[rgb(var(--error))]/10 transition-colors disabled:opacity-50"
            >
              Uninstall
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
