import { useCallback, useEffect, useState } from 'react';
import {
  AppWindow,
  Check,
  Copy,
  Download,
  Loader2,
  ShieldCheck,
  ShieldOff,
  AlertCircle,
} from 'lucide-react';
import { Button } from '@mcpmux/ui';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import claudeIcon from '@/assets/client-icons/claude.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import opencodeIcon from '@/assets/client-icons/opencode.svg';
import opencodeIconDark from '@/assets/client-icons/opencode-dark.svg';
import zedIcon from '@/assets/client-icons/zed.svg';
import { ClientBrandIcon } from '@/components/ClientBrandIcon';
import { getGatewayStatus } from '@/lib/api/gateway';
import { useNavigateTo, useSetPendingSettingsSection } from '@/stores';

/** Brand icon per supported client id (falls back to a generic glyph). opencode
 *  ships theme-specific marks, so it carries a dark variant. */
const CLIENT_ICONS: Record<string, { light: string; dark?: string }> = {
  cursor: { light: cursorIcon },
  'claude-code': { light: claudeIcon },
  vscode: { light: vscodeIcon },
  opencode: { light: opencodeIcon, dark: opencodeIconDark },
  zed: { light: zedIcon },
};
import {
  generateWorkspaceConfigSnippet,
  getGatewayAuthDisabled,
  installWorkspaceMcpConfig,
  listWorkspaceInstallClients,
  type WorkspaceInstallClient,
  type WorkspaceInstallResult,
} from '@/lib/api/workspaceInstall';

/** Clients selected by default the first time, before the user picks. */
const DEFAULT_SELECTED = ['cursor', 'claude-code', 'vscode'];

/** Where the last client selection is remembered across folders/sessions. */
const SELECTION_STORAGE_KEY = 'mcpmux:workspace-install-clients';

/** Read the remembered client selection, or null when none/invalid. */
function loadSavedSelection(): Set<string> | null {
  try {
    const raw = localStorage.getItem(SELECTION_STORAGE_KEY);
    if (!raw) return null;
    const arr: unknown = JSON.parse(raw);
    if (Array.isArray(arr) && arr.every((x) => typeof x === 'string')) {
      return new Set(arr as string[]);
    }
  } catch {
    /* ignore corrupt / unavailable storage */
  }
  return null;
}

function saveSelection(ids: Set<string>) {
  try {
    localStorage.setItem(SELECTION_STORAGE_KEY, JSON.stringify(Array.from(ids)));
  } catch {
    /* ignore */
  }
}

/**
 * "Connect apps to this folder" — writes (or extends) project-local MCP configs
 * inside `workspaceRoot`, injecting `X-Mcpmux-Workspace: <folder path>` so the
 * gateway routes those apps to this folder's binding deterministically, even
 * when the client doesn't report MCP roots. Also surfaces (and can flip) the
 * system-wide auth toggle inline, since disabling it makes the config a pure
 * URL + header with no access key.
 */
