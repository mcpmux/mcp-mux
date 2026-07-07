import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@/lib/events';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  FileText,
  Folder,
  FolderOpen,
  FolderSearch,
  KeyRound,
  Layers,
  Loader2,
  MessageSquare,
  Package,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server as ServerIcon,
  Trash2,
  Wrench,
  X,
} from 'lucide-react';
import { Button, Card, CardContent, useToast, ToastContainer, useConfirm } from '@mcpmux/ui';
import {
  clearUnmappedReportedRoots,
  createWorkspaceBinding,
  deleteWorkspaceBinding,
  getWorkspaceEffectiveFeatures,
  listReportedWorkspaceRoots,
  listWorkspaceBindings,
  updateWorkspaceBinding,
  validateWorkspaceRoot,
  type EffectiveFeature,
  type WorkspaceBinding,
  type WorkspaceBindingInput,
  type WorkspaceEffectiveFeatures,
} from '@/lib/api/workspaceBindings';
import { isStarterFeatureSet, listFeatureSets, type FeatureSet } from '@/lib/api/featureSets';
import { getGatewayStatus, listOAuthClients, type OAuthClient } from '@/lib/api/gateway';
import { getGatewayAuthDisabled } from '@/lib/api/workspaceInstall';
import { WorkspaceInstallPanel } from './WorkspaceInstallPanel';
import { WorkspaceSetupWizard } from './WorkspaceSetupWizard';
import { CreateFeatureSetLink } from './CreateFeatureSetLink';
import {
  buildMcpConfig,
  COPIED_LABEL,
  COPY_CONFIG_BEARER_LABEL,
  COPY_CONFIG_LABEL,
  DEFAULT_MCP_ENDPOINT,
} from './connectConfig';
import {
  useSpaces,
  usePendingWorkspaceNew,
  useSetPendingWorkspaceNew,
  usePendingWorkspaceRoot,
  useSetPendingWorkspaceRoot,
} from '@/stores';
import type { Space } from '@/lib/api/spaces';

/**
 * Workspaces page.
 *
 * Mirrors the Clients page's shape for visual consistency:
 *   • Header: title + subtitle + refresh, followed by a single large search.
 *   • Content: responsive cards grid inside a max-w-[2000px] wrapper.
 *   • Inspector: fixed-right side panel with a `fixed inset-0` backdrop-
 *     blur dim + `animate-in slide-in-from-right` entrance.
 *
 * Each card is a workspace entry, unioning bindings and live reported roots
 * (dedup'd by normalized path). Status is conveyed with a corner dot + pill:
 *   • LIVE + unmapped → amber
 *   • LIVE + mapped   → emerald
 *   • OFFLINE + mapped → neutral
 */

type EntryKind = 'unmapped-live' | 'mapped-live' | 'mapped-offline';
interface Entry {
  id: string;
  kind: EntryKind;
  root: string;
  binding: WorkspaceBinding | null;
  isLive: boolean;
}
type Selected = { mode: 'new' } | { mode: 'entry'; id: string };

