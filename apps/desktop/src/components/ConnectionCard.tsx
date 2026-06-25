import { useCallback, useEffect, useState } from 'react';
import {
  ArrowRight,
  Bell,
  Check,
  Copy,
  Loader2,
  Lock,
  Power,
  Sliders,
} from 'lucide-react';
import { Card, Button } from '@mcpmux/ui';
import { useViewSpace, useNavigateTo, useSetPendingSettingsSection } from '@/stores';
import { useGatewayControl } from '@/features/gateway/useGatewayControl';
import { useGatewayEvents } from '@/hooks/useDomainEvents';
import {
  getGatewayStatus,
  listOAuthClients,
  stopGateway,
} from '@/lib/api/gateway';
import { ConnectIDEsGrid } from './ConnectIDEs';

const FALLBACK_URL = 'http://localhost:45818';

function extractPort(url: string | null): string {
  try {
    const u = new URL(url ?? FALLBACK_URL);
    return u.port || '45818';
  } catch {
    return '45818';
  }
}

/**
 * Canonical "how do I connect to McpMux" surface. Owns the gateway URL + port
 * display, Start/Stop, the IDE connect grid, and the pending-approval nudge.
 * Everything else in the app (sidebar footer, status bar) should reduce to a
 * compact status pill rather than repeating the URL.
 */