export function WorkspaceInstallPanel({ workspaceRoot }: { workspaceRoot: string }) {
  const [clients, setClients] = useState<WorkspaceInstallClient[]>([]);
  // Restore the user's last selection (remembered across folders); fall back to
  // the common-three default the first time.
  const [selected, setSelected] = useState<Set<string>>(
    () => loadSavedSelection() ?? new Set(DEFAULT_SELECTED)
  );
  const [mcpUrl, setMcpUrl] = useState<string | null>(null);
  const [authDisabled, setAuthDisabled] = useState<boolean | null>(null);
  const [installing, setInstalling] = useState(false);
  const [results, setResults] = useState<WorkspaceInstallResult[] | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const navigateTo = useNavigateTo();
  const setPendingSettingsSection = useSetPendingSettingsSection();

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [list, status, disabled] = await Promise.all([
          listWorkspaceInstallClients(),
          getGatewayStatus().catch(() => ({ running: false, url: null as string | null })),
          getGatewayAuthDisabled().catch(() => false),
        ]);
        if (cancelled) return;
        setClients(list);
        // Drop any remembered ids that aren't supported anymore; if that
        // leaves nothing, fall back to the defaults that do exist.
        setSelected((prev) => {
          const known = new Set(list.map((c) => c.id));
          const pruned = [...prev].filter((id) => known.has(id));
          return new Set(pruned.length ? pruned : DEFAULT_SELECTED.filter((id) => known.has(id)));
        });
        setAuthDisabled(disabled);
        setMcpUrl(status.url ? `${status.url}/mcp` : null);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Remember the selection across folders and sessions.
  useEffect(() => {
    saveSelection(selected);
  }, [selected]);

  const toggleClient = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    setResults(null);
  };

  const handleCopy = useCallback(
    async (clientId: string) => {
      if (!mcpUrl) return;
      try {
        const snip = await generateWorkspaceConfigSnippet({
          client: clientId,
          serverUrl: mcpUrl,
          workspaceRoot,
        });
        await navigator.clipboard.writeText(snip.content);
        setCopiedId(clientId);
        setTimeout(() => setCopiedId((c) => (c === clientId ? null : c)), 1500);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [mcpUrl, workspaceRoot]
  );

  const handleInstall = async () => {
    if (!mcpUrl || selected.size === 0) return;
    setInstalling(true);
    setError(null);
    setResults(null);
    try {
      const res = await installWorkspaceMcpConfig({
        workspaceRoot,
        serverUrl: mcpUrl,
        clients: Array.from(selected),
      });
      setResults(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="space-y-4" data-testid="workspace-install-panel">
      <p className="text-sm text-[rgb(var(--muted))]">
        Add McpMux to this folder&apos;s MCP config for the apps you use. Each gets an{' '}
        <code className="text-xs">X-Mcpmux-Workspace</code> header set to this path, so it routes
        here automatically — even apps that don&apos;t report the folder.
      </p>

      {/* Self-introductory auth nudge — disabling auth makes the written config
          a pure URL + header with no access key to manage. */}
      {authDisabled === false && (
        <div
          className="flex items-start gap-2.5 rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs dark:border-amber-800/60 dark:bg-amber-900/20"
          data-testid="workspace-install-auth-nudge"
        >
          <ShieldOff className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
          <div className="min-w-0 flex-1">
            <p className="text-amber-800 dark:text-amber-300">
              Enable and authenticate this app once to connect — or disable the requirement in
              Settings.
            </p>
            <Button
              variant="secondary"
              size="sm"
              className="mt-2 h-7 text-xs"
              onClick={() => {
                setPendingSettingsSection('security');
                navigateTo('settings');
              }}
              data-testid="workspace-install-open-auth-settings"
            >
              <ShieldOff className="mr-1.5 h-3 w-3" />
              Open Settings
            </Button>
          </div>
        </div>
      )}
      {authDisabled === true && (
        <div className="flex items-center gap-2 rounded-lg border border-emerald-200 bg-emerald-50 p-2.5 text-xs text-emerald-700 dark:border-emerald-800/60 dark:bg-emerald-900/20 dark:text-emerald-300">
          <ShieldCheck className="h-4 w-4 flex-shrink-0" />
          Authentication is off — apps connect with just the URL and workspace header.
        </div>
      )}

      {/* Client checklist with per-row copy. */}
      <div className="overflow-hidden rounded-lg border border-[rgb(var(--border))]">
        {clients.map((c, i) => {
          const checked = selected.has(c.id);
          return (
            <label
              key={c.id}
              className={`flex cursor-pointer items-center gap-3 px-3 py-2.5 transition-colors hover:bg-[rgb(var(--surface-hover))] ${
                i > 0 ? 'border-t border-[rgb(var(--border-subtle))]' : ''
              }`}
              data-testid={`workspace-install-client-${c.id}`}
            >
              <input
                type="checkbox"
                checked={checked}
                onChange={() => toggleClient(c.id)}
                className="h-4 w-4 flex-shrink-0 accent-primary-500"
              />
              {CLIENT_ICONS[c.id] ? (
                <ClientBrandIcon
                  light={CLIENT_ICONS[c.id].light}
                  dark={CLIENT_ICONS[c.id].dark}
                  className="h-5 w-5 flex-shrink-0 object-contain"
                />
              ) : (
                <AppWindow className="h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
              )}
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium text-[rgb(var(--foreground))]">{c.label}</div>
                <div className="truncate font-mono text-[11px] text-[rgb(var(--muted))]">
                  {c.config_path}
                </div>
              </div>
              <button
                type="button"
                title="Copy this client's config"
                disabled={!mcpUrl}
                onClick={(e) => {
                  e.preventDefault();
                  void handleCopy(c.id);
                }}
                className="flex-shrink-0 rounded-md p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--foreground))] disabled:opacity-40"
                data-testid={`workspace-install-copy-${c.id}`}
              >
                {copiedId === c.id ? (
                  <Check className="h-3.5 w-3.5 text-green-600" />
                ) : (
                  <Copy className="h-3.5 w-3.5" />
                )}
              </button>
            </label>
          );
        })}
      </div>

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-red-200 bg-red-50 p-2.5 text-xs text-red-600 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400">
          <AlertCircle className="mt-0.5 h-3.5 w-3.5 flex-shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {results && (
        <div className="space-y-1.5" data-testid="workspace-install-results">
          {results.map((r) => (
            <div
              key={r.client}
              className="flex items-center gap-2 rounded-md border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-2.5 py-1.5 text-xs"
            >
              {r.action === 'error' ? (
                <AlertCircle className="h-3.5 w-3.5 flex-shrink-0 text-red-500" />
              ) : (
                <Check className="h-3.5 w-3.5 flex-shrink-0 text-green-600" />
              )}
              <span className="font-medium">{r.label}</span>
              <span className="text-[rgb(var(--muted))]">
                {r.action === 'error' ? r.error : `${r.action} ${r.path}`}
              </span>
            </div>
          ))}
        </div>
      )}

      <Button
        variant="primary"
        size="sm"
        className="w-full"
        disabled={installing || selected.size === 0 || !mcpUrl}
        onClick={handleInstall}
        data-testid="workspace-install-button"
      >
        {installing ? (
          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        ) : (
          <Download className="mr-2 h-4 w-4" />
        )}
        {mcpUrl
          ? `Install into ${selected.size} app${selected.size === 1 ? '' : 's'}`
          : 'Start the gateway to install'}
      </Button>
    </div>
  );
}
