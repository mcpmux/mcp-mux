import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';
import jetbrainsIcon from '@/assets/client-icons/jetbrains.svg';
import androidStudioIcon from '@/assets/client-icons/android-studio.svg';
import { resolveKnownClientKey } from '@/lib/clientIcons';
import {
  Laptop,
  Loader2,
  RefreshCw,
  Search,
  AlertCircle,
  PlugZap,
  X,
  Trash2,
  FolderOpen,
  Check,
  Globe,
  ShieldOff,
} from 'lucide-react';
import { ConnectIDEs } from '@/components/ConnectIDEs';
import type { GatewayStatus, OAuthClient } from '@/lib/api/gateway';
import {
  getGatewayStatus,
  listOAuthClients,
  updateOAuthClient,
  deleteOAuthClient,
  getOAuthClientGrants,
  grantOAuthClientFeatureSet,
  revokeOAuthClientFeatureSet,
} from '@/lib/api/gateway';
import {
  isStarterFeatureSet,
  listFeatureSetsBySpace,
  type FeatureSet,
} from '@/lib/api/featureSets';
import {
  Card,
  CardContent,
  Button,
  useToast,
  ToastContainer,
  useConfirm,
  PageHeader,
} from '@mcpmux/ui';
import {
  useDefaultSpace,
  useNavigateTo,
  usePendingClientId,
  useSetPendingClientId,
} from '@/stores';

// Bundled icons for well-known AI clients.
const CLIENT_ICON_ASSETS: Record<string, string> = {
  cursor: cursorIcon,
  vscode: vscodeIcon,
  claude: claudeIcon,
  windsurf: windsurfIcon,
  jetbrains: jetbrainsIcon,
  'android-studio': androidStudioIcon,
};

function ClientIcon({ logo_uri, client_name }: { logo_uri?: string | null; client_name: string }) {
  const knownKey = resolveKnownClientKey(client_name);
  const iconUrl = (knownKey && CLIENT_ICON_ASSETS[knownKey]) || logo_uri;
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt={client_name}
        className="h-full w-full rounded object-contain"
        onError={(e) => {
          e.currentTarget.style.display = 'none';
          e.currentTarget.parentElement!.append(document.createTextNode('🤖'));
        }}
      />
    );
  }
  return <span>🤖</span>;
}