export function WorkspacesPage() {
  const spaces = useSpaces();
  const pendingNew = usePendingWorkspaceNew();
  const clearPendingNew = useSetPendingWorkspaceNew();
  const pendingRoot = usePendingWorkspaceRoot();
  const clearPendingRoot = useSetPendingWorkspaceRoot();
  const [bindings, setBindings] = useState<WorkspaceBinding[]>([]);
  const [reportedRoots, setReportedRoots] = useState<string[]>([]);
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  // Registered inbound clients — used to recognise the id-binding McpMux
  // auto-creates per API-key client (its identifier is the client_id) so the
  // UI can show the client's name and lock that managed mapping down.
  const [clients, setClients] = useState<OAuthClient[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { toasts, success, error: showError, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();

  const [selected, setSelected] = useState<Selected | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [filter, setFilter] = useState<'all' | 'live' | 'mapped' | 'unmapped'>('all');

  const loadData = useCallback(async () => {
    setError(null);
    try {
      const [b, fs, roots, cl] = await Promise.all([
        listWorkspaceBindings(),
        listFeatureSets(),
        listReportedWorkspaceRoots().catch(() => [] as string[]),
        listOAuthClients().catch(() => [] as OAuthClient[]),
      ]);
      setBindings(b);
      setFeatureSets(fs);
      setReportedRoots(roots);
      setClients(Array.isArray(cl) ? cl : []);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    setIsLoading(true);
    void loadData().finally(() => setIsLoading(false));
  }, [loadData]);

  // Opened from the home "Set up a mapping" CTA — launch the create walkthrough.
  useEffect(() => {
    if (pendingNew) {
      setSelected({ mode: 'new' });
      clearPendingNew(false);
    }
  }, [pendingNew, clearPendingNew]);

  // Deep-linked from another surface (e.g. the Clients page "Open this client's
  // mapping" link) to a specific binding. Wait until bindings have loaded, then
  // open that binding's inspector. Match the `workspace_root` verbatim — id
  // keys (client ids) are case-sensitive, and path roots are pre-normalized.
  useEffect(() => {
    if (!pendingRoot || isLoading) return;
    const target = bindings.find((b) => b.workspace_root === pendingRoot);
    if (target) {
      setSelected({ mode: 'entry', id: target.id });
    }
    // Clear regardless: if the binding no longer exists there's nothing to
    // select, and the user still lands on the Mapping tab (the fallback).
    clearPendingRoot(null);
  }, [pendingRoot, isLoading, bindings, clearPendingRoot]);

  // Refresh whenever something the table reflects changes outside the page:
  //   • `session-roots-changed` — a connected client newly reported a root.
  //   • `workspace-binding-changed` — a binding was created/updated/deleted
  //     by another surface (e.g. the new-workspace popup or the meta-tool).
  // Without the binding listener, popup-driven saves leave this page showing
  // the stale "UNMAPPED" badge until the user navigates away and back.
  useEffect(() => {
    const reload = () => {
      void loadData();
    };
    const unRoots = listen('session-roots-changed', reload);
    const unBinding = listen('workspace-binding-changed', reload);
    // A client rename/delete changes the name shown for its managed id mapping.
    const unClient = listen('client-changed', reload);
    return () => {
      unRoots.then((fn) => fn());
      unBinding.then((fn) => fn());
      unClient.then((fn) => fn());
    };
  }, [loadData]);

  const refresh = async () => {
    setIsRefreshing(true);
    try {
      await loadData();
    } finally {
      setIsRefreshing(false);
    }
  };

  const bindingsByRoot = useMemo(() => {
    const m = new Map<string, WorkspaceBinding>();
    for (const b of bindings) m.set(b.workspace_root.toLowerCase(), b);
    return m;
  }, [bindings]);
  const fsById = useMemo(() => {
    const m = new Map<string, FeatureSet>();
    for (const f of featureSets) m.set(f.id, f);
    return m;
  }, [featureSets]);
  const spaceById = useMemo(() => {
    const m = new Map<string, Space>();
    for (const s of spaces) m.set(s.id, s);
    return m;
  }, [spaces]);
  // client_id -> display name (alias preferred, else self-reported name). Used
  // to recognise an auto-created client mapping and label it by client name.
  const clientNameById = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of clients) m.set(c.client_id, c.client_alias || c.client_name);
    return m;
  }, [clients]);
  // An id binding whose identifier matches a registered client is the mapping
  // McpMux auto-created for that API-key client: it carries the client's name,
  // its identifier is fixed, and it's removed with the client (not by hand).
  const clientNameForBinding = useCallback(
    (binding: WorkspaceBinding | null): string | null =>
      binding?.binding_type === 'id'
        ? (clientNameById.get(binding.workspace_root) ?? null)
        : null,
    [clientNameById]
  );

  /**
   * Unified list: live-reported roots come first (unmapped amber, then
   * mapped emerald), then persisted bindings whose clients aren't live.
   */
  const entries: Entry[] = useMemo(() => {
    const list: Entry[] = [];
    const seen = new Set<string>();
    for (const root of reportedRoots) {
      const key = root.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      const binding = bindingsByRoot.get(key) ?? null;
      list.push({
        id: binding?.id ?? `live:${root}`,
        kind: binding ? 'mapped-live' : 'unmapped-live',
        root,
        binding,
        isLive: true,
      });
    }
    for (const b of bindings) {
      const key = b.workspace_root.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      list.push({
        id: b.id,
        kind: 'mapped-offline',
        root: b.workspace_root,
        binding: b,
        isLive: false,
      });
    }
    const rank: Record<EntryKind, number> = {
      'unmapped-live': 0,
      'mapped-live': 1,
      'mapped-offline': 2,
    };
    return list.sort((a, b) => {
      const o = rank[a.kind] - rank[b.kind];
      return o !== 0 ? o : a.root.localeCompare(b.root);
    });
  }, [bindings, bindingsByRoot, reportedRoots]);

  const filtered = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return entries.filter((e) => {
      if (filter === 'live' && !e.isLive) return false;
      if (filter === 'mapped' && !e.binding) return false;
      if (filter === 'unmapped' && e.kind !== 'unmapped-live') return false;
      if (!q) return true;
      const spaceName = e.binding ? (spaceById.get(e.binding.space_id)?.name ?? '') : '';
      const fsNames = e.binding
        ? e.binding.feature_set_ids.map((id) => fsById.get(id)?.name ?? '').join(' ')
        : '';
      return (
        e.root.toLowerCase().includes(q) ||
        spaceName.toLowerCase().includes(q) ||
        fsNames.toLowerCase().includes(q)
      );
    });
  }, [entries, searchQuery, filter, spaceById, fsById]);

  const counts = useMemo(() => {
    let live = 0;
    let mapped = 0;
    let unmapped = 0;
    for (const e of entries) {
      if (e.isLive) live++;
      if (e.binding) mapped++;
      if (e.kind === 'unmapped-live') unmapped++;
    }
    return { all: entries.length, live, mapped, unmapped };
  }, [entries]);

  const selectedEntry: Entry | null =
    selected?.mode === 'entry' ? (entries.find((e) => e.id === selected.id) ?? null) : null;
  const selectedIsNew = selected?.mode === 'new';
  const panelOpen = selected !== null;
  const selectedClientName = clientNameForBinding(selectedEntry?.binding ?? null);

  const handleCreate = async (input: WorkspaceBindingInput): Promise<WorkspaceBinding> => {
    const created = await createWorkspaceBinding(input);
    setBindings((prev) =>
      [...prev, created].sort((a, b) => a.workspace_root.localeCompare(b.workspace_root))
    );
    success('Mapping saved', created.workspace_root);
    return created;
  };

  const handleUpdate = async (id: string, input: WorkspaceBindingInput) => {
    const updated = await updateWorkspaceBinding(id, input);
    setBindings((prev) =>
      prev
        .map((b) => (b.id === id ? updated : b))
        .sort((a, b) => a.workspace_root.localeCompare(b.workspace_root))
    );
    success('Mapping updated', updated.workspace_root);
  };

  const handleDelete = async (binding: WorkspaceBinding) => {
    const ok = await confirm({
      title: 'Remove mapping',
      message: `Apps opening "${binding.workspace_root}" will stop receiving these tools. You can map the project again anytime.`,
      confirmLabel: 'Remove',
      variant: 'danger',
    });
    if (!ok) return;
    try {
      await deleteWorkspaceBinding(binding.id);
      setBindings((prev) => prev.filter((b) => b.id !== binding.id));
      setSelected(null);
      success('Mapping removed', binding.workspace_root);
    } catch (e) {
      showError('Failed to remove mapping', e instanceof Error ? e.message : String(e));
    }
  };

  // Bulk "clear" for the unmapped (amber) folders. These are live-reported
  // roots with no binding — clearing drops them from the gateway's in-memory
  // session-roots registry so this list empties in one action, and the
  // "map this folder?" prompt is offered again next time those apps report a
  // folder. Mapped folders are untouched.
  const handleClearUnmapped = async () => {
    const n = counts.unmapped;
    const ok = await confirm({
      title: 'Clear unmapped folders',
      message: `Remove ${n} unmapped folder${n === 1 ? '' : 's'} from this list. McpMux will offer to map ${n === 1 ? 'it' : 'them'} again the next time those apps report the folder.`,
      confirmLabel: 'Clear all',
    });
    if (!ok) return;
    try {
      const cleared = await clearUnmappedReportedRoots();
      await loadData();
      success(
        cleared > 0
          ? `Cleared ${cleared} unmapped folder${cleared === 1 ? '' : 's'}`
          : 'Nothing to clear',
        cleared > 0 ? "You'll be asked to map them again next time." : undefined
      );
    } catch (e) {
      showError('Could not clear unmapped folders', e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="relative flex h-full flex-col" data-testid="workspaces-page">
      <header className="flex-shrink-0 border-b border-[rgb(var(--border-subtle))] p-8">
        <div className="mx-auto max-w-[2000px]">
          <div className="mb-6 flex items-start justify-between gap-6">
            <div className="min-w-0 flex-1">
              <h1 className="text-3xl font-bold" data-testid="workspaces-title">
                Workspaces
              </h1>
              <p className="mt-2 max-w-2xl text-base text-[rgb(var(--muted))]">
                Map a project to the tools it should get. When you open that project in a connected
                app — Cursor, VS Code, Claude — McpMux serves exactly the tools you chose for it.
                Projects you haven&apos;t mapped fall back to your default Starter set, so they work
                out of the box — map one only when it should see something different.
              </p>
            </div>
            <div className="flex flex-shrink-0 items-center gap-2">
              <Button
                variant="ghost"
                size="md"
                onClick={refresh}
                disabled={isRefreshing}
                className="whitespace-nowrap"
              >
                <RefreshCw className={`mr-2 h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
                Refresh
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={() => setSelected({ mode: 'new' })}
                data-testid="workspace-binding-create-toggle"
                className="whitespace-nowrap"
              >
                <Plus className="mr-2 h-4 w-4" />
                New mapping
              </Button>
            </div>
          </div>

          <div className="flex max-w-3xl flex-wrap items-center gap-3">
            <div className="relative min-w-[220px] flex-1">
              <Search className="absolute left-4 top-1/2 h-5 w-5 -translate-y-1/2 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder="Search by path, space, or feature set…"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="focus:ring-primary-500 focus:border-primary-500 w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] py-3 pl-12 pr-4 text-base transition-all focus:outline-none focus:ring-2"
                data-testid="workspace-binding-search"
              />
            </div>
            <SegmentedFilter
              value={filter}
              onChange={setFilter}
              options={[
                { value: 'all', label: 'All', count: counts.all },
                { value: 'live', label: 'Live', count: counts.live },
                { value: 'mapped', label: 'Mapped', count: counts.mapped },
                { value: 'unmapped', label: 'Unmapped', count: counts.unmapped },
              ]}
            />
            {counts.unmapped > 0 && (
              <Button
                variant="ghost"
                size="md"
                onClick={handleClearUnmapped}
                title="Forget all unmapped folders. McpMux will offer to map them again next time those apps report a folder."
                className="whitespace-nowrap text-amber-600 hover:bg-amber-50 hover:text-amber-700 dark:text-amber-400 dark:hover:bg-amber-900/20"
                data-testid="workspaces-clear-unmapped"
              >
                <Trash2 className="mr-2 h-4 w-4" />
                Clear unmapped
              </Button>
            )}
          </div>
        </div>
      </header>

      {error && (
        <div className="flex-shrink-0 px-8 pt-6">
          <div className="mx-auto max-w-[2000px] rounded-xl border border-red-200 bg-red-50 p-4 text-base text-red-600 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400">
            {error}
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
            <EmptyState
              hasAny={entries.length > 0}
              hasFilter={searchQuery.length > 0 || filter !== 'all'}
              onCreate={() => setSelected({ mode: 'new' })}
            />
          ) : (
            <div className="auto-fill-cards grid gap-5">
              {filtered.map((entry) => {
                const isSelected = selected?.mode === 'entry' && selected.id === entry.id;
                // Mapped entries show their bound Space + FeatureSet names.
                // Unmapped entries read "Not mapped" — they fall back to the
                // default Starter set rather than to an explicit binding.
                const resolvedSpaceName = entry.binding
                  ? spaceById.get(entry.binding.space_id)?.name
                  : undefined;
                const fsNames = entry.binding
                  ? entry.binding.feature_set_ids.map((id) => fsById.get(id)?.name ?? id)
                  : [];
                return (
                  <EntryCard
                    key={entry.id}
                    entry={entry}
                    spaceName={resolvedSpaceName}
                    fsNames={fsNames}
                    clientName={clientNameForBinding(entry.binding)}
                    selected={isSelected}
                    onClick={() => setSelected({ mode: 'entry', id: entry.id })}
                  />
                );
              })}
            </div>
          )}
        </div>
      </div>

      {panelOpen && (
        <>
          <div
            className="animate-in fade-in fixed inset-0 z-40 bg-black/20 backdrop-blur-[2px] duration-200"
            onClick={() => setSelected(null)}
          />
          {selectedIsNew ? (
            <WorkspaceSetupWizard
              spaces={spaces}
              featureSets={featureSets}
              reportedRoots={reportedRoots}
              existingBindings={bindings}
              onClose={() => setSelected(null)}
              onCreate={async (input) => {
                const created = await handleCreate(input);
                // Land on the new mapping's inspector so its effective features
                // are shown right after creation.
                setSelected({ mode: 'entry', id: created.id });
                return created;
              }}
              onError={(msg) => showError('Could not save', msg)}
            />
          ) : (
            <InspectorPanel
              key={selectedEntry?.id ?? 'entry'}
              entry={selectedEntry}
              isNew={false}
              clientName={selectedClientName}
              spaces={spaces}
              featureSets={featureSets}
              existingBindings={bindings}
              onClose={() => setSelected(null)}
              onSubmit={async (input) => {
                if (selectedEntry?.binding) {
                  await handleUpdate(selectedEntry.binding.id, input);
                } else {
                  const created = await handleCreate(input);
                  setSelected({ mode: 'entry', id: created.id });
                }
              }}
              onDelete={async () => {
                if (selectedEntry?.binding) await handleDelete(selectedEntry.binding);
              }}
              onError={(msg) => showError('Could not save', msg)}
            />
          )}
        </>
      )}

      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Filter segmented control
// ---------------------------------------------------------------------------

/**
 * Render a list of FeatureSet names as a single string for display
 * surfaces (cards, badges, panel headers) where a multi-FS binding has
 * to fit on one line. Returns '' for empty input so callers can fall
 * back to a placeholder. Drops empty/missing entries silently — they're
 * already known to the caller as "fs not found", and there's nothing
 * useful to show.
 */
function formatFsList(names: string[]): string {
  return names.filter((n) => n && n.length > 0).join(' + ');
}

/**
 * Structural equality between two binding inputs. The edit form uses this
 * for its "dirty" check — Apply stays disabled until the current values
 * differ from what was loaded, so there's nothing to save on a no-op edit.
 * `feature_set_ids` order matters (it's the operator-chosen render order,
 * not just a set), so we compare positionally.
 */
function sameBindingInput(
  a: WorkspaceBindingInput,
  b: { workspace_root: string; space_id: string; feature_set_ids: string[] }
): boolean {
  if (a.workspace_root.trim() !== b.workspace_root.trim()) return false;
  if (a.space_id !== b.space_id) return false;
  if (a.feature_set_ids.length !== b.feature_set_ids.length) return false;
  return a.feature_set_ids.every((id, i) => id === b.feature_set_ids[i]);
}

function SegmentedFilter<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: Array<{ value: T; label: string; count?: number }>;
}) {
  return (
    <div className="inline-flex gap-0.5 rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-0.5">
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            onClick={() => onChange(o.value)}
            data-testid={`workspace-filter-${o.value}`}
            aria-pressed={active}
            className={[
              'inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium transition-all',
              active
                ? 'bg-[rgb(var(--background))] text-[rgb(var(--foreground))] shadow-sm'
                : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]',
            ].join(' ')}
          >
            {o.label}
            {typeof o.count === 'number' && (
              <span
                className={`inline-flex h-[1.125rem] min-w-[1.25rem] items-center justify-center rounded-full px-1 text-[10px] font-semibold ${
                  active
                    ? 'bg-[rgb(var(--surface))] text-[rgb(var(--foreground))]'
                    : 'bg-[rgb(var(--surface-hover,var(--surface)))] text-[rgb(var(--muted))]'
                }`}
              >
                {o.count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Entry card — a workspace folder and the tools it maps to. Matches the
// Clients/Servers card template: flat surface icon box + subtle border,
// w-14 icon, p-6, hover:scale. Folder name leads, full path beneath, and a
// feature-set summary that collapses to "first + N more" so it stays tidy
// no matter how many sets a folder maps to.
// ---------------------------------------------------------------------------

/** Last path segment — the folder's own name (`proj` from `/a/b/proj`). */
function folderName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Compact feature-set summary for the card footer. Lists up to two names,
 * then collapses to "first + N more" so a folder mapped to many sets doesn't
 * blow out the card. Full list is exposed via the `title` tooltip at the call
 * site.
 */
function summarizeFeatureSets(names: string[]): string {
  if (names.length === 0) return 'No tools';
  if (names.length <= 2) return names.join(' + ');
  return `${names[0]} + ${names.length - 1} more`;
}

/**
 * Per-status color. Soft flat tints (not heavy gradients) + a thin solid
 * accent strip — adds life and at-a-glance status while staying inside the
 * app's tinted-surface design language.
 */
const CARD_TONES = {
  emerald: {
    strip: 'bg-emerald-500',
    box: 'bg-emerald-50 text-emerald-600 ring-emerald-200/70 dark:bg-emerald-900/20 dark:text-emerald-400 dark:ring-emerald-800/50',
  },
  amber: {
    strip: 'bg-amber-500',
    box: 'bg-amber-50 text-amber-600 ring-amber-200/70 dark:bg-amber-900/20 dark:text-amber-400 dark:ring-amber-800/50',
  },
  neutral: {
    strip: 'bg-slate-300 dark:bg-slate-600',
    box: 'bg-[rgb(var(--surface))] text-[rgb(var(--muted))] ring-[rgb(var(--border-subtle))]',
  },
} as const;

function EntryCard({
  entry,
  spaceName,
  fsNames,
  clientName,
  selected,
  onClick,
}: {
  entry: Entry;
  spaceName: string | undefined;
  /** Resolved FeatureSet names for a mapped folder; empty when unmapped. */
  fsNames: string[];
  /** Set when this is an id binding McpMux auto-created for a registered
   *  client — its name is shown in place of the raw identifier. */
  clientName: string | null;
  selected: boolean;
  onClick: () => void;
}) {
  const tone =
    entry.kind === 'unmapped-live' ? 'amber' : entry.kind === 'mapped-live' ? 'emerald' : 'neutral';
  const t = CARD_TONES[tone];
  // For a client mapping the heading is the client's name; the raw identifier
  // stays on the mono line below for reference.
  const name = clientName ?? folderName(entry.root);

  return (
    <Card
      className={`relative cursor-pointer overflow-hidden transition-all hover:scale-[1.01] hover:shadow-lg ${
        selected ? 'ring-primary-500 shadow-lg ring-2' : ''
      }`}
      onClick={onClick}
      data-testid={`workspace-entry-${entry.id}`}
    >
      {/* Status accent strip across the top — a splash of color per state. */}
      <div className={`absolute inset-x-0 top-0 h-1 ${t.strip}`} />
      <CardContent className="p-6">
        <div className="mb-4 flex items-start gap-4">
          <div className="relative flex-shrink-0">
            <div
              className={`flex h-14 w-14 items-center justify-center rounded-xl ring-1 ring-inset ${t.box}`}
            >
              {entry.isLive ? <FolderOpen className="h-6 w-6" /> : <Folder className="h-6 w-6" />}
            </div>
            {entry.isLive && (
              <span
                className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-[rgb(var(--background))]"
                title="A client is active in this folder right now"
              />
            )}
          </div>
          <div className="min-w-0 flex-1">
            <div className="mb-1 flex flex-wrap items-center gap-2">
              {entry.kind === 'unmapped-live' && <Pill tone="amber">Unmapped</Pill>}
              {entry.kind === 'mapped-offline' && <Pill tone="neutral">Offline</Pill>}
              {entry.kind === 'mapped-live' && <Pill tone="emerald">Live</Pill>}
              {clientName && (
                <Pill tone="neutral">
                  <KeyRound className="mr-1 h-2.5 w-2.5" />
                  Client
                </Pill>
              )}
            </div>
            <h3 className="truncate text-base font-semibold" title={entry.root}>
              {name}
            </h3>
            <p className="truncate font-mono text-xs text-[rgb(var(--muted))]" title={entry.root}>
              {entry.root}
            </p>
          </div>
        </div>

        <div className="border-t border-[rgb(var(--border-subtle))] pt-4 text-xs">
          {entry.binding ? (
            <div className="flex items-center justify-between gap-3">
              <span className="inline-flex min-w-0 items-center gap-1.5">
                <Layers className="text-primary-500 h-3.5 w-3.5 flex-shrink-0" />
                <span
                  className="truncate font-medium text-[rgb(var(--foreground))]"
                  title={fsNames.join(', ')}
                >
                  {summarizeFeatureSets(fsNames)}
                </span>
                {fsNames.length > 1 && (
                  <span
                    className="bg-primary-500/10 text-primary-600 dark:text-primary-300 flex-shrink-0 rounded-full px-1.5 text-[10px] font-bold tabular-nums"
                    title={`${fsNames.length} feature sets`}
                  >
                    {fsNames.length}
                  </span>
                )}
              </span>
              <span className="inline-flex flex-shrink-0 items-center gap-1.5 text-[rgb(var(--muted))]">
                <span>in</span>
                <Chip tone="neutral">{spaceName ?? '—'}</Chip>
              </span>
            </div>
          ) : (
            <span className="inline-flex items-center gap-1.5 font-medium text-amber-600 dark:text-amber-400">
              <AlertCircle className="h-3.5 w-3.5 flex-shrink-0" />
              Not mapped — using your default Starter tools
            </span>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function Pill({
  children,
  tone,
}: {
  children: React.ReactNode;
  tone: 'amber' | 'emerald' | 'neutral';
}) {
  const cls =
    tone === 'amber'
      ? 'bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 border-amber-200/80 dark:border-amber-800/60'
      : tone === 'emerald'
        ? 'bg-emerald-50 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-400 border-emerald-200/80 dark:border-emerald-800/60'
        : 'bg-[rgb(var(--surface))] text-[rgb(var(--muted))] border-[rgb(var(--border-subtle))]';
  return (
    <span
      className={`inline-flex items-center rounded-md border px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider ${cls}`}
    >
      {children}
    </span>
  );
}

function Chip({ children, tone }: { children: React.ReactNode; tone: 'primary' | 'neutral' }) {
  const styles =
    tone === 'primary'
      ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300 border-primary-200 dark:border-primary-800/60'
      : 'bg-[rgb(var(--surface))] border-[rgb(var(--border-subtle))] text-[rgb(var(--foreground))]';
  return (
    <span
      className={`inline-flex items-center rounded-md border px-1.5 py-0.5 text-[11px] font-medium ${styles}`}
    >
      {children}
    </span>
  );
}

// ---------------------------------------------------------------------------
// CollapsibleSection — premium expandable card matching the FeatureSetPanel
// pattern (which the user already considers premium). border-2, gradient
// headers when expanded, icon-in-colored-box that fills white-on-tone when
// active, bold semibold titles. Used for both "Mapping" (terracotta) and
// "Effective features" (purple).
// ---------------------------------------------------------------------------

type SectionTone = 'primary' | 'purple';

interface SectionToneSpec {
  /** Header gradient bg when expanded. */
  gradientOpen: string;
  /** Icon container — collapsed (tinted bg). */
  iconQuiet: string;
  /** Icon container — expanded (solid fill, white glyph). */
  iconActive: string;
  /** Badge style when expanded (count chip). */
  badgeOpen: string;
}

const SECTION_TONES: Record<SectionTone, SectionToneSpec> = {
  primary: {
    gradientOpen:
      'bg-gradient-to-r from-primary-50 to-primary-100/50 dark:from-primary-900/20 dark:to-primary-800/10',
    iconQuiet: 'bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400',
    iconActive: 'bg-primary-500 text-white shadow-sm shadow-primary-500/30',
    badgeOpen:
      'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300 border border-primary-300/70 dark:border-primary-700/70',
  },
  purple: {
    gradientOpen:
      'bg-gradient-to-r from-purple-50 to-pink-50 dark:from-purple-900/20 dark:to-pink-900/15',
    iconQuiet: 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400',
    iconActive: 'bg-purple-500 text-white shadow-sm shadow-purple-500/30',
    badgeOpen:
      'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 border border-purple-300/70 dark:border-purple-700/70',
  },
};

function CollapsibleSection({
  icon,
  tone = 'primary',
  title,
  subtitle,
  defaultOpen = true,
  badge,
  headerExtra,
  testId,
  children,
}: {
  icon: React.ReactNode;
  tone?: SectionTone;
  title: string;
  subtitle?: React.ReactNode;
  defaultOpen?: boolean;
  badge?: number;
  /** Small element rendered next to the title (e.g. save status). */
  headerExtra?: React.ReactNode;
  testId?: string;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const t = SECTION_TONES[tone] ?? SECTION_TONES.primary;

  return (
    <div
      className="overflow-hidden rounded-xl border-2 border-[rgb(var(--border))] bg-[rgb(var(--background))] transition-all"
      data-testid={testId}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={[
          'flex w-full items-center justify-between p-4 transition-all',
          open ? t.gradientOpen : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]',
        ].join(' ')}
        aria-expanded={open}
      >
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <div
            className={[
              'flex-shrink-0 rounded-lg p-2 transition-colors duration-200',
              open ? t.iconActive : t.iconQuiet,
            ].join(' ')}
          >
            {icon}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-base font-semibold text-[rgb(var(--foreground))]">{title}</span>
              {typeof badge === 'number' && badge > 0 && (
                <span
                  className={[
                    'rounded-full px-2 py-0.5 text-xs font-bold tabular-nums',
                    open
                      ? t.badgeOpen
                      : 'border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface-dim))] text-[rgb(var(--muted))]',
                  ].join(' ')}
                >
                  {badge}
                </span>
              )}
              {headerExtra}
            </div>
            {subtitle && (
              <div className="mt-0.5 truncate text-xs text-[rgb(var(--muted))]">{subtitle}</div>
            )}
          </div>
        </div>
        {open ? (
          <ChevronDown className="h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
        ) : (
          <ChevronRight className="h-5 w-5 flex-shrink-0 text-[rgb(var(--muted))]" />
        )}
      </button>

      {open && (
        <div className="border-t-2 border-[rgb(var(--border))] bg-white p-4 dark:bg-[rgb(var(--background))]">
          {children}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Inspector side panel
// ---------------------------------------------------------------------------

type SaveStatus =
  | { kind: 'idle' }
  | { kind: 'saving' }
  | { kind: 'saved' }
  | { kind: 'error'; message: string };

function InspectorPanel({
  entry,
  isNew,
  clientName,
  spaces,
  featureSets,
  existingBindings,
  onClose,
  onSubmit,
  onDelete,
  onError,
}: {
  entry: Entry | null;
  isNew: boolean;
  /** Non-null when this is the managed id mapping for a registered client. */
  clientName: string | null;
  spaces: Space[];
  featureSets: FeatureSet[];
  existingBindings: WorkspaceBinding[];
  onClose: () => void;
  onSubmit: (input: WorkspaceBindingInput) => Promise<void>;
  onDelete: () => Promise<void>;
  onError: (msg: string) => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const isMapped = !!entry?.binding;
  // An id-keyed binding routes by header, not a folder path. When that id also
  // names a registered client (`clientName` set), the mapping is host-managed:
  // its identifier is read-only and it's removed with the client, not by hand.
  const isIdBinding = entry?.binding?.binding_type === 'id';
  const isManagedClient = !!clientName;
  const mode: 'create' | 'edit' | 'create-from-live' = isNew
    ? 'create'
    : isMapped
      ? 'edit'
      : 'create-from-live';
  const title = isNew
    ? 'New mapping'
    : isManagedClient
      ? 'Client mapping'
      : isMapped
        ? 'Workspace mapping'
        : 'Map this project';
  const subtitle = isNew
    ? 'Choose the tools a project should get.'
    : (clientName ?? entry?.root ?? '');

  // Auto-save status drives the small pill in the Mapping section header.
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ kind: 'idle' });

  // Effective-features count drives the badge in the section header so the
  // user can see scale without expanding.
  const [effectiveTotal, setEffectiveTotal] = useState<number | null>(null);

  return (
    <div className="animate-in slide-in-from-right fixed bottom-0 right-0 top-0 z-50 flex w-full min-w-[420px] max-w-[480px] flex-col border-l border-[rgb(var(--border))] bg-[rgb(var(--surface))] shadow-2xl duration-300">
      <div className="flex-shrink-0 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        <div className="flex items-start justify-between">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <div className="flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))]">
              <FolderOpen className="h-5 w-5 text-[rgb(var(--muted))]" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="mb-0.5 flex flex-wrap items-center gap-2">
                {!isNew && entry?.isLive && <Pill tone="emerald">Live</Pill>}
                {!isNew && entry && !isMapped && <Pill tone="amber">Unmapped</Pill>}
                {!isNew && entry && isMapped && !entry.isLive && (
                  <Pill tone="neutral">Offline</Pill>
                )}
              </div>
              <h2 className="truncate text-lg font-bold">{title}</h2>
              <p
                className={`truncate text-xs text-[rgb(var(--muted))] ${!isNew ? 'font-mono' : ''}`}
                title={subtitle}
              >
                {subtitle}
              </p>
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

      <div className="flex-1 space-y-5 overflow-y-auto p-6">
        <CollapsibleSection
          icon={<FolderOpen className="h-5 w-5" />}
          tone="primary"
          title="Mapping"
          subtitle={
            mode === 'create'
              ? 'Choose the project and the tools it should get.'
              : mode === 'create-from-live'
                ? 'This project is open in an app and using your default Starter tools — map it to give it a specific set instead.'
                : isMapped && entry?.binding
                  ? `Gives ${
                      formatFsList(
                        entry.binding!.feature_set_ids.map(
                          (id) => featureSets.find((f) => f.id === id)?.name ?? id
                        )
                      ) || '—'
                    } from ${spaces.find((s) => s.id === entry.binding!.space_id)?.name ?? '—'}`
                  : 'Edit what this folder sees, then press Apply.'
          }
          defaultOpen={isNew || !isMapped}
          headerExtra={<SaveStatusPill status={saveStatus} />}
          testId="workspace-mapping-section"
        >
          <BindingForm
            mode={mode}
            spaces={spaces}
            featureSets={featureSets}
            initial={entry?.binding ?? null}
            prefillRoot={entry && !isMapped ? entry.root : undefined}
            existingBindings={existingBindings}
            managedClientName={clientName}
            onCancel={onClose}
            onSubmit={onSubmit}
            onError={onError}
            onSaveStatusChange={setSaveStatus}
          />
        </CollapsibleSection>

        {entry && !isNew && (
          <CollapsibleSection
            icon={<Wrench className="h-5 w-5" />}
            tone="primary"
            title={
              isIdBinding
                ? isManagedClient
                  ? 'Connect this client'
                  : 'Connect a client'
                : 'Connect to this project'
            }
            subtitle={
              isIdBinding
                ? isManagedClient
                  ? 'How this client reaches the gateway — its API key already routes it here.'
                  : 'Copy a ready-to-paste MCP config — its workspace header is pinned to this identifier.'
                : 'See what this project applies, copy a ready-to-paste config, or write it into the project for the apps you use.'
            }
            defaultOpen={true}
            testId="workspace-install-section"
          >
            {isIdBinding ? (
              <ConnectConfigPanel
                variant={isManagedClient ? 'client' : 'id'}
                headerValue={entry.binding!.workspace_root}
              />
            ) : (
              <div className="space-y-4">
                {entry.binding && (
                  <>
                    <AppliedSettings
                      spaceName={spaces.find((s) => s.id === entry.binding!.space_id)?.name ?? '—'}
                      fsNames={
                        formatFsList(
                          entry.binding!.feature_set_ids.map(
                            (id) => featureSets.find((f) => f.id === id)?.name ?? id
                          )
                        ) || '—'
                      }
                    />
                    <ConnectConfigPanel variant="path" headerValue={entry.root} />
                  </>
                )}
                <WorkspaceInstallPanel workspaceRoot={entry.root} />
              </div>
            )}
          </CollapsibleSection>
        )}

        {entry && !isNew && (
          <CollapsibleSection
            icon={<Layers className="h-5 w-5" />}
            tone="purple"
            title="Effective Features"
            subtitle="Tools, prompts, and resources this folder currently sees"
            defaultOpen={true}
            badge={effectiveTotal ?? undefined}
            testId="workspace-effective-features-section"
          >
            <EffectiveFeaturesContent root={entry.root} onTotalChange={setEffectiveTotal} />
          </CollapsibleSection>
        )}
      </div>

      {entry?.binding && isManagedClient && (
        <div className="flex-shrink-0 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
          <p
            className="flex items-start gap-2 text-xs text-[rgb(var(--muted))]"
            data-testid="workspace-binding-managed-note"
          >
            <KeyRound className="mt-0.5 h-3.5 w-3.5 flex-shrink-0" />
            <span>
              Managed by the client <strong>{clientName}</strong>. Remove it by deleting the client
              from the Clients tab.
            </span>
          </p>
        </div>
      )}
      {entry?.binding && !isManagedClient && (
        <div className="flex-shrink-0 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void onDelete()}
            className="w-full text-red-600 hover:bg-red-50 hover:text-red-700 dark:hover:bg-red-900/20"
            data-testid={`workspace-binding-delete-${entry.binding.id}`}
          >
            <Trash2 className="mr-2 h-4 w-4" />
            Remove mapping
          </Button>
        </div>
      )}
    </div>
  );
}

function SaveStatusPill({ status }: { status: SaveStatus }) {
  if (status.kind === 'idle') return null;
  const base =
    'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider border';
  if (status.kind === 'saving') {
    return (
      <span
        className={`${base} border-[rgb(var(--border))] bg-[rgb(var(--surface-dim))] text-[rgb(var(--muted))]`}
      >
        <Loader2 className="h-2.5 w-2.5 animate-spin" />
        Saving
      </span>
    );
  }
  if (status.kind === 'saved') {
    return (
      <span
        className={`${base} animate-in fade-in border-green-300/70 bg-green-100 text-green-700 duration-200 dark:border-green-700/70 dark:bg-green-900/30 dark:text-green-300`}
      >
        <Check className="h-2.5 w-2.5" strokeWidth={2.5} />
        Saved
      </span>
    );
  }
  return (
    <span
      className={`${base} border-red-200 bg-red-50 text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400`}
      title={status.message}
    >
      <AlertCircle className="h-2.5 w-2.5" />
      Error
    </span>
  );
}

// ---------------------------------------------------------------------------
// Connect-config panel + applied-settings summary for explicitly-targeted
// mappings (id, client, or a project addressed by header)
// ---------------------------------------------------------------------------

/**
 * Compact "what this mapping applies" summary — the resolved Space + FeatureSet
 * names — shown at the top of a project mapping's connect section so the user
 * sees the routing outcome before the copy / install controls.
 */
function AppliedSettings({ spaceName, fsNames }: { spaceName: string; fsNames: string }) {
  return (
    <div
      className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3"
      data-testid="workspace-applied-settings"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[10px] font-bold uppercase tracking-wider text-[rgb(var(--muted))]">
          Applies
        </span>
        <span className="inline-flex min-w-0 items-center gap-1.5">
          <Layers className="text-primary-500 h-3.5 w-3.5 flex-shrink-0" />
          <span className="truncate text-sm font-semibold text-[rgb(var(--foreground))]">
            {fsNames}
          </span>
        </span>
        <span className="text-xs text-[rgb(var(--muted))]">in</span>
        <span className="text-sm font-medium text-[rgb(var(--foreground))]">{spaceName}</span>
      </div>
    </div>
  );
}

/**
 * Connect surface for a mapping a client targets explicitly. Hands over a
 * ready-to-paste MCP config plus a SECOND copy variant, framed by the routing
 * model:
 *
 *   • `client` — the API key identifies the client, so mcpmux routes it here
 *     automatically (even with inbound auth off). Shows ONLY the Bearer config;
 *     this mapping is the client's default when it sends no workspace override.
 *   • `id` / `path` — routed by the `X-Mcpmux-Workspace` header pinned to this
 *     mapping's identifier (an id string) or project path. Primary copy is the
 *     header config; the secondary adds a Bearer key for when inbound auth is on.
 *
 * Falls back to the default local endpoint when the gateway isn't running so
 * the snippet is still copy-paste useful. The copied text is the `"mcpmux": …`
 * server entry, ready to drop into a client's existing `mcpServers` block.
 */
function ConnectConfigPanel({
  headerValue,
  variant,
}: {
  /** The `X-Mcpmux-Workspace` value this mapping matches (project path or id). */
  headerValue: string;
  variant: 'id' | 'path' | 'client';
}) {
  const [mcpUrl, setMcpUrl] = useState<string | null>(null);
  const [authDisabled, setAuthDisabled] = useState<boolean | null>(null);
  const [copied, setCopied] = useState<'primary' | 'secondary' | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [status, disabled] = await Promise.all([
        getGatewayStatus().catch(() => null),
        getGatewayAuthDisabled().catch(() => null),
      ]);
      if (cancelled) return;
      setMcpUrl(status?.url ? `${status.url}/mcp` : null);
      setAuthDisabled(disabled);
      setLoading(false);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const endpoint = mcpUrl ?? DEFAULT_MCP_ENDPOINT;
  const isClient = variant === 'client';

  // Client: just the Bearer config (the API key already routes it here).
  // id/path: the header config is primary, the Bearer variant the "auth is
  // on" add-on (second copy button).
  const primaryConfig = useMemo(
    () =>
      isClient
        ? buildMcpConfig({ endpoint, bearer: true })
        : buildMcpConfig({ endpoint, workspace: headerValue }),
    [endpoint, headerValue, isClient]
  );
  const secondaryConfig = useMemo(
    () => buildMcpConfig({ endpoint, workspace: headerValue, bearer: true }),
    [endpoint, headerValue]
  );

  const copy = async (which: 'primary' | 'secondary') => {
    try {
      await navigator.clipboard.writeText(which === 'primary' ? primaryConfig : secondaryConfig);
      setCopied(which);
      setTimeout(() => setCopied(null), 1500);
    } catch {
      /* clipboard unavailable — ignore */
    }
  };

  return (
    <div className="space-y-3" data-testid="workspace-id-config-panel">
      {isClient ? (
        <p className="text-sm text-[rgb(var(--muted))]">
          Your API key identifies this client, so mcpmux routes it to this mapping automatically — no
          workspace header needed (works even with inbound auth disabled).
        </p>
      ) : variant === 'path' ? (
        <p className="text-sm text-[rgb(var(--muted))]">
          This project is matched automatically when your client reports its folder as an MCP root.
          To target it explicitly from any client, paste this config — its{' '}
          <code className="text-xs">X-Mcpmux-Workspace</code> header is pinned to this project&apos;s
          path, so the client routes here even without reporting the folder.
        </p>
      ) : (
        <p className="text-sm text-[rgb(var(--muted))]">
          This mapping routes by a header. Paste this into your client&apos;s MCP config — its{' '}
          <code className="text-xs">X-Mcpmux-Workspace</code> header is pinned to this mapping, so
          the client gets exactly these tools.
        </p>
      )}

      {isClient && (
        <p className="text-xs text-[rgb(var(--muted))]">
          This mapping is used when no <code className="text-xs">X-Mcpmux-Workspace</code> header is
          passed for this API client.
        </p>
      )}

      {authDisabled === false && !isClient && (
        <p className="rounded-lg border border-amber-200 bg-amber-50 p-2.5 text-xs text-amber-800 dark:border-amber-800/60 dark:bg-amber-900/20 dark:text-amber-300">
          Authentication is on — use <strong>{COPY_CONFIG_BEARER_LABEL}</strong> and swap in this
          client&apos;s API key.
        </p>
      )}

      <pre className="overflow-x-auto rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] p-3 font-mono text-xs leading-relaxed">
        {primaryConfig}
      </pre>

      <div className="flex flex-wrap gap-2">
        <Button
          variant="primary"
          size="sm"
          className="flex-1"
          onClick={() => void copy('primary')}
          disabled={loading}
          data-testid="workspace-id-config-copy"
        >
          {copied === 'primary' ? (
            <Check className="mr-2 h-4 w-4" />
          ) : (
            <Copy className="mr-2 h-4 w-4" />
          )}
          {copied === 'primary'
            ? COPIED_LABEL
            : // The client variant's single button copies the Bearer config, so
              // it reads "Copy with Bearer" to match the shared pattern (a
              // bearer config is always "Copy with Bearer"). The header/path
              // variants' primary is the plain config → "Copy config".
              isClient
              ? COPY_CONFIG_BEARER_LABEL
              : COPY_CONFIG_LABEL}
        </Button>
        {!isClient && (
          <Button
            variant="secondary"
            size="sm"
            className="flex-1"
            onClick={() => void copy('secondary')}
            disabled={loading}
            data-testid="workspace-id-config-copy-bearer"
          >
            {copied === 'secondary' ? (
              <Check className="mr-2 h-4 w-4" />
            ) : (
              <Copy className="mr-2 h-4 w-4" />
            )}
            {copied === 'secondary' ? COPIED_LABEL : COPY_CONFIG_BEARER_LABEL}
          </Button>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Effective features — what tools / prompts / resources this folder sees
// right now, grouped by backend server so the user can see at a glance
// "github is fine, but my-search is disconnected so 4 tools are dark."
// Mirrors the expandable-section pattern from the old Clients-page panel.
// ---------------------------------------------------------------------------

interface ServerGroup {
  server_id: string;
  server_alias: string;
  server_status: EffectiveFeature['server_status'];
  available: boolean;
  tools: EffectiveFeature[];
  prompts: EffectiveFeature[];
  resources: EffectiveFeature[];
  /** Mapped count for this server in the resolved FS (= tools+prompts+resources lengths). */
  mapped: number;
  /** Total count of features the server exposes in the resolved Space, regardless of FS. */
  server_total: number;
  /** Of `mapped`, how many are unavailable because the server is disconnected. */
  unavailable_mapped: number;
}

function buildServerGroups(data: WorkspaceEffectiveFeatures): ServerGroup[] {
  const map = new Map<string, ServerGroup>();
  const place = (item: EffectiveFeature, kind: 'tool' | 'prompt' | 'resource') => {
    let g = map.get(item.server_id);
    if (!g) {
      const totals = data.server_totals[item.server_id];
      const server_total = totals ? totals.tools + totals.prompts + totals.resources : 0;
      g = {
        server_id: item.server_id,
        server_alias: item.server_alias ?? item.server_id,
        // Per-feature status is the same across a server (status comes
        // from the server, not the feature) — pick the first one we see.
        server_status: item.server_status,
        available: item.available,
        tools: [],
        prompts: [],
        resources: [],
        mapped: 0,
        server_total,
        unavailable_mapped: 0,
      };
      map.set(item.server_id, g);
    }
    if (kind === 'tool') g.tools.push(item);
    else if (kind === 'prompt') g.prompts.push(item);
    else g.resources.push(item);
    g.mapped += 1;
    if (!item.available) g.unavailable_mapped += 1;
  };
  for (const t of data.tools) place(t, 'tool');
  for (const p of data.prompts) place(p, 'prompt');
  for (const r of data.resources) place(r, 'resource');
  // Sort: connected first, then by alias.
  return Array.from(map.values()).sort((a, b) => {
    if (a.available !== b.available) return a.available ? -1 : 1;
    return a.server_alias.localeCompare(b.server_alias);
  });
}

/**
 * Body of the Effective-features collapsible. The outer card / header /
 * chevron lives in `CollapsibleSection`; this component just renders the
 * resolved-to summary and the per-server expandable groups.
 *
 * Reports the configured-features total to the parent via `onTotalChange`
 * so the section header can show a count badge without re-fetching.
 */
function EffectiveFeaturesContent({
  root,
  onTotalChange,
}: {
  root: string;
  onTotalChange?: (total: number | null) => void;
}) {
  const [data, setData] = useState<WorkspaceEffectiveFeatures | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [openServers, setOpenServers] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    let cancelled = false;
    // Standard fetch-on-prop-change pattern: synchronous resets ensure the
    // UI doesn't show stale data from a previous root while a new fetch
    // is in flight. The lint rule is overly strict for this idiom.
    /* eslint-disable react-hooks/set-state-in-effect */
    setLoading(true);
    setError(null);
    onTotalChange?.(null);
    /* eslint-enable react-hooks/set-state-in-effect */
    void getWorkspaceEffectiveFeatures(root)
      .then((d) => {
        if (cancelled) return;
        setData(d);
        const total = d.tools.length + d.prompts.length + d.resources.length;
        onTotalChange?.(total);
        const groups = buildServerGroups(d);
        if (groups.length > 0) {
          setOpenServers(new Set([groups[0].server_id]));
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(typeof e === 'string' ? e : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [root, onTotalChange]);

  // Re-fetch on binding / server-status changes so the panel stays honest
  // without the user reopening it.
  useEffect(() => {
    let cancelled = false;
    const reload = () => {
      void getWorkspaceEffectiveFeatures(root)
        .then((d) => {
          if (cancelled) return;
          setData(d);
          onTotalChange?.(d.tools.length + d.prompts.length + d.resources.length);
        })
        .catch(() => {
          /* ignore — initial load already surfaced any error */
        });
    };
    const unBinding = listen('workspace-binding-changed', reload);
    const unServer = listen('server-status-changed', reload);
    return () => {
      cancelled = true;
      unBinding.then((fn) => fn());
      unServer.then((fn) => fn());
    };
  }, [root, onTotalChange]);

  // All hooks must run on every render — keep them above any early
  // returns so React's hook-order invariant holds.
  const groups = useMemo(() => (data ? buildServerGroups(data) : []), [data]);
  const totalCount = data ? data.tools.length + data.prompts.length + data.resources.length : 0;
  const availableCount = useMemo(
    () => groups.reduce((acc, g) => acc + (g.mapped - g.unavailable_mapped), 0),
    [groups]
  );

  const toggleServer = (id: string) => {
    setOpenServers((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  if (loading && !data) {
    return (
      <div className="flex items-center justify-center py-6">
        <Loader2 className="h-6 w-6 animate-spin text-purple-500" />
      </div>
    );
  }
  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-600 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400">
        <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0" />
        <span>{error}</span>
      </div>
    );
  }
  if (!data) return null;

  const allAvailable = totalCount > 0 && availableCount === totalCount;
  const partialAvailable = availableCount > 0 && availableCount < totalCount;

  return (
    <div className="space-y-4">
      {/* Resolution summary — bold pills showing what this folder
          resolves to, plus a progress bar for availability. */}
      <div className="space-y-2.5 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[10px] font-bold uppercase tracking-wider text-[rgb(var(--muted))]">
            Resolves to
          </span>
          <span className="truncate text-sm font-semibold text-[rgb(var(--foreground))]">
            {formatFsList(data.feature_sets.map((fs) => fs.name)) || '—'}
          </span>
          <span className="text-xs text-[rgb(var(--muted))]">in</span>
          <span className="text-sm font-medium text-[rgb(var(--foreground))]">
            {data.space_name}
          </span>
          <span
            title={
              data.source === 'binding'
                ? 'A workspace binding matched this project — live sessions reporting it route here.'
                : 'No binding matches this project, so it falls back to the default Starter set shown here. Map it to give this project a different set.'
            }
            className={[
              'ml-auto rounded-full border px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider',
              data.source === 'binding'
                ? 'border-purple-300/70 bg-purple-100 text-purple-700 dark:border-purple-700/70 dark:bg-purple-900/30 dark:text-purple-300'
                : 'border-amber-300/70 bg-amber-100 text-amber-700 dark:border-amber-700/70 dark:bg-amber-900/30 dark:text-amber-300',
            ].join(' ')}
          >
            {data.source === 'binding' ? 'binding' : 'unbound'}
          </span>
        </div>

        {/* Availability progress bar. Stays quiet (green) when all servers
            are connected, leans amber when some are dim. */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs">
            <span className="tabular-nums text-[rgb(var(--muted))]">
              <span className="font-semibold text-[rgb(var(--foreground))]">{availableCount}</span>
              <span> of </span>
              <span className="font-semibold text-[rgb(var(--foreground))]">{totalCount}</span>
              <span> available</span>
            </span>
            {totalCount > 0 && (
              <span
                className={[
                  'text-[10px] font-bold uppercase tracking-wider',
                  allAvailable
                    ? 'text-green-600 dark:text-green-400'
                    : partialAvailable
                      ? 'text-amber-600 dark:text-amber-400'
                      : 'text-[rgb(var(--muted))]',
                ].join(' ')}
              >
                {allAvailable ? 'All ready' : partialAvailable ? 'Partial' : 'Offline'}
              </span>
            )}
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
            <div
              className={[
                'h-full transition-all duration-300',
                totalCount === 0
                  ? 'bg-gray-400 dark:bg-gray-600'
                  : allAvailable
                    ? 'bg-gradient-to-r from-green-500 to-emerald-500'
                    : partialAvailable
                      ? 'bg-gradient-to-r from-amber-500 to-green-500'
                      : 'bg-gray-400 dark:bg-gray-600',
              ].join(' ')}
              style={{
                width: totalCount > 0 ? `${(availableCount / totalCount) * 100}%` : '0%',
              }}
            />
          </div>
        </div>
      </div>

      {/* Server-grouped feature list. */}
      {groups.length === 0 ? (
        <div className="py-8 text-center text-[rgb(var(--muted))]">
          <Package className="mx-auto mb-2 h-8 w-8 opacity-50" />
          <p className="text-sm">No features configured in this feature set yet.</p>
        </div>
      ) : (
        <div className="overflow-hidden rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))]">
          <div className="divide-y divide-[rgb(var(--border))]">
            {groups.map((g) => (
              <ServerGroupRow
                key={g.server_id}
                group={g}
                open={openServers.has(g.server_id)}
                onToggle={() => toggleServer(g.server_id)}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function ServerGroupRow({
  group,
  open,
  onToggle,
}: {
  group: ServerGroup;
  open: boolean;
  onToggle: () => void;
}) {
  const issue = serverStatusIssue(group.server_status);
  const availableCount = group.mapped - group.unavailable_mapped;
  // Badge denominator is the server's *total* feature count in the Space,
  // not the mapped count — the user wants to see "3 of 10 cloudflare-docs
  // tools are in this FS" rather than "3 of 3 mapped tools work".
  const denominator = group.server_total > 0 ? group.server_total : group.mapped;
  const allAvailable = group.mapped > 0 && availableCount === group.mapped;
  const someAvailable = availableCount > 0 && availableCount < group.mapped;
  const noneAvailable = availableCount === 0;

  // Strip reverse-DNS prefix so display reads "cloudflare-bindings" not
  // "com.cloudflare-bindings". The full id stays in title for hover.
  const prefix = group.server_alias.includes('.') ? group.server_alias.split('.', 2)[0] : null;
  const displayName = prefix ? group.server_alias.slice(prefix.length + 1) : group.server_alias;

  return (
    <div className="bg-[rgb(var(--surface))]">
      <div
        className="flex cursor-pointer items-center justify-between px-4 py-3 transition-colors hover:bg-[rgb(var(--surface-hover))]"
        onClick={onToggle}
        role="button"
        title={group.server_alias}
      >
        <div className="flex min-w-0 flex-1 items-center gap-3">
          {open ? (
            <ChevronDown className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
          ) : (
            <ChevronRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
          )}
          <ServerIcon className="h-4 w-4 flex-shrink-0 text-blue-500" />
          <div className="min-w-0 flex-1">
            <div className="mb-1 flex flex-wrap items-center gap-2">
              {prefix && (
                <span className="font-mono text-[10px] text-[rgb(var(--muted))]">{prefix}.</span>
              )}
              <span className="truncate font-mono text-sm font-medium">{displayName}</span>
              <span
                className={[
                  'flex-shrink-0 rounded-full px-2 py-0.5 text-xs font-bold tabular-nums',
                  noneAvailable
                    ? 'border border-gray-300/70 bg-gray-100 text-gray-600 dark:border-gray-700/70 dark:bg-gray-900/30 dark:text-gray-400'
                    : allAvailable
                      ? 'border border-green-300/70 bg-green-100 text-green-700 dark:border-green-700/70 dark:bg-green-900/30 dark:text-green-300'
                      : 'border border-amber-300/70 bg-amber-100 text-amber-700 dark:border-amber-700/70 dark:bg-amber-900/30 dark:text-amber-300',
                ].join(' ')}
              >
                {group.mapped}/{denominator}
              </span>
              {issue && (
                <span
                  className={[
                    'rounded-full border px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wider',
                    issue.tone === 'red'
                      ? 'border-red-200 bg-red-50 text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400'
                      : issue.tone === 'amber'
                        ? 'border-amber-200 bg-amber-50 text-amber-700 dark:border-amber-800 dark:bg-amber-900/20 dark:text-amber-400'
                        : 'border-gray-200 bg-gray-50 text-gray-600 dark:border-gray-800 dark:bg-gray-900/20 dark:text-gray-400',
                  ].join(' ')}
                >
                  {issue.label}
                </span>
              )}
            </div>
            {/* Per-server progress bar — same treatment as FeatureSetPanel's
                server rows so the visual language is consistent. */}
            <div className="h-1 overflow-hidden rounded-full bg-gray-200 dark:bg-gray-800">
              <div
                className={[
                  'h-full transition-all duration-300',
                  noneAvailable
                    ? 'bg-gray-400 dark:bg-gray-600'
                    : allAvailable
                      ? 'bg-green-500'
                      : someAvailable
                        ? 'bg-gradient-to-r from-amber-500 to-green-500'
                        : 'bg-gray-400',
                ].join(' ')}
                style={{
                  width: group.mapped > 0 ? `${(availableCount / group.mapped) * 100}%` : '0%',
                }}
              />
            </div>
          </div>
        </div>
      </div>

      {open && (
        <div className="border-t border-[rgb(var(--border))] bg-[rgb(var(--background))]">
          <FeatureSubGroup label="tool" items={group.tools} />
          <FeatureSubGroup label="prompt" items={group.prompts} />
          <FeatureSubGroup label="resource" items={group.resources} />
        </div>
      )}
    </div>
  );
}

/**
 * Indented feature rows inside an expanded server group. Mirrors the
 * FeatureSetPanel feature rows: type icon + name + type pill +
 * description, indented `pl-12` to align under the server icon.
 */
function FeatureSubGroup({
  label,
  items,
}: {
  label: 'tool' | 'prompt' | 'resource';
  items: EffectiveFeature[];
}) {
  if (items.length === 0) return null;
  return (
    <>
      {items.map((item) => (
        <div
          key={item.id}
          className={[
            'flex items-start gap-3 border-b border-[rgb(var(--border))] px-4 py-2.5 pl-12 last:border-b-0',
            !item.available ? 'opacity-50' : '',
          ].join(' ')}
          title={item.description ?? item.feature_name}
        >
          {getFeatureTypeIcon(label)}
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="truncate font-mono text-sm font-medium">
                {item.display_name || item.feature_name}
              </span>
              <span
                className={[
                  'rounded px-1.5 py-0.5 text-[10px] font-medium',
                  getFeatureTypeColor(label),
                ].join(' ')}
              >
                {label}
              </span>
              {!item.available && (
                <span className="text-[9px] font-bold uppercase tracking-wider text-[rgb(var(--muted))]">
                  unavailable
                </span>
              )}
            </div>
            {item.description && (
              <p className="mt-0.5 line-clamp-1 text-xs text-[rgb(var(--muted))]">
                {item.description}
              </p>
            )}
          </div>
        </div>
      ))}
    </>
  );
}

function getFeatureTypeIcon(type: 'tool' | 'prompt' | 'resource') {
  switch (type) {
    case 'tool':
      return <Wrench className="mt-0.5 h-4 w-4 flex-shrink-0 text-purple-500" />;
    case 'prompt':
      return <MessageSquare className="mt-0.5 h-4 w-4 flex-shrink-0 text-blue-500" />;
    case 'resource':
      return <FileText className="mt-0.5 h-4 w-4 flex-shrink-0 text-green-500" />;
  }
}

function getFeatureTypeColor(type: 'tool' | 'prompt' | 'resource'): string {
  switch (type) {
    case 'tool':
      return 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300';
    case 'prompt':
      return 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300';
    case 'resource':
      return 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300';
  }
}

/**
 * Translate a server status into a small UI annotation — but ONLY when
 * the status warrants attention. The healthy "connected" path returns
 * null so the row stays quiet.
 */
function serverStatusIssue(
  status: EffectiveFeature['server_status']
): { label: string; tone: 'red' | 'amber' | 'muted' } | null {
  switch (status) {
    case 'connected':
      return null;
    case 'connecting':
      return { label: 'Connecting', tone: 'amber' };
    case 'authenticating':
      return { label: 'Authenticating', tone: 'amber' };
    case 'refreshing':
      return { label: 'Refreshing', tone: 'amber' };
    case 'auth_required':
      return { label: 'Auth needed', tone: 'amber' };
    case 'error':
      return { label: 'Error', tone: 'red' };
    case 'disconnected':
      return { label: 'Disconnected', tone: 'muted' };
    case 'unknown':
    default:
      return { label: 'Offline', tone: 'muted' };
  }
}

// ---------------------------------------------------------------------------
// Binding form
// ---------------------------------------------------------------------------

function BindingForm({
  mode,
  spaces,
  featureSets,
  initial,
  prefillRoot,
  existingBindings,
  managedClientName,
  onCancel,
  onSubmit,
  onError,
  onSaveStatusChange,
}: {
  mode: 'create' | 'edit' | 'create-from-live';
  spaces: Space[];
  featureSets: FeatureSet[];
  initial?: WorkspaceBinding | null;
  prefillRoot?: string;
  /** Every saved mapping, used to flag a folder that's already mapped. */
  existingBindings: WorkspaceBinding[];
  /** Set when editing the managed id mapping of a registered client — its
   *  identifier becomes read-only (it's fixed at client registration). */
  managedClientName?: string | null;
  onCancel: () => void;
  onSubmit: (input: WorkspaceBindingInput) => Promise<void>;
  onError: (message: string) => void;
  /** Surfaced upward so the section header can show a Saving / Saved pill. */
  onSaveStatusChange?: (status: SaveStatus) => void;
}) {
  const defaultSpaceId = useMemo(
    () => spaces.find((s) => s.is_default)?.id ?? spaces[0]?.id ?? '',
    [spaces]
  );

  const rootRef = useRef<HTMLInputElement | null>(null);
  const [root, setRoot] = useState(initial?.workspace_root ?? prefillRoot ?? '');
  const [spaceId, setSpaceId] = useState<string>(initial?.space_id ?? defaultSpaceId);
  // Multi-FS: a binding may resolve to N FeatureSets (the resolver merges
  // their members into one allow set). Order is preserved so the operator
  // can rank a "primary" FS first; the resolver itself doesn't care.
  const [fsIds, setFsIds] = useState<string[]>(initial?.feature_set_ids ?? []);
  // A mapping is keyed by a folder path OR an arbitrary id/label. The type is
  // chosen at create time and fixed thereafter (an id never becomes a folder).
  // Mapping type is chosen in the create wizard and fixed thereafter; here
  // (edit / create-from-live) we only read it so an id mapping isn't
  // re-validated as a filesystem path.
  const bindingType = initial?.binding_type ?? 'path';
  const isId = bindingType === 'id';
  // A managed client mapping owns its identifier (it's the client_id, set at
  // registration) — show it read-only so it can't drift out of sync.
  const isManagedClient = !!managedClientName;
  const [fsSearch, setFsSearch] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const isEdit = mode === 'edit';

  // Holds the timer that clears the transient "Saved" pill after an
  // explicit Apply. Cleared if another save starts or the panel unmounts.
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    return () => {
      if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
    };
  }, []);

  // Live validation of the workspace_root field. Edit + create-from-live
  // modes already have a trusted root (edit: the persisted one; create-from-
  // live: came from the MCP client), so we skip validation for those — only
  // manual creates / edits to the path need the live check.
  const [rootValidation, setRootValidation] = useState<
    | { state: 'idle' }
    | { state: 'checking' }
    | { state: 'ok'; normalized: string }
    | { state: 'error'; reason: string }
  >({ state: 'idle' });
  const validationSeq = useRef(0);

  // A live-reported folder has a fixed path; a managed client mapping has a
  // fixed identifier. Both are surfaced read-only.
  const rootEditable = mode !== 'create-from-live' && !isManagedClient;

  useEffect(() => {
    if (!rootEditable) {
      setRootValidation({ state: 'ok', normalized: root });
      return;
    }
    if (isId) {
      // Id keys are matched verbatim — skip filesystem-path validation.
      setRootValidation(root.trim() ? { state: 'ok', normalized: root.trim() } : { state: 'idle' });
      return;
    }
    if (!root.trim()) {
      setRootValidation({ state: 'idle' });
      return;
    }
    // Debounce a little so we don't hammer the IPC on every keystroke.
    const seq = ++validationSeq.current;
    setRootValidation({ state: 'checking' });
    const handle = setTimeout(() => {
      void validateWorkspaceRoot(root)
        .then((normalized) => {
          if (validationSeq.current !== seq) return;
          setRootValidation({ state: 'ok', normalized });
        })
        .catch((e: unknown) => {
          if (validationSeq.current !== seq) return;
          const reason = typeof e === 'string' ? e : String(e);
          setRootValidation(reason === '' ? { state: 'idle' } : { state: 'error', reason });
        });
    }, 180);
    return () => clearTimeout(handle);
  }, [root, rootEditable, isId]);

  useEffect(() => {
    if (mode === 'create') rootRef.current?.focus();
  }, [mode]);

  const availableFs = useMemo(
    () => featureSets.filter((f) => f.space_id === spaceId && !f.is_deleted),
    [featureSets, spaceId]
  );

  // Filter the available FS list by the search query. Search runs against
  // name + description, case-insensitive — matches the typeahead expectation
  // most operators bring from the FeatureSets editor.
  const filteredFs = useMemo(() => {
    const q = fsSearch.trim().toLowerCase();
    if (!q) return availableFs;
    return availableFs.filter((f) => {
      if (f.name.toLowerCase().includes(q)) return true;
      if (f.description?.toLowerCase().includes(q)) return true;
      return false;
    });
  }, [availableFs, fsSearch]);

  // When the Space changes, drop selections that aren't in the new Space's
  // FS list. In CREATE modes only, reseed an empty selection with the default
  // FS so the operator doesn't have to click anything for the common case.
  // In EDIT mode we never reseed — an intentionally-empty mapping ("no Space
  // tools") must survive reopening.
  useEffect(() => {
    if (availableFs.length === 0) {
      if (fsIds.length > 0) setFsIds([]);
      return;
    }
    const validIds = new Set(availableFs.map((f) => f.id));
    const filtered = fsIds.filter((id) => validIds.has(id));
    if (filtered.length !== fsIds.length) {
      // Cross-space cleanup: drop ids that don't belong to this Space.
      setFsIds(filtered);
    } else if (filtered.length === 0 && !initial) {
      const fallback = availableFs.find(isStarterFeatureSet) ?? availableFs[0];
      setFsIds([fallback.id]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [availableFs]);

  const toggleFs = (id: string) => {
    setFsIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const trimmedRoot = root.trim();
  // The canonical form the server will store. We prefer the validator's
  // normalized output (drive-letter case, slash direction, trailing slash
  // all settled) so the duplicate check matches exactly what a save writes.
  const effectiveRoot = rootValidation.state === 'ok' ? rootValidation.normalized : trimmedRoot;

  // Has this folder already been mapped? Compare against every saved mapping
  // (case-insensitively, the app's notion of "same folder"), excluding the
  // one we're editing. A match means a duplicate — block the save and tell
  // the user to edit the existing mapping instead of stacking a second one.
  const duplicate = useMemo(() => {
    if (!effectiveRoot) return null;
    const key = effectiveRoot.toLowerCase();
    return (
      existingBindings.find(
        (b) => b.id !== initial?.id && b.workspace_root.toLowerCase() === key
      ) ?? null
    );
  }, [existingBindings, effectiveRoot, initial?.id]);

  // In edit mode, only enable Apply once something actually changed — there's
  // nothing to save otherwise. Create modes are always "dirty".
  const dirty = useMemo(() => {
    if (!isEdit || !initial) return true;
    return !sameBindingInput(
      { workspace_root: trimmedRoot, space_id: spaceId, feature_set_ids: fsIds },
      {
        workspace_root: initial.workspace_root,
        space_id: initial.space_id,
        feature_set_ids: initial.feature_set_ids,
      }
    );
  }, [isEdit, initial, trimmedRoot, spaceId, fsIds]);

  // Note: an empty feature-set selection is a VALID mapping ("no Space tools";
  // built-in servers still apply per Space), so it does not block Apply.
  const canSubmit =
    !submitting &&
    !!spaceId &&
    (rootValidation.state === 'ok' || !rootEditable) &&
    !duplicate &&
    dirty;

  const handleSubmit = async () => {
    if (!root.trim()) {
      onError(isId ? 'Enter a workspace identifier.' : 'Pick a project first.');
      return;
    }
    if (!isId && rootValidation.state === 'error') {
      onError(rootValidation.reason);
      return;
    }
    if (duplicate) {
      onError(
        isId
          ? 'That identifier is already mapped. Open its existing mapping to change it.'
          : `That project is already mapped. Open the existing mapping to change it.`
      );
      return;
    }
    if (!spaceId) {
      onError('Pick a Space.');
      return;
    }
    if (savedTimerRef.current) {
      clearTimeout(savedTimerRef.current);
      savedTimerRef.current = null;
    }
    setSubmitting(true);
    onSaveStatusChange?.({ kind: 'saving' });
    try {
      await onSubmit({
        workspace_root: root.trim(),
        space_id: spaceId,
        feature_set_ids: fsIds,
        binding_type: bindingType,
      });
      onSaveStatusChange?.({ kind: 'saved' });
      savedTimerRef.current = setTimeout(() => {
        onSaveStatusChange?.({ kind: 'idle' });
      }, 1800);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      onSaveStatusChange?.({ kind: 'error', message: msg });
      onError(msg);
    } finally {
      setSubmitting(false);
    }
  };

  // Saving is now explicit: nothing is written until the user presses
  // Apply (see `handleSubmit`). The old debounced autosave + flush-on-close
  // was removed — it fired writes while the user was still deciding and
  // needed extra reconciliation work to stay correct. Closing the panel now
  // simply discards unsaved edits.

  const submitLabel = isEdit
    ? 'Apply changes'
    : mode === 'create-from-live'
      ? 'Save mapping'
      : 'Create mapping';

  return (
    <div className="space-y-5">
      {/* Plain-language primer for anyone who's never seen McpMux. Explains
          the whole flow in two sentences before the fields. */}
      <div className="rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-3.5 py-3 text-xs leading-relaxed text-[rgb(var(--muted))]">
        <span className="font-semibold text-[rgb(var(--foreground))]">What is a mapping?</span>{' '}
        {isId
          ? 'Enter any string you choose, then choose the tools it gets. A client that sends this value in the X-Mcpmux-Workspace header receives exactly those tools.'
          : 'Pick a project, then choose the tools it should get. Whenever you open that project in a connected app — Cursor, VS Code, Claude — McpMux hands it exactly the tools you choose here, and nothing else.'}
      </div>

      <FormField label={isId ? 'Workspace identifier' : 'Project'}>
        <div className="flex gap-2">
          <input
            ref={rootRef}
            type="text"
            value={root}
            onChange={(e) => setRoot(e.target.value)}
            readOnly={!rootEditable}
            placeholder={
              isId ? 'Any string you choose' : 'Browse for a project, or paste an absolute path'
            }
            className={[
              'min-w-0 flex-1 rounded-lg px-3 py-2 font-mono text-sm focus:outline-none focus:ring-2',
              !rootEditable
                ? 'focus:ring-primary-500 cursor-not-allowed border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] text-[rgb(var(--muted))]'
                : rootValidation.state === 'error'
                  ? 'border border-red-500/60 bg-[rgb(var(--background))] focus:border-red-500 focus:ring-red-500'
                  : 'focus:ring-primary-500 focus:border-primary-500 border border-[rgb(var(--border))] bg-[rgb(var(--background))]',
            ].join(' ')}
            data-testid="workspace-binding-root-input"
          />
          {rootEditable && !isId && (
            <button
              type="button"
              onClick={async () => {
                // Native directory picker — honors each OS's conventions
                // (NSOpenPanel on macOS, IFileDialog on Windows, portal on
                // Linux). The selected path is absolute already, so we
                // just hand it off and let the live validator normalize.
                try {
                  const picked = await openDialog({
                    directory: true,
                    multiple: false,
                    title: 'Pick a project folder',
                  });
                  if (typeof picked === 'string' && picked.length > 0) {
                    setRoot(picked);
                  }
                } catch (e) {
                  onError(e instanceof Error ? e.message : String(e));
                }
              }}
              className="focus:ring-primary-500 inline-flex flex-shrink-0 items-center gap-1.5 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:bg-[rgb(var(--surface-hover))] focus:outline-none focus:ring-2"
              title="Pick a folder"
              data-testid="workspace-binding-browse"
            >
              <FolderSearch className="h-4 w-4" />
              <span className="hidden sm:inline">Browse</span>
            </button>
          )}
        </div>
        {duplicate ? (
          <p
            className="mt-1.5 flex items-start gap-1.5 text-[11px] text-red-600 dark:text-red-400"
            data-testid="workspace-binding-duplicate-error"
          >
            <AlertCircle className="mt-px h-3 w-3 flex-shrink-0" />
            <span>
              {isId
                ? 'That identifier is already mapped. Open its existing mapping to change what it sees instead of adding a second one.'
                : 'This project is already mapped. Open its existing mapping to change what it sees instead of adding a second one.'}
            </span>
          </p>
        ) : isId ? (
          <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
            {isManagedClient ? (
              <>
                Managed by the client <strong>{managedClientName}</strong> — this identifier is set
                when the client is registered and can&apos;t be changed here.
              </>
            ) : (
              <>
                Matched exactly (case-sensitive). A client sends this value in the{' '}
                <code className="font-mono">X-Mcpmux-Workspace</code> header.
              </>
            )}
          </p>
        ) : (
          <RootValidationHint state={rootValidation} editable={rootEditable} originalValue={root} />
        )}
      </FormField>

      <FormField
        label="Space"
        hint="A Space is a profile that groups MCP servers. Choose which one this folder draws its tools from."
      >
        <Picker
          value={spaceId}
          onChange={setSpaceId}
          placeholder="Pick a Space"
          options={spaces.map((s) => ({
            value: s.id,
            label: s.is_default ? `${s.name} · default` : s.name,
            icon: s.icon ?? undefined,
          }))}
          testId="workspace-binding-space"
        />
      </FormField>

      <FormField
        label={fsIds.length > 1 ? `Feature set (${fsIds.length} selected)` : 'Feature set'}
        hint="A feature set is a curated list of tools, prompts, and resources from that Space — exactly what this folder is allowed to use. Pick one, or combine several into a single set."
      >
        {!spaceId ? (
          <p className="px-3 py-2 text-xs italic text-[rgb(var(--muted))]">Pick a Space first.</p>
        ) : availableFs.length === 0 ? (
          <p className="px-3 py-2 text-xs italic text-[rgb(var(--muted))]">
            No feature sets in that Space yet.
          </p>
        ) : (
          <div
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))]"
            data-testid="workspace-binding-fs"
          >
            <div className="border-b border-[rgb(var(--border-subtle))] p-2">
              <input
                type="text"
                value={fsSearch}
                onChange={(e) => setFsSearch(e.target.value)}
                placeholder={`Search ${availableFs.length} feature set${availableFs.length === 1 ? '' : 's'}…`}
                className="focus:ring-primary-500 w-full rounded border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-2.5 py-1.5 text-xs focus:outline-none focus:ring-2"
                data-testid="workspace-binding-fs-search"
              />
            </div>
            <div className="max-h-56 space-y-1 overflow-y-auto p-1.5">
              {filteredFs.length === 0 ? (
                <p className="px-2 py-3 text-center text-xs italic text-[rgb(var(--muted))]">
                  No feature sets match &ldquo;{fsSearch}&rdquo;.
                </p>
              ) : (
                filteredFs.map((f) => {
                  const isSelected = fsIds.includes(f.id);
                  const order = isSelected ? fsIds.indexOf(f.id) + 1 : null;
                  return (
                    <button
                      key={f.id}
                      type="button"
                      onClick={() => toggleFs(f.id)}
                      className={[
                        'flex w-full items-center gap-2.5 rounded px-2.5 py-1.5 text-left text-sm transition-colors',
                        isSelected
                          ? 'bg-primary-500/10 hover:bg-primary-500/15'
                          : 'hover:bg-[rgb(var(--surface-hover))]',
                      ].join(' ')}
                      data-testid={`workspace-binding-fs-toggle-${f.id}`}
                    >
                      <div
                        className={[
                          'flex h-4 w-4 flex-shrink-0 items-center justify-center rounded border',
                          isSelected
                            ? 'bg-primary-500 border-primary-500'
                            : 'border-[rgb(var(--border-strong))] bg-[rgb(var(--surface))]',
                        ].join(' ')}
                      >
                        {isSelected ? (
                          <Check className="h-3 w-3 text-white" strokeWidth={3} />
                        ) : null}
                      </div>
                      {f.icon && (
                        <span className="flex-shrink-0 text-base leading-none">{f.icon}</span>
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <p className="truncate font-medium">{f.name}</p>
                          {isStarterFeatureSet(f) && (
                            <span
                              className="flex-shrink-0 rounded bg-[rgb(var(--surface))] px-1 py-0.5 text-[9px] uppercase tracking-wide text-[rgb(var(--muted))]"
                              title="Auto-seeded with this Space."
                            >
                              starter
                            </span>
                          )}
                        </div>
                        {f.description && (
                          <p className="truncate text-[11px] text-[rgb(var(--muted))]">
                            {f.description}
                          </p>
                        )}
                      </div>
                      {order !== null && fsIds.length > 1 && (
                        <span
                          className="text-primary-600 dark:text-primary-300 bg-primary-500/15 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full text-[10px] font-bold"
                          title="Render order — first FS rendered first; resolver merges all into one set."
                        >
                          {order}
                        </span>
                      )}
                    </button>
                  );
                })
              )}
            </div>
            {fsSearch && filteredFs.length > 0 && filteredFs.length < availableFs.length && (
              <div className="border-t border-[rgb(var(--border-subtle))] px-3 py-1.5 text-[11px] text-[rgb(var(--muted))]">
                {filteredFs.length} of {availableFs.length} shown
              </div>
            )}
          </div>
        )}
        {/* Escape hatch to the FeatureSets editor — a Space starts with only its
            auto-seeded Starter set, so this points the user at where new sets
            are made when Starter isn't what this folder should get. */}
        {spaceId && (
          <div className="mt-2">
            <CreateFeatureSetLink spaceId={spaceId} />
          </div>
        )}
      </FormField>

      {/* Saving is explicit in every mode now — nothing is written until
          Apply is pressed, so the user can keep deciding without half-saved
          state. In edit mode the button stays disabled until something
          actually changes. An empty feature-set selection is valid and
          savable. */}
      <div className="space-y-2 pt-1">
        {spaceId && fsIds.length === 0 && (
          // Empty is allowed — explain what it means rather than blocking.
          <p className="text-[11px] text-[rgb(var(--muted))]">
            No feature sets selected — this folder gets <strong>no tools</strong> from this Space.
            Built-in servers still apply per Space (see Built-in Servers).
          </p>
        )}
        {isEdit && dirty && !duplicate && (
          <p className="text-[11px] text-amber-600 dark:text-amber-400">
            Unsaved changes — press <strong>Apply changes</strong> to save.
          </p>
        )}
        <div className="flex items-center gap-2">
          <Button
            variant="primary"
            size="md"
            onClick={handleSubmit}
            disabled={!canSubmit}
            className="flex-1"
            data-testid="workspace-binding-submit"
          >
            {submitting ? (
              <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
            ) : (
              <Check className="mr-1.5 h-4 w-4" />
            )}
            {submitLabel}
          </Button>
          <Button variant="secondary" size="md" onClick={onCancel} disabled={submitting}>
            {isEdit ? 'Close' : 'Cancel'}
          </Button>
        </div>
      </div>
    </div>
  );
}

/**
 * Inline hint under the workspace_root input. Three visual states:
 *   • idle        — neutral hint about normalization rules
 *   • checking    — subtle spinner + "Checking…"
 *   • ok          — if the normalized form differs from the raw input,
 *                   show it as a preview so the user sees exactly what
 *                   gets saved (drive letter lowercased, URI scheme
 *                   stripped, slashes flipped, etc.). Otherwise silent.
 *   • error       — red message with the server's explanation
 */
function RootValidationHint({
  state,
  editable,
  originalValue,
}: {
  state:
    | { state: 'idle' }
    | { state: 'checking' }
    | { state: 'ok'; normalized: string }
    | { state: 'error'; reason: string };
  editable: boolean;
  originalValue: string;
}) {
  if (!editable) {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        This project was reported by the app that&apos;s open in it, so the path is fixed — just
        choose its tools below.
      </p>
    );
  }
  if (state.state === 'idle') {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        Click <strong>Browse</strong> to pick a project, or paste an absolute path. Accepts{' '}
        <code>/unix</code>, <code>C:\windows</code>, and <code>file://</code> forms.
      </p>
    );
  }
  if (state.state === 'checking') {
    return (
      <p className="mt-1.5 inline-flex items-center gap-1.5 text-[11px] text-[rgb(var(--muted))]">
        <Loader2 className="h-3 w-3 animate-spin" />
        Checking…
      </p>
    );
  }
  if (state.state === 'error') {
    return <p className="mt-1.5 text-[11px] text-red-600 dark:text-red-400">{state.reason}</p>;
  }
  // ok
  const changed = state.normalized !== originalValue.trim();
  if (!changed) {
    return <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">Ready to save.</p>;
  }
  return (
    <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
      Will be saved as{' '}
      <code className="font-mono text-[rgb(var(--foreground))]">{state.normalized}</code>.
    </p>
  );
}

function FormField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="mb-2 block text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
        {label}
      </label>
      {children}
      {hint && <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">{hint}</p>}
    </div>
  );
}

function Picker({
  value,
  onChange,
  options,
  placeholder,
  disabled,
  testId,
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string; icon?: string }>;
  placeholder: string;
  disabled?: boolean;
  testId?: string;
}) {
  return (
    <div className="relative">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        className="focus:ring-primary-500 focus:border-primary-500 w-full appearance-none rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 pr-9 text-sm focus:outline-none focus:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
        data-testid={testId}
      >
        <option value="">{placeholder}</option>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.icon ? `${o.icon}  ` : ''}
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[rgb(var(--muted))]" />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

function EmptyState({
  hasAny,
  hasFilter,
  onCreate,
}: {
  hasAny: boolean;
  hasFilter: boolean;
  onCreate: () => void;
}) {
  if (hasFilter && hasAny) {
    return (
      <Card className="mx-auto max-w-2xl">
        <CardContent className="flex flex-col items-center justify-center py-16">
          <Search className="mb-4 h-16 w-16 text-[rgb(var(--muted))]" />
          <h3 className="mb-2 text-lg font-medium">No workspaces match</h3>
          <p className="max-w-md text-center text-sm text-[rgb(var(--muted))]">
            Try adjusting the search or filter.
          </p>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card className="mx-auto max-w-2xl">
      <CardContent className="flex flex-col items-center justify-center py-16">
        <div className="bg-primary-50 dark:bg-primary-900/20 mb-4 flex h-16 w-16 items-center justify-center rounded-full">
          <Radio className="text-primary-500 h-8 w-8" />
        </div>
        <h3 className="mb-2 text-lg font-medium">No projects mapped yet</h3>
        <p className="mb-6 max-w-md text-center text-sm text-[rgb(var(--muted))]">
          When you open a project in a connected app, it shows up here so you can choose its tools.
          You can also map a project ahead of time — add one now to get started.
        </p>
        <Button variant="primary" onClick={onCreate}>
          <Plus className="mr-2 h-4 w-4" />
          Add a mapping
        </Button>
      </CardContent>
    </Card>
  );
}
