import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
  Folder,
  FolderOpen,
  FolderSearch,
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
import {
  Button,
  Card,
  CardContent,
  useToast,
  ToastContainer,
  useConfirm,
} from '@mcpmux/ui';
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
import {
  isStarterFeatureSet,
  listFeatureSets,
  type FeatureSet,
} from '@/lib/api/featureSets';
import { WorkspaceInstallPanel } from './WorkspaceInstallPanel';
import { WorkspaceSetupWizard } from './WorkspaceSetupWizard';
import { useSpaces, usePendingWorkspaceNew, useSetPendingWorkspaceNew } from '@/stores';
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
  const [bindings, setBindings] = useState<WorkspaceBinding[]>([]);
  const [reportedRoots, setReportedRoots] = useState<string[]>([]);
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
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
      const [b, fs, roots] = await Promise.all([
        listWorkspaceBindings(),
        listFeatureSets(),
        listReportedWorkspaceRoots().catch(() => [] as string[]),
      ]);
      setBindings(b);
      setFeatureSets(fs);
      setReportedRoots(roots);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    setIsLoading(true);
    void loadData().finally(() => setIsLoading(false));
  }, [loadData]);

  // Opened from the home "Set up a folder" CTA — launch the create walkthrough.
  useEffect(() => {
    if (pendingNew) {
      setSelected({ mode: 'new' });
      clearPendingNew(false);
    }
  }, [pendingNew, clearPendingNew]);

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
    return () => {
      unRoots.then((fn) => fn());
      unBinding.then((fn) => fn());
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
      const spaceName = e.binding ? spaceById.get(e.binding.space_id)?.name ?? '' : '';
      const fsNames = e.binding
        ? e.binding.feature_set_ids
            .map((id) => fsById.get(id)?.name ?? '')
            .join(' ')
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
    selected?.mode === 'entry' ? entries.find((e) => e.id === selected.id) ?? null : null;
  const selectedIsNew = selected?.mode === 'new';
  const panelOpen = selected !== null;

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
      message: `Apps opening "${binding.workspace_root}" will stop receiving these tools. You can map the folder again anytime.`,
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
      showError(
        'Could not clear unmapped folders',
        e instanceof Error ? e.message : String(e)
      );
    }
  };

  return (
    <div className="h-full flex flex-col relative" data-testid="workspaces-page">
      <header className="flex-shrink-0 p-8 border-b border-[rgb(var(--border-subtle))]">
        <div className="max-w-[2000px] mx-auto">
          <div className="flex items-start justify-between gap-6 mb-6">
            <div className="min-w-0 flex-1">
              <h1 className="text-3xl font-bold" data-testid="workspaces-title">
                Workspaces
              </h1>
              <p className="text-base text-[rgb(var(--muted))] mt-2 max-w-2xl">
                Map a folder to the tools it should get. When you open that
                folder in a connected app — Cursor, VS Code, Claude — McpMux
                serves exactly the tools you chose for it. Folders you
                haven&apos;t mapped fall back to your default Starter set, so
                they work out of the box — map one only when it should see
                something different.
              </p>
            </div>
            <div className="flex-shrink-0 flex items-center gap-2">
              <Button
                variant="ghost"
                size="md"
                onClick={refresh}
                disabled={isRefreshing}
                className="whitespace-nowrap"
              >
                <RefreshCw className={`h-4 w-4 mr-2 ${isRefreshing ? 'animate-spin' : ''}`} />
                Refresh
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={() => setSelected({ mode: 'new' })}
                data-testid="workspace-binding-create-toggle"
                className="whitespace-nowrap"
              >
                <Plus className="h-4 w-4 mr-2" />
                New mapping
              </Button>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3 max-w-3xl">
            <div className="relative flex-1 min-w-[220px]">
              <Search className="absolute left-4 top-1/2 -translate-y-1/2 h-5 w-5 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder="Search by path, space, or feature set…"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-12 pr-4 py-3 text-base bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-xl focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500 transition-all"
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
                <Trash2 className="h-4 w-4 mr-2" />
                Clear unmapped
              </Button>
            )}
          </div>
        </div>
      </header>

      {error && (
        <div className="flex-shrink-0 px-8 pt-6">
          <div className="max-w-[2000px] mx-auto p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-xl text-base text-red-600 dark:text-red-400">
            {error}
          </div>
        </div>
      )}

      <div className="flex-1 overflow-auto px-8 py-8">
        <div className="max-w-[2000px] mx-auto">
          {isLoading ? (
            <div className="flex items-center justify-center h-64">
              <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            </div>
          ) : filtered.length === 0 ? (
            <EmptyState
              hasAny={entries.length > 0}
              hasFilter={searchQuery.length > 0 || filter !== 'all'}
              onCreate={() => setSelected({ mode: 'new' })}
            />
          ) : (
            <div className="grid gap-5 auto-fill-cards">
              {filtered.map((entry) => {
                const isSelected =
                  selected?.mode === 'entry' && selected.id === entry.id;
                // Mapped entries show their bound Space + FeatureSet names.
                // Unmapped entries read "Not mapped" — they fall back to the
                // default Starter set rather than to an explicit binding.
                const resolvedSpaceName = entry.binding
                  ? spaceById.get(entry.binding.space_id)?.name
                  : undefined;
                const fsNames = entry.binding
                  ? entry.binding.feature_set_ids.map(
                      (id) => fsById.get(id)?.name ?? id
                    )
                  : [];
                return (
                  <EntryCard
                    key={entry.id}
                    entry={entry}
                    spaceName={resolvedSpaceName}
                    fsNames={fsNames}
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
            className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-40 animate-in fade-in duration-200"
            onClick={() => setSelected(null)}
          />
          {selectedIsNew ? (
            <WorkspaceSetupWizard
              spaces={spaces}
              featureSets={featureSets}
              reportedRoots={reportedRoots}
              existingBindings={bindings}
              onClose={() => setSelected(null)}
              onCreate={handleCreate}
              onError={(msg) => showError('Could not save', msg)}
            />
          ) : (
            <InspectorPanel
              key={selectedEntry?.id ?? 'entry'}
              entry={selectedEntry}
              isNew={false}
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
    <div className="inline-flex rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-0.5 gap-0.5">
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
              'inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg transition-all',
              active
                ? 'bg-[rgb(var(--background))] text-[rgb(var(--foreground))] shadow-sm'
                : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]',
            ].join(' ')}
          >
            {o.label}
            {typeof o.count === 'number' && (
              <span
                className={`inline-flex items-center justify-center min-w-[1.25rem] h-[1.125rem] px-1 text-[10px] font-semibold rounded-full ${
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
  selected,
  onClick,
}: {
  entry: Entry;
  spaceName: string | undefined;
  /** Resolved FeatureSet names for a mapped folder; empty when unmapped. */
  fsNames: string[];
  selected: boolean;
  onClick: () => void;
}) {
  const tone =
    entry.kind === 'unmapped-live'
      ? 'amber'
      : entry.kind === 'mapped-live'
        ? 'emerald'
        : 'neutral';
  const t = CARD_TONES[tone];
  const name = folderName(entry.root);

  return (
    <Card
      className={`relative cursor-pointer overflow-hidden transition-all hover:shadow-lg hover:scale-[1.01] ${
        selected ? 'ring-2 ring-primary-500 shadow-lg' : ''
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
              {entry.isLive ? (
                <FolderOpen className="h-6 w-6" />
              ) : (
                <Folder className="h-6 w-6" />
              )}
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
            </div>
            <h3 className="truncate text-base font-semibold" title={entry.root}>
              {name}
            </h3>
            <p
              className="truncate font-mono text-xs text-[rgb(var(--muted))]"
              title={entry.root}
            >
              {entry.root}
            </p>
          </div>
        </div>

        <div className="border-t border-[rgb(var(--border-subtle))] pt-4 text-xs">
          {entry.binding ? (
            <div className="flex items-center justify-between gap-3">
              <span className="inline-flex min-w-0 items-center gap-1.5">
                <Layers className="h-3.5 w-3.5 flex-shrink-0 text-primary-500" />
                <span
                  className="truncate font-medium text-[rgb(var(--foreground))]"
                  title={fsNames.join(', ')}
                >
                  {summarizeFeatureSets(fsNames)}
                </span>
                {fsNames.length > 1 && (
                  <span
                    className="flex-shrink-0 rounded-full bg-primary-500/10 px-1.5 text-[10px] font-bold tabular-nums text-primary-600 dark:text-primary-300"
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
      className={`inline-flex items-center px-1.5 py-0.5 rounded-md border text-[10px] font-semibold uppercase tracking-wider ${cls}`}
    >
      {children}
    </span>
  );
}

function Chip({
  children,
  tone,
}: {
  children: React.ReactNode;
  tone: 'primary' | 'neutral';
}) {
  const styles =
    tone === 'primary'
      ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300 border-primary-200 dark:border-primary-800/60'
      : 'bg-[rgb(var(--surface))] border-[rgb(var(--border-subtle))] text-[rgb(var(--foreground))]';
  return (
    <span
      className={`inline-flex items-center px-1.5 py-0.5 rounded-md border text-[11px] font-medium ${styles}`}
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
    iconQuiet:
      'bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400',
    iconActive: 'bg-primary-500 text-white shadow-sm shadow-primary-500/30',
    badgeOpen:
      'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300 border border-primary-300/70 dark:border-primary-700/70',
  },
  purple: {
    gradientOpen:
      'bg-gradient-to-r from-purple-50 to-pink-50 dark:from-purple-900/20 dark:to-pink-900/15',
    iconQuiet:
      'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400',
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
      className="bg-[rgb(var(--background))] rounded-xl border-2 border-[rgb(var(--border))] overflow-hidden transition-all"
      data-testid={testId}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={[
          'w-full flex items-center justify-between p-4 transition-all',
          open
            ? t.gradientOpen
            : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]',
        ].join(' ')}
        aria-expanded={open}
      >
        <div className="flex items-center gap-3 min-w-0 flex-1">
          <div
            className={[
              'p-2 rounded-lg flex-shrink-0 transition-colors duration-200',
              open ? t.iconActive : t.iconQuiet,
            ].join(' ')}
          >
            {icon}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="font-semibold text-base text-[rgb(var(--foreground))]">
                {title}
              </span>
              {typeof badge === 'number' && badge > 0 && (
                <span
                  className={[
                    'text-xs px-2 py-0.5 rounded-full font-bold tabular-nums',
                    open
                      ? t.badgeOpen
                      : 'bg-[rgb(var(--surface-dim))] text-[rgb(var(--muted))] border border-[rgb(var(--border-subtle))]',
                  ].join(' ')}
                >
                  {badge}
                </span>
              )}
              {headerExtra}
            </div>
            {subtitle && (
              <div className="text-xs text-[rgb(var(--muted))] truncate mt-0.5">
                {subtitle}
              </div>
            )}
          </div>
        </div>
        {open ? (
          <ChevronDown className="h-5 w-5 text-[rgb(var(--muted))] flex-shrink-0" />
        ) : (
          <ChevronRight className="h-5 w-5 text-[rgb(var(--muted))] flex-shrink-0" />
        )}
      </button>

      {open && (
        <div className="border-t-2 border-[rgb(var(--border))] bg-white dark:bg-[rgb(var(--background))] p-4">
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
  const mode: 'create' | 'edit' | 'create-from-live' = isNew
    ? 'create'
    : isMapped
      ? 'edit'
      : 'create-from-live';
  const title = isNew
    ? 'New mapping'
    : isMapped
      ? 'Workspace mapping'
      : 'Map this folder';
  const subtitle = isNew
    ? 'Choose the tools a folder should get.'
    : entry?.root ?? '';

  // Auto-save status drives the small pill in the Mapping section header.
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ kind: 'idle' });

  // Effective-features count drives the badge in the section header so the
  // user can see scale without expanding.
  const [effectiveTotal, setEffectiveTotal] = useState<number | null>(null);

  return (
    <div className="fixed right-0 top-0 bottom-0 w-full max-w-[480px] min-w-[420px] bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-50">
      <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3 flex-1 min-w-0">
            <div className="w-11 h-11 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg flex-shrink-0 border border-[rgb(var(--border-subtle))]">
              <FolderOpen className="h-5 w-5 text-[rgb(var(--muted))]" />
            </div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-0.5 flex-wrap">
                {!isNew && entry?.isLive && <Pill tone="emerald">Live</Pill>}
                {!isNew && entry && !isMapped && <Pill tone="amber">Unmapped</Pill>}
                {!isNew && entry && isMapped && !entry.isLive && <Pill tone="neutral">Offline</Pill>}
              </div>
              <h2 className="text-lg font-bold truncate">{title}</h2>
              <p
                className={`text-xs text-[rgb(var(--muted))] truncate ${!isNew ? 'font-mono' : ''}`}
                title={subtitle}
              >
                {subtitle}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
            aria-label="Close panel"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6 space-y-5">
        <CollapsibleSection
          icon={<FolderOpen className="h-5 w-5" />}
          tone="primary"
          title="Mapping"
          subtitle={
            mode === 'create'
              ? 'Choose the folder and the tools it should get.'
              : mode === 'create-from-live'
                ? 'This folder is open in an app and using your default Starter tools — map it to give it a specific set instead.'
                : isMapped && entry?.binding
                  ? `Gives ${
                      formatFsList(
                        entry.binding!.feature_set_ids.map(
                          (id) => featureSets.find((f) => f.id === id)?.name ?? id
                        )
                      ) || '—'
                    } from ${
                      spaces.find((s) => s.id === entry.binding!.space_id)?.name ?? '—'
                    }`
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
            title="Connect apps to this folder"
            subtitle="Write the McpMux config into this folder for the apps you use, with this folder's workspace header."
            defaultOpen={!isMapped}
            testId="workspace-install-section"
          >
            <WorkspaceInstallPanel workspaceRoot={entry.root} />
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
            <EffectiveFeaturesContent
              root={entry.root}
              onTotalChange={setEffectiveTotal}
            />
          </CollapsibleSection>
        )}
      </div>

      {entry?.binding && (
        <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void onDelete()}
            className="w-full text-red-600 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
            data-testid={`workspace-binding-delete-${entry.binding.id}`}
          >
            <Trash2 className="h-4 w-4 mr-2" />
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
        className={`${base} bg-[rgb(var(--surface-dim))] text-[rgb(var(--muted))] border-[rgb(var(--border))]`}
      >
        <Loader2 className="h-2.5 w-2.5 animate-spin" />
        Saving
      </span>
    );
  }
  if (status.kind === 'saved') {
    return (
      <span
        className={`${base} bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 border-green-300/70 dark:border-green-700/70 animate-in fade-in duration-200`}
      >
        <Check className="h-2.5 w-2.5" strokeWidth={2.5} />
        Saved
      </span>
    );
  }
  return (
    <span
      className={`${base} bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-400 border-red-200 dark:border-red-800`}
      title={status.message}
    >
      <AlertCircle className="h-2.5 w-2.5" />
      Error
    </span>
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
      const server_total = totals
        ? totals.tools + totals.prompts + totals.resources
        : 0;
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
    const unServer = listen('server-status', reload);
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
      <div className="p-3 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 flex items-start gap-2 text-sm text-red-600 dark:text-red-400">
        <AlertCircle className="h-4 w-4 flex-shrink-0 mt-0.5" />
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
      <div className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3 space-y-2.5">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-[10px] font-bold uppercase tracking-wider text-[rgb(var(--muted))]">
            Resolves to
          </span>
          <span className="text-sm font-semibold text-[rgb(var(--foreground))] truncate">
            {formatFsList(data.feature_sets.map((fs) => fs.name)) || '—'}
          </span>
          <span className="text-xs text-[rgb(var(--muted))]">in</span>
          <span className="text-sm font-medium text-[rgb(var(--foreground))]">
            {data.space_name}
          </span>
          <span
            title={
              data.source === 'binding'
                ? 'A workspace binding matched this folder — live sessions reporting it route here.'
                : 'No binding matches this folder, so it falls back to the default Starter set shown here. Map it to give this folder a different set.'
            }
            className={[
              'ml-auto text-[10px] px-2 py-0.5 rounded-full font-bold uppercase tracking-wider border',
              data.source === 'binding'
                ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 border-purple-300/70 dark:border-purple-700/70'
                : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 border-amber-300/70 dark:border-amber-700/70',
            ].join(' ')}
          >
            {data.source === 'binding' ? 'binding' : 'unbound'}
          </span>
        </div>

        {/* Availability progress bar. Stays quiet (green) when all servers
            are connected, leans amber when some are dim. */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs">
            <span className="text-[rgb(var(--muted))] tabular-nums">
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
          <div className="h-1.5 bg-gray-200 dark:bg-gray-800 rounded-full overflow-hidden">
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
        <div className="text-center py-8 text-[rgb(var(--muted))]">
          <Package className="h-8 w-8 mx-auto mb-2 opacity-50" />
          <p className="text-sm">No features configured in this feature set yet.</p>
        </div>
      ) : (
        <div className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] overflow-hidden">
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
  const prefix = group.server_alias.includes('.')
    ? group.server_alias.split('.', 2)[0]
    : null;
  const displayName = prefix
    ? group.server_alias.slice(prefix.length + 1)
    : group.server_alias;

  return (
    <div className="bg-[rgb(var(--surface))]">
      <div
        className="flex items-center justify-between px-4 py-3 hover:bg-[rgb(var(--surface-hover))] cursor-pointer transition-colors"
        onClick={onToggle}
        role="button"
        title={group.server_alias}
      >
        <div className="flex items-center gap-3 flex-1 min-w-0">
          {open ? (
            <ChevronDown className="h-4 w-4 text-[rgb(var(--muted))] flex-shrink-0" />
          ) : (
            <ChevronRight className="h-4 w-4 text-[rgb(var(--muted))] flex-shrink-0" />
          )}
          <ServerIcon className="h-4 w-4 text-blue-500 flex-shrink-0" />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1 flex-wrap">
              {prefix && (
                <span className="text-[10px] text-[rgb(var(--muted))] font-mono">
                  {prefix}.
                </span>
              )}
              <span className="font-medium text-sm truncate font-mono">
                {displayName}
              </span>
              <span
                className={[
                  'text-xs px-2 py-0.5 rounded-full font-bold flex-shrink-0 tabular-nums',
                  noneAvailable
                    ? 'bg-gray-100 dark:bg-gray-900/30 text-gray-600 dark:text-gray-400 border border-gray-300/70 dark:border-gray-700/70'
                    : allAvailable
                      ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 border border-green-300/70 dark:border-green-700/70'
                      : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 border border-amber-300/70 dark:border-amber-700/70',
                ].join(' ')}
              >
                {group.mapped}/{denominator}
              </span>
              {issue && (
                <span
                  className={[
                    'text-[10px] px-1.5 py-0.5 rounded-full font-bold uppercase tracking-wider border',
                    issue.tone === 'red'
                      ? 'bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-400 border-red-200 dark:border-red-800'
                      : issue.tone === 'amber'
                        ? 'bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 border-amber-200 dark:border-amber-800'
                        : 'bg-gray-50 dark:bg-gray-900/20 text-gray-600 dark:text-gray-400 border-gray-200 dark:border-gray-800',
                  ].join(' ')}
                >
                  {issue.label}
                </span>
              )}
            </div>
            {/* Per-server progress bar — same treatment as FeatureSetPanel's
                server rows so the visual language is consistent. */}
            <div className="h-1 bg-gray-200 dark:bg-gray-800 rounded-full overflow-hidden">
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
                  width:
                    group.mapped > 0
                      ? `${(availableCount / group.mapped) * 100}%`
                      : '0%',
                }}
              />
            </div>
          </div>
        </div>
      </div>

      {open && (
        <div className="bg-[rgb(var(--background))] border-t border-[rgb(var(--border))]">
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
            'flex items-start gap-3 px-4 py-2.5 pl-12 border-b border-[rgb(var(--border))] last:border-b-0',
            !item.available ? 'opacity-50' : '',
          ].join(' ')}
          title={item.description ?? item.feature_name}
        >
          {getFeatureTypeIcon(label)}
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="font-medium text-sm truncate font-mono">
                {item.display_name || item.feature_name}
              </span>
              <span
                className={[
                  'text-[10px] px-1.5 py-0.5 rounded font-medium',
                  getFeatureTypeColor(label),
                ].join(' ')}
              >
                {label}
              </span>
              {!item.available && (
                <span className="text-[9px] uppercase tracking-wider font-bold text-[rgb(var(--muted))]">
                  unavailable
                </span>
              )}
            </div>
            {item.description && (
              <p className="text-xs text-[rgb(var(--muted))] mt-0.5 line-clamp-1">
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
      return <Wrench className="h-4 w-4 text-purple-500 flex-shrink-0 mt-0.5" />;
    case 'prompt':
      return <MessageSquare className="h-4 w-4 text-blue-500 flex-shrink-0 mt-0.5" />;
    case 'resource':
      return <FileText className="h-4 w-4 text-green-500 flex-shrink-0 mt-0.5" />;
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

  const rootEditable = mode !== 'create-from-live';

  useEffect(() => {
    if (!rootEditable) {
      setRootValidation({ state: 'ok', normalized: root });
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
          setRootValidation(
            reason === ''
              ? { state: 'idle' }
              : { state: 'error', reason }
          );
        });
    }, 180);
    return () => clearTimeout(handle);
  }, [root, rootEditable]);

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
    setFsIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]
    );
  };

  const trimmedRoot = root.trim();
  // The canonical form the server will store. We prefer the validator's
  // normalized output (drive-letter case, slash direction, trailing slash
  // all settled) so the duplicate check matches exactly what a save writes.
  const effectiveRoot =
    rootValidation.state === 'ok' ? rootValidation.normalized : trimmedRoot;

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
      onError('Pick a folder first.');
      return;
    }
    if (rootValidation.state === 'error') {
      onError(rootValidation.reason);
      return;
    }
    if (duplicate) {
      onError(`That folder is already mapped. Open the existing mapping to change it.`);
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
        <span className="font-semibold text-[rgb(var(--foreground))]">
          What is a mapping?
        </span>{' '}
        Pick a folder, then choose the tools it should get. Whenever you open
        that folder in a connected app — Cursor, VS Code, Claude — McpMux hands
        it exactly the tools you choose here, and nothing else.
      </div>

      <FormField label="Workspace folder">
        <div className="flex gap-2">
          <input
            ref={rootRef}
            type="text"
            value={root}
            onChange={(e) => setRoot(e.target.value)}
            readOnly={!rootEditable}
            placeholder="Browse for a folder, or paste an absolute path"
            className={[
              'flex-1 min-w-0 px-3 py-2 rounded-lg text-sm font-mono focus:outline-none focus:ring-2',
              !rootEditable
                ? 'bg-[rgb(var(--background))] border border-[rgb(var(--border-subtle))] text-[rgb(var(--muted))] cursor-not-allowed focus:ring-primary-500'
                : rootValidation.state === 'error'
                  ? 'bg-[rgb(var(--background))] border border-red-500/60 focus:ring-red-500 focus:border-red-500'
                  : 'bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:ring-primary-500 focus:border-primary-500',
            ].join(' ')}
            data-testid="workspace-binding-root-input"
          />
          {rootEditable && (
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
                    title: 'Pick a workspace folder',
                  });
                  if (typeof picked === 'string' && picked.length > 0) {
                    setRoot(picked);
                  }
                } catch (e) {
                  onError(e instanceof Error ? e.message : String(e));
                }
              }}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] hover:bg-[rgb(var(--surface-hover))] text-sm font-medium text-[rgb(var(--foreground))] transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 flex-shrink-0"
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
            className="mt-1.5 text-[11px] text-red-600 dark:text-red-400 flex items-start gap-1.5"
            data-testid="workspace-binding-duplicate-error"
          >
            <AlertCircle className="h-3 w-3 flex-shrink-0 mt-px" />
            <span>
              This folder is already mapped. Open its existing mapping to change
              what it sees instead of adding a second one.
            </span>
          </p>
        ) : (
          <RootValidationHint
            state={rootValidation}
            editable={rootEditable}
            originalValue={root}
          />
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
        label={
          fsIds.length > 1
            ? `Feature set (${fsIds.length} selected)`
            : 'Feature set'
        }
        hint="A feature set is a curated list of tools, prompts, and resources from that Space — exactly what this folder is allowed to use. Pick one, or combine several into a single set."
      >
        {!spaceId ? (
          <p className="text-xs text-[rgb(var(--muted))] italic px-3 py-2">
            Pick a Space first.
          </p>
        ) : availableFs.length === 0 ? (
          <p className="text-xs text-[rgb(var(--muted))] italic px-3 py-2">
            No feature sets in that Space yet.
          </p>
        ) : (
          <div
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))]"
            data-testid="workspace-binding-fs"
          >
            <div className="p-2 border-b border-[rgb(var(--border-subtle))]">
              <input
                type="text"
                value={fsSearch}
                onChange={(e) => setFsSearch(e.target.value)}
                placeholder={`Search ${availableFs.length} feature set${availableFs.length === 1 ? '' : 's'}…`}
                className="w-full px-2.5 py-1.5 text-xs bg-[rgb(var(--surface))] border border-[rgb(var(--border-subtle))] rounded focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="workspace-binding-fs-search"
              />
            </div>
            <div className="max-h-56 overflow-y-auto p-1.5 space-y-1">
              {filteredFs.length === 0 ? (
                <p className="text-xs text-[rgb(var(--muted))] italic px-2 py-3 text-center">
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
                        'w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded text-left text-sm transition-colors',
                        isSelected
                          ? 'bg-primary-500/10 hover:bg-primary-500/15'
                          : 'hover:bg-[rgb(var(--surface-hover))]',
                      ].join(' ')}
                      data-testid={`workspace-binding-fs-toggle-${f.id}`}
                    >
                      <div
                        className={[
                          'h-4 w-4 rounded border flex items-center justify-center flex-shrink-0',
                          isSelected
                            ? 'bg-primary-500 border-primary-500'
                            : 'border-[rgb(var(--border-strong))] bg-[rgb(var(--surface))]',
                        ].join(' ')}
                      >
                        {isSelected ? (
                          <Check
                            className="h-3 w-3 text-white"
                            strokeWidth={3}
                          />
                        ) : null}
                      </div>
                      {f.icon && (
                        <span className="text-base leading-none flex-shrink-0">
                          {f.icon}
                        </span>
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-1.5">
                          <p className="font-medium truncate">{f.name}</p>
                          {isStarterFeatureSet(f) && (
                            <span
                              className="text-[9px] uppercase tracking-wide text-[rgb(var(--muted))] bg-[rgb(var(--surface))] px-1 py-0.5 rounded flex-shrink-0"
                              title="Auto-seeded with this Space."
                            >
                              starter
                            </span>
                          )}
                        </div>
                        {f.description && (
                          <p className="text-[11px] text-[rgb(var(--muted))] truncate">
                            {f.description}
                          </p>
                        )}
                      </div>
                      {order !== null && fsIds.length > 1 && (
                        <span
                          className="text-[10px] font-bold text-primary-600 dark:text-primary-300 bg-primary-500/15 rounded-full h-5 w-5 flex items-center justify-center flex-shrink-0"
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
              <div className="px-3 py-1.5 text-[11px] text-[rgb(var(--muted))] border-t border-[rgb(var(--border-subtle))]">
                {filteredFs.length} of {availableFs.length} shown
              </div>
            )}
          </div>
        )}
      </FormField>

      {/* Saving is explicit in every mode now — nothing is written until
          Apply is pressed, so the user can keep deciding without half-saved
          state. In edit mode the button stays disabled until something
          actually changes. An empty feature-set selection is valid and
          savable. */}
      <div className="pt-1 space-y-2">
        {spaceId && fsIds.length === 0 && (
          // Empty is allowed — explain what it means rather than blocking.
          <p className="text-[11px] text-[rgb(var(--muted))]">
            No feature sets selected — this folder gets <strong>no tools</strong>{' '}
            from this Space. Built-in servers still apply per Space (see Built-in
            Servers).
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
              <Loader2 className="h-4 w-4 animate-spin mr-1.5" />
            ) : (
              <Check className="h-4 w-4 mr-1.5" />
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
        This folder was reported by the app that&apos;s open in it, so the path
        is fixed — just choose its tools below.
      </p>
    );
  }
  if (state.state === 'idle') {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        Click <strong>Browse</strong> to pick a folder, or paste an absolute path. Accepts{' '}
        <code>/unix</code>, <code>C:\windows</code>, and <code>file://</code> forms.
      </p>
    );
  }
  if (state.state === 'checking') {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))] inline-flex items-center gap-1.5">
        <Loader2 className="h-3 w-3 animate-spin" />
        Checking…
      </p>
    );
  }
  if (state.state === 'error') {
    return (
      <p className="mt-1.5 text-[11px] text-red-600 dark:text-red-400">
        {state.reason}
      </p>
    );
  }
  // ok
  const changed = state.normalized !== originalValue.trim();
  if (!changed) {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        Ready to save.
      </p>
    );
  }
  return (
    <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
      Will be saved as{' '}
      <code className="font-mono text-[rgb(var(--foreground))]">
        {state.normalized}
      </code>
      .
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
      <label className="block text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))] mb-2">
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
        className="w-full appearance-none px-3 py-2 pr-9 bg-[rgb(var(--background))] border border-[rgb(var(--border))] rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500 disabled:opacity-50 disabled:cursor-not-allowed"
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
      <ChevronDown className="absolute right-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))] pointer-events-none" />
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
      <Card className="max-w-2xl mx-auto">
        <CardContent className="flex flex-col items-center justify-center py-16">
          <Search className="h-16 w-16 text-[rgb(var(--muted))] mb-4" />
          <h3 className="text-lg font-medium mb-2">No workspaces match</h3>
          <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md">
            Try adjusting the search or filter.
          </p>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card className="max-w-2xl mx-auto">
      <CardContent className="flex flex-col items-center justify-center py-16">
        <div className="h-16 w-16 rounded-full bg-primary-50 dark:bg-primary-900/20 flex items-center justify-center mb-4">
          <Radio className="h-8 w-8 text-primary-500" />
        </div>
        <h3 className="text-lg font-medium mb-2">No folders mapped yet</h3>
        <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md mb-6">
          When you open a folder in a connected app, it shows up here so you can
          choose its tools. You can also map a folder ahead of time — add one
          now to get started.
        </p>
        <Button variant="primary" onClick={onCreate}>
          <Plus className="h-4 w-4 mr-2" />
          Add a mapping
        </Button>
      </CardContent>
    </Card>
  );
}