function formatLastSeen(iso: string | null): string {
  if (!iso) return 'never';
  const then = new Date(iso);
  const now = new Date();
  const secs = Math.floor((now.getTime() - then.getTime()) / 1000);
  if (secs < 10) return 'just now';
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

/**
 * Connections page — list approved AI clients and revoke their access.
 *
 * In the v2 world, routing decisions (which Space, which FeatureSet) live
 * in Workspaces (per-root bindings), not per-client. This page is pure
 * observability + lifecycle: which clients have been approved, when each
 * was last seen, and "remove this key" when trust is withdrawn.
 */
export default function ClientsPage() {
  const [clients, setClients] = useState<OAuthClient[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [selected, setSelected] = useState<OAuthClient | null>(null);
  const [editAlias, setEditAlias] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [gatewayStatus, setGatewayStatus] = useState<GatewayStatus>({
    running: false,
    url: null,
    active_sessions: 0,
    connected_backends: 0,
  });

  const { toasts, success, error: showError, info, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();
  const pendingClientId = usePendingClientId();
  const setPendingClientId = useSetPendingClientId();
  const navigateTo = useNavigateTo();
  const defaultSpace = useDefaultSpace();

  const loadClients = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const data = await listOAuthClients();
      setClients(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const refreshClients = async () => {
    setIsRefreshing(true);
    try {
      setClients(await listOAuthClients());
    } catch (e) {
      console.warn('Failed to refresh clients:', e);
    } finally {
      setIsRefreshing(false);
    }
  };

  useEffect(() => {
    void loadClients();
    getGatewayStatus()
      .then(setGatewayStatus)
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!pendingClientId || isLoading) return;
    const client = clients.find((c) => c.client_id === pendingClientId);
    if (client) {
      openPanel(client);
      setPendingClientId(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingClientId, isLoading, clients]);

  useEffect(() => {
    const unlistenDomain = listen<{
      action: string;
      client_id: string;
      client_name?: string;
    }>('client-changed', (event) => {
      refreshClients();
      if (event.payload.action === 'reconnected') {
        const name = event.payload.client_name || event.payload.client_id;
        info('Client reconnected', name);
      }
    });
    const unlistenOAuth = listen('oauth-client-changed', () => {
      refreshClients();
    });
    return () => {
      unlistenDomain.then((fn) => fn());
      unlistenOAuth.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const openPanel = (client: OAuthClient) => {
    setSelected(client);
    setEditAlias(client.client_alias || '');
  };

  const handleSaveAlias = async () => {
    if (!selected) return;
    setIsSaving(true);
    try {
      const updated = await updateOAuthClient(selected.client_id, {
        client_alias: editAlias || undefined,
      });
      setClients((prev) => prev.map((c) => (c.client_id === updated.client_id ? updated : c)));
      setSelected(updated);
      success('Saved', `"${updated.client_alias || updated.client_name}" updated`);
    } catch (e) {
      showError('Failed to save', e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const handleRevoke = async (client: OAuthClient) => {
    const name = client.client_alias || client.client_name;
    if (
      !(await confirm({
        title: 'Revoke connection',
        message: `Remove "${name}"? All tokens for this client will be revoked. The client will need to re-approve to connect again.`,
        confirmLabel: 'Revoke',
        variant: 'danger',
      }))
    ) {
      return;
    }
    try {
      await deleteOAuthClient(client.client_id);
      setClients((prev) => prev.filter((c) => c.client_id !== client.client_id));
      setSelected(null);
      success('Connection revoked', `"${name}" removed`);
    } catch (e) {
      showError('Failed to revoke', e instanceof Error ? e.message : String(e));
    }
  };

  const filtered = clients.filter((client) => {
    if (!searchQuery) return true;
    const q = searchQuery.toLowerCase();
    return (
      client.client_name.toLowerCase().includes(q) ||
      client.client_alias?.toLowerCase().includes(q) ||
      client.client_id.toLowerCase().includes(q)
    );
  });

  // Snapshot `now` each time the clients list changes so the staleness
  // indicators refresh when the underlying data refreshes — without making
  // the component body impure.
  const renderNow = useMemo(() => Date.now(), [clients]);

  return (
    <div className="relative flex h-full flex-col" data-testid="clients-page">
      <header className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-8">
        <div className="mx-auto max-w-[2000px]">
          <PageHeader
            title="Apps"
            titleTestId="clients-title"
            subtitle={
              <>
                The AI apps connected through your gateway. Which tools each one gets (which Space,
                which FeatureSet) is configured in{' '}
                <button
                  onClick={() => navigateTo('workspaces')}
                  className="font-medium text-[rgb(var(--accent))] hover:underline"
                >
                  Workspaces
                </button>{' '}
                per folder, not per app.
              </>
            }
            actions={
              <Button variant="ghost" size="md" onClick={refreshClients} disabled={isRefreshing}>
                <RefreshCw className={`mr-2 h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
                Refresh
              </Button>
            }
          />

          {clients.length > 0 && (
            <div className="relative max-w-3xl">
              <Search className="absolute left-4 top-1/2 h-5 w-5 -translate-y-1/2 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder="Search by name, alias, or id…"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="focus:ring-primary-500 focus:border-primary-500 w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] py-3 pl-12 pr-4 text-base transition-all focus:outline-none focus:ring-2"
              />
            </div>
          )}
        </div>
      </header>

      {error && (
        <div className="flex-shrink-0 px-8 pt-6">
          <div className="mx-auto flex max-w-[2000px] items-start gap-3 rounded-xl border border-red-200 bg-red-50 p-4 dark:border-red-800 dark:bg-red-900/20">
            <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0 text-red-600 dark:text-red-400" />
            <p className="text-base text-red-600 dark:text-red-400">{error}</p>
          </div>
        </div>
      )}

      <div className="flex-1 overflow-auto px-8 py-8">
        <div className="mx-auto max-w-[2000px]">
          {isLoading ? (
            <div className="flex h-64 items-center justify-center">
              <Loader2 className="text-primary-500 h-8 w-8 animate-spin" />
            </div>
          ) : filtered.length === 0 ? (
            searchQuery ? (
              <Card className="mx-auto max-w-2xl">
                <CardContent className="flex flex-col items-center justify-center py-16">
                  <Laptop className="mb-4 h-16 w-16 text-[rgb(var(--muted))]" />
                  <h3 className="mb-2 text-lg font-medium">No connections match your search</h3>
                  <p className="max-w-md text-center text-sm text-[rgb(var(--muted))]">
                    Try adjusting your search terms.
                  </p>
                </CardContent>
              </Card>
            ) : (
              <EmptyStateOnboarding gatewayStatus={gatewayStatus} />
            )
          ) : (
            <div className="auto-fill-cards grid gap-5">
              {filtered.map((client) => {
                const isSelected = selected?.client_id === client.client_id;
                const displayName = client.client_alias || client.client_name;
                return (
                  <Card
                    key={client.client_id}
                    className={`cursor-pointer transition-all hover:scale-[1.01] hover:shadow-lg ${
                      isSelected ? 'ring-primary-500 shadow-lg ring-2' : ''
                    }`}
                    onClick={() => openPanel(client)}
                    data-testid={`client-card-${client.client_id.replace(/[^a-zA-Z0-9-_]/g, '_')}`}
                  >
                    <CardContent className="p-6">
                      <div className="mb-4 flex items-start gap-4">
                        <div className="flex h-14 w-14 flex-shrink-0 items-center justify-center rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] text-3xl">
                          <ClientIcon logo_uri={client.logo_uri} client_name={client.client_name} />
                        </div>
                        <div className="min-w-0 flex-1">
                          <h3 className="mb-1 truncate text-lg font-semibold">{displayName}</h3>
                          {client.client_alias && (
                            <p className="truncate text-xs text-[rgb(var(--muted))]">
                              {client.client_name}
                            </p>
                          )}
                        </div>
                      </div>

                      <div className="flex items-center justify-between text-xs text-[rgb(var(--muted))]">
                        <span className="inline-flex items-center gap-1.5">
                          <span
                            className={`h-1.5 w-1.5 rounded-full ${lastSeenDotColor(client.last_seen, renderNow)}`}
                          />
                          Last seen {formatLastSeen(client.last_seen)}
                        </span>
                        <CapabilityBadge
                          reportsRoots={client.reports_roots}
                          rootsCapabilityKnown={client.roots_capability_known}
                        />
                      </div>
                    </CardContent>
                  </Card>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {selected && (
        <>
          <div
            className="animate-in fade-in fixed inset-0 z-40 bg-black/20 backdrop-blur-[2px] duration-200"
            onClick={() => setSelected(null)}
          />
          <SidePanel
            client={selected}
            editAlias={editAlias}
            setEditAlias={setEditAlias}
            isSaving={isSaving}
            defaultSpaceId={defaultSpace?.id ?? null}
            onClose={() => setSelected(null)}
            onSaveAlias={handleSaveAlias}
            onRevoke={() => handleRevoke(selected)}
            onOpenWorkspaces={() => {
              setSelected(null);
              navigateTo('workspaces');
            }}
            onToastError={showError}
            onToastSuccess={success}
          />
        </>
      )}

      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}
    </div>
  );
}

function lastSeenDotColor(lastSeen: string | null, now: number): string {
  if (!lastSeen) return 'bg-gray-400';
  const secs = (now - new Date(lastSeen).getTime()) / 1000;
  if (secs < 120) return 'bg-emerald-500';
  if (secs < 3600) return 'bg-amber-500';
  return 'bg-gray-400';
}

/**
 * Tri-state capability chip: shows nothing until the gateway has actually
 * observed this client's `initialize` (so a brand-new client doesn't
 * misleadingly look "Rootless" before we know which it is). Once we've
 * processed at least one session the chip resolves to:
 *  - **Reports workspace** (green) — the client declared MCP `roots`,
 *    routing flows through Workspace bindings, per-client grants are a
 *    rare-case fallback only.
 *  - **Rootless** (amber) — the client explicitly does NOT declare the
 *    `roots` capability (Claude.ai web, ChatGPT connectors, …); the
 *    per-client grant list below is the routing source.
 *
 * Sticky-positive: once a client has been seen reporting roots we keep
 * the green badge across reconnects so a one-off rootless session doesn't
 * flip the UI to amber.
 */
function CapabilityBadge({
  reportsRoots,
  rootsCapabilityKnown,
}: {
  reportsRoots: boolean;
  rootsCapabilityKnown: boolean;
}) {
  if (!rootsCapabilityKnown) {
    // Unknown — hide the badge entirely. Returning null keeps adjacent
    // layout stable (the panel header + the grants section both render
    // their own context, so we don't need a placeholder).
    return null;
  }
  if (reportsRoots) {
    return (
      <span
        title="This client declares the MCP roots capability. Its sessions route via Workspace bindings; the per-client grant list below applies only to rare rootless reconnects."
        className="inline-flex items-center gap-1 rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300"
      >
        <FolderOpen className="h-3 w-3" />
        Reports workspace
      </span>
    );
  }
  return (
    <span
      title="This client does NOT declare the MCP roots capability. It always routes via the per-client grants set in this panel — configure them below."
      className="inline-flex items-center gap-1 rounded-full bg-amber-100 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-amber-700 dark:bg-amber-900/30 dark:text-amber-300"
    >
      <Globe className="h-3 w-3" />
      Rootless
    </span>
  );
}

// ---------------------------------------------------------------------------
// Side panel
// ---------------------------------------------------------------------------

interface SidePanelProps {
  client: OAuthClient;
  editAlias: string;
  setEditAlias: (v: string) => void;
  isSaving: boolean;
  defaultSpaceId: string | null;
  onClose: () => void;
  onSaveAlias: () => void;
  onRevoke: () => void;
  onOpenWorkspaces: () => void;
  onToastError: (title: string, body?: string) => void;
  onToastSuccess: (title: string, body?: string) => void;
}

function SidePanel({
  client,
  editAlias,
  setEditAlias,
  isSaving,
  defaultSpaceId,
  onClose,
  onSaveAlias,
  onRevoke,
  onOpenWorkspaces,
  onToastError,
  onToastSuccess,
}: SidePanelProps) {
  const aliasDirty = (client.client_alias || '') !== editAlias;

  return (
    <div className="animate-in slide-in-from-right fixed bottom-0 right-0 top-0 z-50 flex w-full min-w-[420px] max-w-[480px] flex-col border-l border-[rgb(var(--border))] bg-[rgb(var(--surface))] shadow-2xl duration-300">
      <div className="flex-shrink-0 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        <div className="flex items-start justify-between">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <div className="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] text-2xl">
              <ClientIcon logo_uri={client.logo_uri} client_name={client.client_name} />
            </div>
            <div className="min-w-0 flex-1">
              <h2 className="truncate text-lg font-bold">
                {client.client_alias || client.client_name}
              </h2>
              <div className="mt-0.5 flex items-center gap-2">
                <p className="min-w-0 flex-1 truncate text-xs text-[rgb(var(--muted))]">
                  {client.client_alias ? client.client_name : client.client_id}
                </p>
                <CapabilityBadge
                  reportsRoots={client.reports_roots}
                  rootsCapabilityKnown={client.roots_capability_known}
                />
              </div>
            </div>
          </div>
          <button
            onClick={onClose}
            className="flex-shrink-0 rounded-lg p-1.5 transition-colors hover:bg-[rgb(var(--surface-hover))]"
            aria-label="Close panel"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      <div className="flex-1 space-y-6 overflow-y-auto p-6">
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
            Display name
          </h3>
          <div className="flex gap-2">
            <input
              type="text"
              value={editAlias}
              onChange={(e) => setEditAlias(e.target.value)}
              placeholder={client.client_name}
              className="focus:ring-primary-500 focus:border-primary-500 flex-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 text-sm focus:outline-none focus:ring-2"
            />
            <Button
              size="sm"
              variant="primary"
              onClick={onSaveAlias}
              disabled={!aliasDirty || isSaving}
            >
              {isSaving ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Check className="h-4 w-4" />
              )}
            </Button>
          </div>
          <p className="mt-1.5 text-xs text-[rgb(var(--muted))]">
            An alias shown in logs and this list. Doesn't affect routing.
          </p>
        </section>

        <section className="rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--background))] p-4">
          <div className="flex items-start gap-3">
            <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-[rgb(var(--accent))]/10">
              <FolderOpen className="h-5 w-5 text-[rgb(var(--accent))]" />
            </div>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold">Routing is workspace-driven</p>
              <p className="mt-1 text-xs text-[rgb(var(--muted))]">
                When this client reports a folder as an MCP root, mcpmux uses the matching Workspace
                binding to pick the Space and FeatureSet.
              </p>
              <button
                onClick={onOpenWorkspaces}
                className="mt-2 text-xs font-medium text-[rgb(var(--accent))] hover:underline"
              >
                Open Workspaces →
              </button>
            </div>
          </div>
        </section>

        {/* Per-client grants only matter for clients that explicitly do
            NOT declare the MCP `roots` capability — Claude.ai web,
            ChatGPT connectors, and similar rootless connectors. For
            roots-capable clients (Cursor, VS Code, Claude Desktop)
            routing flows through Workspace bindings and these grants
            never apply, so the section is just chrome. For clients
            we haven't observed yet, the capability is unknown and the
            section would have no audience either way — defer it until
            the first `initialize` reveals the answer. */}
        {client.roots_capability_known && !client.reports_roots && (
          <RootlessGrantsSection
            clientId={client.client_id}
            defaultSpaceId={defaultSpaceId}
            onError={onToastError}
            onSuccess={onToastSuccess}
          />
        )}

        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
            Client info
          </h3>
          <div className="space-y-2 text-xs">
            <InfoRow label="Client ID" value={client.client_id} mono />
            <InfoRow label="Client name" value={client.client_name} />
            {client.software_id && <InfoRow label="Software" value={client.software_id} />}
            {client.software_version && <InfoRow label="Version" value={client.software_version} />}
            <InfoRow label="Registered via" value={client.registration_type ?? 'dynamic'} />
            {client.last_seen && (
              <InfoRow
                label="Last seen"
                value={`${formatLastSeen(client.last_seen)} (${new Date(client.last_seen).toLocaleString()})`}
              />
            )}
          </div>
        </section>
      </div>

      <div className="flex-shrink-0 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        <Button
          variant="ghost"
          size="sm"
          onClick={onRevoke}
          className="w-full text-red-600 hover:bg-red-50 hover:text-red-700 dark:hover:bg-red-900/20"
        >
          <Trash2 className="mr-2 h-4 w-4" />
          Revoke connection
        </Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Rootless-fallback FeatureSet grants
//
// Edits the `client_grants` table. Only consulted by the resolver when the
// client did NOT declare the MCP `roots` capability — i.e. Claude.ai web,
// ChatGPT, and similar connectors that don't surface a workspace folder.
// Roots-capable desktop clients (Cursor, VS Code, Claude Desktop) ignore
// these grants entirely; their routing comes from Workspace bindings.
//
// We render this section unconditionally rather than hiding it for
// roots-capable clients: capability detection only happens at session time,
// so a client we've classified as "reports workspace" today might tomorrow
// open a rootless session (e.g. CLI subcommand). Surfacing the grant
// editor + a clear "only used when…" note is more honest than hiding it.
// ---------------------------------------------------------------------------

/**
 * Renders the per-client FS grant editor. The parent decides whether to
 * mount this — only mounted for clients that have explicitly declared
 * they do NOT support the MCP `roots` capability. Roots-capable and
 * unknown-capability clients don't see this section at all.
 */
function RootlessGrantsSection({
  clientId,
  defaultSpaceId,
  onError,
  onSuccess,
}: {
  clientId: string;
  defaultSpaceId: string | null;
  onError: (title: string, body?: string) => void;
  onSuccess: (title: string, body?: string) => void;
}) {
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  const [grantedIds, setGrantedIds] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [pendingFsId, setPendingFsId] = useState<string | null>(null);
  const [search, setSearch] = useState('');

  // Filter the FS list by search query (name + description, case-
  // insensitive). Always show currently-granted FSes even if they don't
  // match the query — otherwise the operator could "lose" a granted FS
  // they're trying to revoke. A small "+ N granted" hint surfaces them
  // so the omission is visible.
  const filteredFs = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return featureSets;
    return featureSets.filter((f) => {
      if (grantedIds.includes(f.id)) return true;
      if (f.name.toLowerCase().includes(q)) return true;
      if (f.description?.toLowerCase().includes(q)) return true;
      return false;
    });
  }, [featureSets, search, grantedIds]);

  useEffect(() => {
    let cancelled = false;
    if (!defaultSpaceId) {
      setIsLoading(false);
      return;
    }
    setIsLoading(true);
    Promise.all([
      listFeatureSetsBySpace(defaultSpaceId),
      getOAuthClientGrants(clientId, defaultSpaceId),
    ])
      .then(([fs, grants]) => {
        if (cancelled) return;
        setFeatureSets(fs);
        setGrantedIds(grants);
      })
      .catch((e) => {
        if (cancelled) return;
        onError('Failed to load grants', e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clientId, defaultSpaceId]);

  const toggle = async (fs: FeatureSet) => {
    if (!defaultSpaceId) return;
    const isGranted = grantedIds.includes(fs.id);
    setPendingFsId(fs.id);
    // Optimistic update — gateway emits ClientGrantChanged + we'll re-sync
    // via the `oauth-client-changed` listener at the parent level.
    setGrantedIds((prev) => (isGranted ? prev.filter((id) => id !== fs.id) : [...prev, fs.id]));
    try {
      if (isGranted) {
        await revokeOAuthClientFeatureSet(clientId, defaultSpaceId, fs.id);
        onSuccess(`Revoked "${fs.name}"`);
      } else {
        await grantOAuthClientFeatureSet(clientId, defaultSpaceId, fs.id);
        onSuccess(`Granted "${fs.name}"`);
      }
    } catch (e) {
      // Roll back the optimistic update on failure.
      setGrantedIds((prev) => (isGranted ? [...prev, fs.id] : prev.filter((id) => id !== fs.id)));
      onError(
        isGranted ? 'Failed to revoke grant' : 'Failed to grant',
        e instanceof Error ? e.message : String(e)
      );
    } finally {
      setPendingFsId(null);
    }
  };

  return (
    <section>
      <div className="mb-2 flex items-start gap-2">
        <div className="flex-1">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
            Default for rootless sessions
          </h3>
        </div>
        <span
          className="inline-flex items-center gap-1 rounded-full bg-[rgb(var(--accent))]/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-[rgb(var(--accent))]"
          title="Used only when this client connects without reporting a workspace folder"
        >
          <Globe className="h-3 w-3" />
          Rootless only
        </span>
      </div>
      <p className="mb-3 text-xs leading-relaxed text-[rgb(var(--muted))]">
        This client doesn&apos;t declare the MCP{' '}
        <code className="rounded bg-[rgb(var(--surface))] px-1 text-[10px]">roots</code> capability,
        so its sessions route through the FeatureSets you pick here instead of through Workspace
        bindings. Leaving the list empty denies the client — rootless sessions then see only the
        built-in
        <code className="mx-1 rounded bg-[rgb(var(--surface))] px-1 text-[10px]">mcpmux_*</code>
        management tools.
      </p>

      {!defaultSpaceId ? (
        <p className="text-xs italic text-[rgb(var(--muted))]">No default Space configured.</p>
      ) : isLoading ? (
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-4 w-4 animate-spin text-[rgb(var(--muted))]" />
        </div>
      ) : featureSets.length === 0 ? (
        <p className="text-xs italic text-[rgb(var(--muted))]">
          No FeatureSets exist in the default Space yet.
        </p>
      ) : (
        // Bordered container, search at the top, scrollable body — same
        // shape as the Workspaces binding picker so the two screens feel
        // consistent. Always-on search since even small lists benefit
        // from typeahead, and it caps height growth as the FS count
        // grows past the visible area.
        <div
          className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))]"
          data-testid="rootless-grants-list"
        >
          <div className="border-b border-[rgb(var(--border-subtle))] p-2">
            <div className="relative">
              <Search className="absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[rgb(var(--muted))]" />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={`Search ${featureSets.length} feature set${featureSets.length === 1 ? '' : 's'}…`}
                className="focus:ring-primary-500 w-full rounded border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] py-1.5 pl-7 pr-2.5 text-xs focus:outline-none focus:ring-2"
                data-testid="rootless-grants-search"
              />
            </div>
          </div>
          <div className="max-h-72 space-y-1 overflow-y-auto p-1.5">
            {filteredFs.length === 0 ? (
              <p className="px-2 py-3 text-center text-xs italic text-[rgb(var(--muted))]">
                No feature sets match &ldquo;{search}&rdquo;.
              </p>
            ) : (
              filteredFs.map((fs) => {
                const isGranted = grantedIds.includes(fs.id);
                const isPending = pendingFsId === fs.id;
                return (
                  <button
                    key={fs.id}
                    onClick={() => toggle(fs)}
                    disabled={isPending}
                    className={[
                      'flex w-full items-center gap-2.5 rounded px-2.5 py-2 text-left text-sm transition-colors',
                      isGranted
                        ? 'bg-primary-500/10 hover:bg-primary-500/15'
                        : 'hover:bg-[rgb(var(--surface-hover))]',
                      isPending ? 'cursor-wait opacity-60' : 'cursor-pointer',
                    ].join(' ')}
                    data-testid={`grant-toggle-${fs.id}`}
                  >
                    <div
                      className={[
                        'flex h-4 w-4 flex-shrink-0 items-center justify-center rounded border',
                        isGranted
                          ? 'bg-primary-500 border-primary-500'
                          : 'border-[rgb(var(--border-strong))] bg-[rgb(var(--surface))]',
                      ].join(' ')}
                    >
                      {isPending ? (
                        <Loader2 className="h-3 w-3 animate-spin text-white" />
                      ) : isGranted ? (
                        <Check className="h-3 w-3 text-white" strokeWidth={3} />
                      ) : null}
                    </div>
                    <span className="flex-shrink-0 text-base leading-none">{fs.icon ?? '📦'}</span>
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-medium">{fs.name}</p>
                      {fs.description && (
                        <p className="truncate text-[11px] text-[rgb(var(--muted))]">
                          {fs.description}
                        </p>
                      )}
                    </div>
                    {isStarterFeatureSet(fs) && (
                      <span
                        className="flex-shrink-0 rounded bg-[rgb(var(--surface))] px-1 py-0.5 text-[9px] uppercase tracking-wide text-[rgb(var(--muted))]"
                        title="Auto-seeded with this Space."
                      >
                        starter
                      </span>
                    )}
                  </button>
                );
              })
            )}
          </div>
          {search && filteredFs.length > 0 && filteredFs.length < featureSets.length && (
            <div className="border-t border-[rgb(var(--border-subtle))] px-3 py-1.5 text-[11px] text-[rgb(var(--muted))]">
              {filteredFs.length} of {featureSets.length} shown
              {grantedIds.some((id) => !filteredFs.find((f) => f.id === id)) &&
                ' (granted FSes always visible)'}
            </div>
          )}
        </div>
      )}

      {grantedIds.length === 0 && featureSets.length > 0 && !isLoading && (
        <div className="mt-3 flex items-start gap-2 rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-2.5">
          <ShieldOff className="mt-0.5 h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
          <p className="text-[11px] text-[rgb(var(--muted))]">
            No defaults set — rootless sessions from this client are denied. That&apos;s the safe
            default. Pick a FeatureSet above only if you trust this client to operate without a
            workspace folder.
          </p>
        </div>
      )}
    </section>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] p-2">
      <span className="w-28 flex-shrink-0 text-[rgb(var(--muted))]">{label}</span>
      <span className={`min-w-0 flex-1 break-all ${mono ? 'font-mono text-[10px]' : ''}`}>
        {value}
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty-state onboarding (preserved from original)
// ---------------------------------------------------------------------------

function EmptyStateOnboarding({ gatewayStatus }: { gatewayStatus: GatewayStatus }) {
  return (
    <div className="mx-auto max-w-4xl space-y-6" data-testid="clients-empty-connect">
      <Card data-testid="clients-empty-onboarding">
        <CardContent className="p-8">
          <div className="mb-1 flex items-start gap-4">
            <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-xl bg-[rgb(var(--primary))]/10 text-[rgb(var(--primary))]">
              <PlugZap className="h-6 w-6" />
            </div>
            <div>
              <h2 className="text-xl font-semibold">Connect your first AI app</h2>
              <p className="mt-1 text-sm text-[rgb(var(--muted))]">
                McpMux is one connection your AI app uses to reach every tool. Three steps and
                you&apos;re done:
              </p>
            </div>
          </div>

          <ol className="mt-6 space-y-3 text-sm">
            <OnboardingStep
              n={1}
              tone="primary"
              title="Pick your IDE below and follow its prompt"
              body="Each card tells you exactly what the button does — either one-click install or copy a small config for you to paste."
            />
            <OnboardingStep
              n={2}
              tone="primary"
              title="Enable mcpmux in your IDE's MCP settings"
              body="One-click install usually wires it up automatically. If you pasted a config, open your IDE's MCP panel and toggle mcpmux on — a restart may be needed for the IDE to pick it up."
            />
            <OnboardingStep
              n={3}
              tone="emerald"
              title={
                <>
                  Approve the connection{' '}
                  <span className="ml-1 inline-flex items-center rounded-md bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-emerald-700 dark:text-emerald-300">
                    right here
                  </span>
                </>
              }
              body="mcpmux will pop a dialog the moment your IDE reaches the gateway. Until you accept it, nothing is routed."
            />
          </ol>

          {!gatewayStatus.running && (
            <div className="mt-5 flex items-start gap-2 rounded-lg border border-amber-300 bg-amber-50 p-3 text-xs dark:border-amber-700/60 dark:bg-amber-900/20">
              <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
              <div>
                <p className="font-semibold text-amber-800 dark:text-amber-200">
                  Gateway is stopped
                </p>
                <p className="mt-0.5 text-amber-700 dark:text-amber-300">
                  Start it from the Dashboard first — otherwise the IDE will hang at{' '}
                  <code>initialize</code>.
                </p>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <ConnectIDEs
        gatewayUrl={gatewayStatus.url || 'http://localhost:45818'}
        gatewayRunning={gatewayStatus.running}
      />
    </div>
  );
}

function OnboardingStep({
  n,
  title,
  body,
  tone,
}: {
  n: number;
  title: React.ReactNode;
  body: string;
  tone: 'primary' | 'emerald';
}) {
  const cls =
    tone === 'emerald'
      ? 'bg-emerald-100 dark:bg-emerald-900/40 text-emerald-700 dark:text-emerald-300'
      : 'bg-primary-100 dark:bg-primary-900/40 text-primary-700 dark:text-primary-300';
  return (
    <li className="flex items-start gap-3">
      <span
        className={`flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full text-sm font-semibold ${cls}`}
      >
        {n}
      </span>
      <div className="flex-1">
        <p className="font-medium">{title}</p>
        <p className="mt-0.5 text-xs text-[rgb(var(--muted))]">{body}</p>
      </div>
    </li>
  );
}