export function ConnectionCard() {
  const viewSpace = useViewSpace();
  const navigateTo = useNavigateTo();
  const setPendingSettingsSection = useSetPendingSettingsSection();
  const gatewayControl = useGatewayControl();

  const [status, setStatus] = useState<{ running: boolean; url: string | null }>({
    running: false,
    url: null,
  });
  const [pendingApprovals, setPendingApprovals] = useState(0);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);

  const displayUrl = status.url ?? FALLBACK_URL;
  const mcpUrl = `${displayUrl}/mcp`;
  const port = extractPort(status.url);

  const reloadStatus = useCallback(async () => {
    try {
      const s = await getGatewayStatus(viewSpace?.id);
      setStatus({ running: s.running, url: s.url });
    } catch {
      /* keep previous status */
    }
  }, [viewSpace?.id]);

  const reloadApprovals = useCallback(async () => {
    try {
      const clients = await listOAuthClients();
      setPendingApprovals(clients.filter((c) => !c.approved).length);
    } catch {
      setPendingApprovals(0);
    }
  }, []);

  useEffect(() => {
    reloadStatus();
    reloadApprovals();
  }, [reloadStatus, reloadApprovals]);

  // Live gateway state — no polling, driven by the event bus.
  useGatewayEvents((payload) => {
    if (payload.action === 'started') {
      setStatus({ running: true, url: payload.url || null });
      reloadApprovals();
    } else if (payload.action === 'stopped') {
      setStatus({ running: false, url: null });
    }
  });

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(mcpUrl);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      console.error('[ConnectionCard] copy failed', e);
    }
  };

  const handleToggle = async () => {
    if (busy) return;
    setBusy(true);
    try {
      if (status.running) {
        await stopGateway();
        setStatus({ running: false, url: null });
      } else {
        const outcome = await gatewayControl.start();
        if (outcome.status !== 'cancelled') {
          setStatus({ running: true, url: outcome.url });
        }
      }
    } catch (e) {
      console.error('[ConnectionCard] toggle failed', e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {gatewayControl.ConfirmDialogElement}
      <Card className="relative overflow-hidden p-0" data-testid="gateway-status-card">
      {/* Hairline gradient — present on both states, brighter when running.
          Gives the hero card a subtle sense of depth without a heavy header
          background. */}
      <div
        className={`absolute inset-x-0 top-0 h-px transition-opacity ${
          status.running
            ? 'bg-gradient-to-r from-transparent via-primary-400/70 to-transparent opacity-100'
            : 'bg-gradient-to-r from-transparent via-[rgb(var(--border))] to-transparent opacity-60'
        }`}
      />

      {/* Top bar — status + primary action */}
      <div className="flex items-center justify-between gap-4 px-6 py-4 border-b border-[rgb(var(--border-subtle))]">
        <div className="flex items-center gap-3 min-w-0">
          <StatusDot running={status.running} />
          <div className="min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span
                className="text-sm font-semibold text-[rgb(var(--card-foreground))]"
                data-testid="connection-status-text"
              >
                {status.running ? 'Gateway running' : 'Gateway stopped'}
              </span>
              {status.running && (
                <span className="inline-flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-[rgb(var(--muted))] px-1.5 py-0.5 rounded-md bg-[rgb(var(--surface))] border border-[rgb(var(--border-subtle))]">
                  <Lock className="h-2.5 w-2.5" />
                  Local only
                </span>
              )}
            </div>
            <p className="text-xs text-[rgb(var(--muted))] mt-0.5 truncate">
              {status.running
                ? 'Accepting IDE connections on this device.'
                : 'Start the gateway to let IDEs connect through McpMux.'}
            </p>
          </div>
        </div>
        <Button
          variant={status.running ? 'secondary' : 'primary'}
          size="sm"
          onClick={handleToggle}
          disabled={busy}
          data-testid="gateway-toggle-btn"
        >
          {busy ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Power className="h-3.5 w-3.5" />
          )}
          {status.running ? 'Stop' : 'Start'}
        </Button>
      </div>

      <div className="px-6 py-5 space-y-5">
        {/* Endpoint — the canonical address users paste into clients. */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <label className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[rgb(var(--muted))]">
              Endpoint
            </label>
            <button
              type="button"
              onClick={() => {
                // Land on (and flash) the Gateway section where the port lives.
                setPendingSettingsSection('gateway');
                navigateTo('settings');
              }}
              className="group inline-flex items-center gap-1 text-xs text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] transition-colors"
              data-testid="connection-port-settings-link"
            >
              <Sliders className="h-3 w-3" />
              Port {port}
              <span className="opacity-0 group-hover:opacity-100 transition-opacity">
                · change in Settings
              </span>
            </button>
          </div>

          <button
            type="button"
            onClick={handleCopy}
            className={`group relative w-full flex items-center gap-3 rounded-lg border transition-all overflow-hidden
              ${
                status.running
                  ? 'border-[rgb(var(--border))] bg-[rgb(var(--surface))] hover:border-primary-400/70 hover:bg-primary-500/[0.04] focus-visible:border-primary-500/70 focus-visible:ring-2 focus-visible:ring-primary-500/30'
                  : 'border-dashed border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]/40 hover:border-[rgb(var(--border))]'
              }
              px-4 py-3 text-left focus:outline-none`}
            data-testid="connection-url-copy-btn"
            title="Click to copy"
          >
            <code
              className="flex-1 font-mono text-sm truncate select-all text-[rgb(var(--card-foreground))]"
              data-testid="connection-url"
            >
              {mcpUrl}
            </code>
            <span
              className={`flex items-center gap-1.5 text-xs font-medium transition-colors ${
                copied
                  ? 'text-green-600 dark:text-green-400'
                  : 'text-[rgb(var(--muted))] group-hover:text-[rgb(var(--primary))]'
              }`}
            >
              {copied ? (
                <>
                  <Check className="h-3.5 w-3.5" />
                  Copied
                </>
              ) : (
                <>
                  <Copy className="h-3.5 w-3.5" />
                  Copy
                </>
              )}
            </span>
          </button>
        </div>

        {/* Pending approvals — surfaces only when a client is waiting. The
            canonical "approve this connection" UI still lives in the Clients
            page; this is a nudge so users don't miss pending work. */}
        {pendingApprovals > 0 && (
          <button
            type="button"
            onClick={() => navigateTo('clients')}
            className="w-full flex items-center justify-between gap-3 rounded-lg border border-amber-300/60 dark:border-amber-700/60 bg-amber-50 dark:bg-amber-900/20 px-4 py-2.5 text-left hover:bg-amber-100/80 dark:hover:bg-amber-900/30 transition-colors"
            data-testid="connection-pending-approvals"
          >
            <div className="flex items-center gap-2.5 min-w-0">
              <Bell className="h-4 w-4 text-amber-600 dark:text-amber-400 flex-shrink-0" />
              <div className="min-w-0">
                <p className="text-sm font-medium text-amber-900 dark:text-amber-100">
                  {pendingApprovals} client{pendingApprovals === 1 ? '' : 's'} waiting for approval
                </p>
                <p className="text-xs text-amber-700 dark:text-amber-300 truncate">
                  Review and approve to let them through.
                </p>
              </div>
            </div>
            <ArrowRight className="h-4 w-4 text-amber-600 dark:text-amber-400 flex-shrink-0" />
          </button>
        )}

        {/* Connect a client — the grid reuses the chromeless ConnectIDEsGrid. */}
        <div className="pt-4 border-t border-[rgb(var(--border-subtle))]">
          <div className="mb-3">
            <p className="text-sm font-semibold text-[rgb(var(--card-foreground))]">
              Connect a client
            </p>
            <p className="text-xs text-[rgb(var(--muted))] mt-0.5">
              VS Code &amp; Cursor are one-click. The rest copy a config you paste into your IDE's
              MCP settings. Either path ends with an approval prompt here.
            </p>
          </div>
          <ConnectIDEsGrid gatewayUrl={displayUrl} gatewayRunning={status.running} />
        </div>
      </div>
    </Card>
    </>
  );
}

/**
 * Two-layer dot: solid circle + a halo that pulses while running. The pulse
 * gives ambient life to the "running" state without being a focal point.
 */
function StatusDot({ running }: { running: boolean }) {
  return (
    <span className="relative flex h-2.5 w-2.5 flex-shrink-0" aria-hidden="true">
      {running && (
        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-500 opacity-60" />
      )}
      <span
        className={`relative inline-flex h-2.5 w-2.5 rounded-full ${
          running
            ? 'bg-green-500 shadow-[0_0_0_3px_rgb(34_197_94_/_0.15)]'
            : 'bg-zinc-400 dark:bg-zinc-600'
        }`}
      />
    </span>
  );
}
