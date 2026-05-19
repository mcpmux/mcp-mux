import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
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
  ToggleLeft,
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
  clearSessionOverrides,
  listSessionOverrides,
  overridesForWorkspace,
  type SessionOverride,
} from '@/lib/api/sessionOverrides';
import {
  isStarterFeatureSet,
  listFeatureSets,
  type FeatureSet,
} from '@/lib/api/featureSets';
import { useSpaces } from '@/stores';
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
  const [filter, setFilter] = useState<'all' | 'live' | 'unmapped'>('all');

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
   * The system's routing fallback: the `is_default` Space plus that Space's
   * Default FeatureSet. Sessions whose reported root has no binding resolve
   * here. We compute it once and pass it down so EntryCard can show the
   * effective FS on every row, including unmapped ones.
   */
  const fallback = useMemo(() => {
    const space = spaces.find((s) => s.is_default) ?? spaces[0] ?? null;
    if (!space) return null;
    const fs =
      featureSets.find(
        (f) => f.space_id === space.id && isStarterFeatureSet(f)
      ) ?? null;
    return { space, fs };
  }, [spaces, featureSets]);

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
    let unmapped = 0;
    for (const e of entries) {
      if (e.isLive) live++;
      if (e.kind === 'unmapped-live') unmapped++;
    }
    return { all: entries.length, live, unmapped };
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
    success('Binding saved', created.workspace_root);
    return created;
  };

  const handleUpdate = async (id: string, input: WorkspaceBindingInput) => {
    const updated = await updateWorkspaceBinding(id, input);
    setBindings((prev) =>
      prev
        .map((b) => (b.id === id ? updated : b))
        .sort((a, b) => a.workspace_root.localeCompare(b.workspace_root))
    );
    success('Binding updated', updated.workspace_root);
  };

  const handleDelete = async (binding: WorkspaceBinding) => {
    const ok = await confirm({
      title: 'Remove binding',
      message: `Sessions matching "${binding.workspace_root}" will fall back to the default Space. You can recreate the binding anytime.`,
      confirmLabel: 'Remove',
      variant: 'danger',
    });
    if (!ok) return;
    try {
      await deleteWorkspaceBinding(binding.id);
      setBindings((prev) => prev.filter((b) => b.id !== binding.id));
      setSelected(null);
      success('Binding removed', binding.workspace_root);
    } catch (e) {
      showError('Failed to remove binding', e instanceof Error ? e.message : String(e));
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
                Each binding tells mcpmux which Space and feature set a folder routes into.
                Folders without a binding fall back to the default Space.
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
                New binding
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
                { value: 'unmapped', label: 'Unmapped', count: counts.unmapped },
              ]}
            />
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
                // For mapped entries: trust the binding. For unmapped: fall
                // back to the system's default Space + its Default FS so
                // every card answers "what tools does this folder see?".
                const resolvedSpaceName = entry.binding
                  ? spaceById.get(entry.binding.space_id)?.name
                  : fallback?.space.name;
                const resolvedFsName = entry.binding
                  ? formatFsList(
                      entry.binding.feature_set_ids.map(
                        (id) => fsById.get(id)?.name ?? id
                      )
                    )
                  : fallback?.fs?.name;
                return (
                  <EntryCard
                    key={entry.id}
                    entry={entry}
                    spaceName={resolvedSpaceName}
                    fsName={resolvedFsName}
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
          <InspectorPanel
            key={selectedIsNew ? 'new' : selectedEntry?.id ?? 'new'}
            entry={selectedEntry}
            isNew={selectedIsNew}
            spaces={spaces}
            featureSets={featureSets}
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
 * Structural equality between two binding inputs. The autosave effect
 * uses this to skip writes when the user re-toggled their way back to
 * the last-saved state — avoids spamming `WorkspaceBindingChanged` for
 * a no-op edit. `feature_set_ids` order matters (it's the operator-
 * chosen render order, not just a set), so we compare positionally.
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
// Entry card — matches Clients page card anatomy (56×56 icon, 3xl size, chips)
// ---------------------------------------------------------------------------

function EntryCard({
  entry,
  spaceName,
  fsName,
  selected,
  onClick,
}: {
  entry: Entry;
  spaceName: string | undefined;
  fsName: string | undefined;
  selected: boolean;
  onClick: () => void;
}) {
  const tone =
    entry.kind === 'unmapped-live'
      ? 'amber'
      : entry.kind === 'mapped-live'
        ? 'emerald'
        : 'neutral';

  return (
    <Card
      className={`cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01] ${
        selected ? 'ring-2 ring-primary-500 shadow-lg' : ''
      }`}
      onClick={onClick}
      data-testid={`workspace-entry-${entry.id}`}
    >
      <CardContent className="p-6">
        <div className="flex items-start gap-4 mb-4">
          <div className="relative flex-shrink-0">
            <div
              className={[
                'w-14 h-14 flex items-center justify-center rounded-xl border',
                tone === 'amber'
                  ? 'bg-amber-50 dark:bg-amber-900/20 border-amber-200/80 dark:border-amber-800/50 text-amber-600 dark:text-amber-400'
                  : tone === 'emerald'
                    ? 'bg-emerald-50 dark:bg-emerald-900/20 border-emerald-200/80 dark:border-emerald-800/50 text-emerald-600 dark:text-emerald-400'
                    : 'bg-[rgb(var(--surface))] border-[rgb(var(--border-subtle))] text-[rgb(var(--muted))]',
              ].join(' ')}
            >
              <FolderOpen className="h-6 w-6" />
            </div>
            {entry.isLive && (
              <span
                className="absolute -top-0.5 -right-0.5 h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-[rgb(var(--background))]"
                title="A client is currently active in this folder"
              />
            )}
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1.5 flex-wrap">
              {entry.kind === 'unmapped-live' && <Pill tone="amber">Unmapped</Pill>}
              {entry.kind === 'mapped-offline' && <Pill tone="neutral">Offline</Pill>}
              {entry.kind === 'mapped-live' && <Pill tone="emerald">Live</Pill>}
            </div>
            <p
              className="font-mono text-sm text-[rgb(var(--foreground))] truncate"
              title={entry.root}
            >
              {entry.root}
            </p>
          </div>
        </div>

        <div className="pt-4 border-t border-[rgb(var(--border-subtle))] text-xs text-[rgb(var(--muted))]">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span>Routes to</span>
            <Chip tone="primary">{fsName ?? '—'}</Chip>
            <span>in</span>
            <Chip tone="neutral">{spaceName ?? '—'}</Chip>
            {!entry.binding && (
              <span
                className="ml-1 inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium uppercase tracking-wider bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 border border-amber-200/70 dark:border-amber-800/60"
                title="No binding matches this folder yet — a live session would be denied. Click to bind it to a FeatureSet."
              >
                unbound
              </span>
            )}
          </div>
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
  onClose,
  onSubmit,
  onDelete,
  onError,
}: {
  entry: Entry | null;
  isNew: boolean;
  spaces: Space[];
  featureSets: FeatureSet[];
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
  const title = isNew ? 'New binding' : isMapped ? 'Binding' : 'Configure workspace';
  const subtitle = isNew
    ? 'Tell mcpmux how a folder should route.'
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
              ? 'Pick the FeatureSet this folder routes through.'
              : mode === 'create-from-live'
                ? 'Configure routing for this live workspace.'
                : isMapped && entry?.binding
                  ? `Routes to ${
                      formatFsList(
                        entry.binding!.feature_set_ids.map(
                          (id) => featureSets.find((f) => f.id === id)?.name ?? id
                        )
                      ) || '—'
                    } in ${
                      spaces.find((s) => s.id === entry.binding!.space_id)?.name ?? '—'
                    }`
                  : 'Changes save automatically.'
          }
          defaultOpen={isNew || !isMapped}
          headerExtra={mode === 'edit' ? <SaveStatusPill status={saveStatus} /> : null}
          testId="workspace-mapping-section"
        >
          <BindingForm
            mode={mode}
            spaces={spaces}
            featureSets={featureSets}
            initial={entry?.binding ?? null}
            prefillRoot={entry && !isMapped ? entry.root : undefined}
            onCancel={onClose}
            onSubmit={onSubmit}
            onError={onError}
            onSaveStatusChange={setSaveStatus}
          />
        </CollapsibleSection>

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

        {entry && !isNew && entry.isLive && (
          <CollapsibleSection
            icon={<ToggleLeft className="h-5 w-5" />}
            tone="primary"
            title="Active session overrides"
            subtitle="Servers an LLM enabled or disabled for live sessions on this folder"
            defaultOpen={true}
            testId="workspace-session-overrides-section"
          >
            <SessionOverridesContent workspaceRoot={entry.root} />
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
            Remove binding
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
// Session overrides — per-session enable/disable from meta tools
// ---------------------------------------------------------------------------

/**
 * Lists live sessions reporting this workspace root and any session-scoped
 * server overrides applied via `mcpmux_enable_server` / `mcpmux_disable_server`.
 */
function SessionOverridesContent({ workspaceRoot }: { workspaceRoot: string }) {
  const [entries, setEntries] = useState<SessionOverride[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [clearingId, setClearingId] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const all = await listSessionOverrides();
      setEntries(overridesForWorkspace(all, workspaceRoot));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setEntries([]);
    } finally {
      setIsLoading(false);
    }
  }, [workspaceRoot]);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    const unMeta = listen<{ tool_name?: string }>('meta-tool-invoked', (ev) => {
      const name = ev.payload?.tool_name ?? '';
      if (name === 'mcpmux_enable_server' || name === 'mcpmux_disable_server') {
        void reload();
      }
    });
    const unOverrides = listen('session-overrides-changed', () => {
      void reload();
    });
    return () => {
      void unMeta.then((fn) => fn());
      void unOverrides.then((fn) => fn());
    };
  }, [reload]);

  const handleClear = async (sessionId: string) => {
    setClearingId(sessionId);
    try {
      await clearSessionOverrides(sessionId);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setClearingId(null);
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 text-sm text-[rgb(var(--muted))] py-2">
        <Loader2 className="h-4 w-4 animate-spin" />
        Loading session overrides…
      </div>
    );
  }

  if (error) {
    return (
      <p className="text-sm text-red-600 dark:text-red-400" role="alert">
        {error}
      </p>
    );
  }

  if (entries.length === 0) {
    return (
      <p className="text-sm text-[rgb(var(--muted))]">
        No live sessions on this folder have session-scoped server overrides.
      </p>
    );
  }

  return (
    <div className="space-y-3" data-testid="workspace-session-overrides-list">
      {entries.map((entry) => {
        const shortId =
          entry.session_id.length > 12
            ? `${entry.session_id.slice(0, 8)}…${entry.session_id.slice(-4)}`
            : entry.session_id;
        const hasOverrides = entry.enabled.length > 0 || entry.disabled.length > 0;
        return (
          <div
            key={entry.session_id}
            className="rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] p-3 space-y-2"
            data-testid={`session-override-${entry.session_id}`}
          >
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <p
                  className="text-xs font-mono text-[rgb(var(--foreground))] truncate"
                  title={entry.session_id}
                >
                  {shortId}
                </p>
                {entry.roots.length > 0 && (
                  <p className="text-[10px] text-[rgb(var(--muted))] font-mono truncate mt-0.5">
                    {entry.roots.join(', ')}
                  </p>
                )}
              </div>
              {hasOverrides && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="flex-shrink-0 text-xs h-7"
                  disabled={clearingId === entry.session_id}
                  onClick={() => void handleClear(entry.session_id)}
                  data-testid={`session-override-clear-${entry.session_id}`}
                >
                  {clearingId === entry.session_id ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    'Clear all'
                  )}
                </Button>
              )}
            </div>
            {entry.enabled.length > 0 && (
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-wider text-green-700 dark:text-green-400 mb-1">
                  Enabled (session)
                </p>
                <div className="flex flex-wrap gap-1">
                  {entry.enabled.map((id) => (
                    <span
                      key={`en-${id}`}
                      className="inline-flex px-1.5 py-0.5 rounded text-[10px] font-mono bg-green-100 dark:bg-green-900/30 text-green-800 dark:text-green-200"
                    >
                      {id}
                    </span>
                  ))}
                </div>
              </div>
            )}
            {entry.disabled.length > 0 && (
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-wider text-amber-700 dark:text-amber-400 mb-1">
                  Disabled (session)
                </p>
                <div className="flex flex-wrap gap-1">
                  {entry.disabled.map((id) => (
                    <span
                      key={`dis-${id}`}
                      className="inline-flex px-1.5 py-0.5 rounded text-[10px] font-mono bg-amber-100 dark:bg-amber-900/30 text-amber-800 dark:text-amber-200"
                    >
                      {id}
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>
        );
      })}
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
                : 'No binding matches this folder. A live roots-capable session would be denied; the FeatureSet shown is a preview of what binding here would expose.'
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
  // FS list. Reseed an empty selection with the default FS so the operator
  // doesn't have to click anything for a "single-FS, default" binding.
  useEffect(() => {
    if (availableFs.length === 0) {
      if (fsIds.length > 0) setFsIds([]);
      return;
    }
    const validIds = new Set(availableFs.map((f) => f.id));
    const filtered = fsIds.filter((id) => validIds.has(id));
    if (filtered.length === 0) {
      const fallback = availableFs.find(isStarterFeatureSet) ?? availableFs[0];
      setFsIds([fallback.id]);
    } else if (filtered.length !== fsIds.length) {
      setFsIds(filtered);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [availableFs]);

  const toggleFs = (id: string) => {
    setFsIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]
    );
  };

  const canSubmit =
    !submitting &&
    !!spaceId &&
    fsIds.length > 0 &&
    (rootValidation.state === 'ok' || !rootEditable);

  const handleSubmit = async () => {
    if (!root.trim()) {
      onError('Workspace root is required.');
      return;
    }
    if (rootValidation.state === 'error') {
      onError(rootValidation.reason);
      return;
    }
    if (!spaceId) {
      onError('Pick a Space.');
      return;
    }
    if (fsIds.length === 0) {
      onError('Pick at least one feature set.');
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit({
        workspace_root: root.trim(),
        space_id: spaceId,
        feature_set_ids: fsIds,
      });
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  // ---------- Autosave (edit mode) -----------------------------------------
  //
  // Debounced (1500 ms) so a burst of FS-toggle clicks coalesces into one
  // save instead of firing N WorkspaceBindingChanged events back-to-back.
  // Dedupe is against the **last successfully-saved** payload, not just
  // `initial` — so re-toggling A → B → A is a no-op (back to last saved),
  // and once a save lands the next idle window doesn't re-save the same
  // values.
  //
  // Critical: the debounce timer is cleared on dependency change but the
  // **pending payload survives panel close**. If the user edits then
  // closes before the debounce fires, the unmount handler flushes the
  // save synchronously to Tauri — the IPC goes out before React tears
  // the component down, and the save completes in the background.
  const saveSeqRef = useRef(0);
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Snapshot of the last payload we successfully wrote. `null` means
  // "never saved during this panel session" — fall back to `initial` for
  // dedupe in that case.
  const lastSavedRef = useRef<WorkspaceBindingInput | null>(null);
  // The most recent payload the user produced that has NOT yet been
  // committed. Cleared on successful save. The unmount handler reads
  // this to decide whether to flush.
  const pendingPayloadRef = useRef<WorkspaceBindingInput | null>(null);
  // Latest closures via ref so the unmount-only effect's empty-deps
  // cleanup can still call the freshest handlers — closing the panel
  // mid-edit must use the parent's *current* `onSubmit`, not whatever it
  // captured on first mount.
  const onSubmitRef = useRef(onSubmit);
  const onSaveStatusChangeRef = useRef(onSaveStatusChange);
  useEffect(() => {
    onSubmitRef.current = onSubmit;
    onSaveStatusChangeRef.current = onSaveStatusChange;
  }, [onSubmit, onSaveStatusChange]);

  useEffect(() => {
    if (!isEdit || !initial) return;
    if (!canSubmit) return;

    const candidate: WorkspaceBindingInput = {
      workspace_root: root.trim(),
      space_id: spaceId,
      feature_set_ids: fsIds,
    };

    // Dedupe baseline: last-saved if we've saved during this session,
    // otherwise the initial payload from when the panel opened.
    const baseline = lastSavedRef.current ?? {
      workspace_root: initial.workspace_root,
      space_id: initial.space_id,
      feature_set_ids: initial.feature_set_ids,
    };
    if (sameBindingInput(candidate, baseline)) {
      pendingPayloadRef.current = null;
      return;
    }

    pendingPayloadRef.current = candidate;
    const seq = ++saveSeqRef.current;
    onSaveStatusChange?.({ kind: 'idle' });
    const handle = setTimeout(async () => {
      if (saveSeqRef.current !== seq) return;
      onSaveStatusChange?.({ kind: 'saving' });
      setSubmitting(true);
      try {
        await onSubmit(candidate);
        if (saveSeqRef.current !== seq) return;
        lastSavedRef.current = candidate;
        pendingPayloadRef.current = null;
        onSaveStatusChange?.({ kind: 'saved' });
        if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
        savedTimerRef.current = setTimeout(() => {
          onSaveStatusChange?.({ kind: 'idle' });
        }, 1800);
      } catch (e) {
        if (saveSeqRef.current !== seq) return;
        const msg = e instanceof Error ? e.message : String(e);
        onSaveStatusChange?.({ kind: 'error', message: msg });
        onError(msg);
      } finally {
        setSubmitting(false);
      }
    }, 1500);
    return () => clearTimeout(handle);
  }, [
    isEdit,
    initial,
    root,
    spaceId,
    fsIds,
    canSubmit,
    onSubmit,
    onError,
    onSaveStatusChange,
  ]);

  // Unmount-only flush. If a save was scheduled but the timer hasn't
  // fired by the time the user closes the panel, fire it now so their
  // edits aren't silently dropped. Empty-deps so this only runs on
  // unmount, not on every dep change of the autosave effect above.
  useEffect(() => {
    return () => {
      const pending = pendingPayloadRef.current;
      if (!pending) return;
      // Fire-and-forget. Tauri's `invoke` posts the IPC message to the
      // Rust side immediately; the React tree can unmount in parallel
      // and the save still completes. Bump the seq so any in-flight
      // debounced save from before the close is discarded if it lands.
      saveSeqRef.current += 1;
      onSaveStatusChangeRef.current?.({ kind: 'saving' });
      onSubmitRef
        .current(pending)
        .then(() => {
          onSaveStatusChangeRef.current?.({ kind: 'saved' });
        })
        .catch((e) => {
          // Parent's toast bridge is gone with the panel — fall back to
          // the console so the failure isn't silent in dev.
          console.warn(
            '[workspace-binding] flush-on-close save failed:',
            e instanceof Error ? e.message : String(e)
          );
        });
    };
  }, []);

  const submitLabel =
    mode === 'create-from-live' ? 'Save binding' : 'Create binding';

  return (
    <div className="space-y-5">
      <FormField label="Workspace root">
        <div className="flex gap-2">
          <input
            ref={rootRef}
            type="text"
            value={root}
            onChange={(e) => setRoot(e.target.value)}
            readOnly={!rootEditable}
            placeholder="Pick a folder, or paste an absolute path"
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
        <RootValidationHint state={rootValidation} editable={rootEditable} originalValue={root} />
      </FormField>

      <FormField label="Space" hint="Which Space this folder belongs to.">
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
            ? `Feature sets (${fsIds.length} selected)`
            : 'Feature set'
        }
        hint="Which tools this folder sees. Pick one or compose several — selected sets union into a single allow list."
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

      {!isEdit && (
        <div className="flex items-center gap-2 pt-1">
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
            Cancel
          </Button>
        </div>
      )}
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
        Reported by the connected client — the path isn&apos;t editable.
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
        <h3 className="text-lg font-medium mb-2">Nothing to show yet</h3>
        <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md mb-6">
          When a connected MCP client reports a workspace root, it will appear here live.
          You can also add a binding ahead of time for a folder you care about.
        </p>
        <Button variant="primary" onClick={onCreate}>
          <Plus className="h-4 w-4 mr-2" />
          Add a binding
        </Button>
      </CardContent>
    </Card>
  );
}
