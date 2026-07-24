import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import {
  useServerStatusEvents,
  useWorkspaceEventListener,
  type WorkspaceEventChannel,
} from '@/hooks';
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
  FolderOpen,
  Loader2,
  MessageSquare,
  Package,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server as ServerGlyph,
  Trash2,
  Wrench,
  X,
  Monitor,
  UserRound,
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
  forgetReportedRoot,
  getWorkspaceEffectiveFeatures,
  isWorkspaceBindingPromptDismissed,
  listReportedWorkspaceRoots,
  listWorkspaceBindings,
  type EffectiveFeature,
  type WorkspaceBinding,
  type WorkspaceEffectiveFeatures,
  isIdBinding,
} from '@/lib/api/workspaceBindings';
import {
  createMachine,
  getHostname,
  getLocalMachineId,
  listMachines,
  setLocalMachineId as persistLocalMachineId,
  type Machine,
} from '@/lib/api/machines';
import {
  getMissingMachineProfileField,
  isMachineProfileComplete,
  toMachineProfilePayload,
} from '@/lib/machine-profile.helpers';
import { listWorkspaceAppearances, type WorkspaceAppearance } from '@/lib/api/workspaceAppearances';
import {
  listFeatureSets,
  type FeatureSet,
} from '@/lib/api/featureSets';
import { ServerIcon } from '@/components/ServerIcon';
import {
  useBindingPanelStore,
  usePendingWorkspaceNew,
  useSetPendingWorkspaceNew,
  useSpaces,
} from '@/stores';
import type { Space } from '@/lib/api/spaces';
import { FormField } from './workspace-binding-form.component';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';
import { useViewerIdentity } from '@/hooks/use-viewer-identity.hook';

/**
 * Workspaces page.
 *
 * Mirrors the Clients page's shape for visual consistency:
 *   • Header: title + subtitle + refresh, followed by a single large search.
 *   • Content: responsive cards grid inside a max-w-[2000px] wrapper.
 *   • Binding edits: card clicks open the global WorkspaceBindingPanel via
 *     bindingPanelStore (mounted in App.tsx).
 *
 * Each card is a workspace entry, unioning bindings and live reported roots
 * (dedup'd by normalized path). Status is conveyed with a corner dot + pill:
 *   • LIVE + unmapped              → amber UNMAPPED
 *   • LIVE + bound (other machine) → violet BOUND ELSEWHERE
 *   • LIVE + bound (this machine)  → emerald LIVE
 *   • OFFLINE + mapped             → neutral
 */

type EntryKind = 'unmapped-live' | 'live-elsewhere' | 'mapped-live' | 'mapped-offline';
interface Entry {
  id: string;
  kind: EntryKind;
  root: string;
  bindings: WorkspaceBinding[];
  isLive: boolean;
  /** Id-type bindings route by OAuth/API client id, not folder path. */
  isClientMapping?: boolean;
}

/**
 * Canonical binding for an entry — global (`machine_id IS NULL`) first,
 * else first machine-scoped binding.
 */
function primaryBinding(entry: Entry): WorkspaceBinding | null {
  return (
    entry.bindings.find((b) => b.machine_id == null) ?? entry.bindings[0] ?? null
  );
}

/**
 * True when at least one binding applies on this install: global or scoped to
 * the local machine.
 */
function entryIsBoundForCurrentMachine(
  entry: Entry,
  localMachineId: string | null,
): boolean {
  return entry.bindings.some(
    (b) => b.machine_id == null || b.machine_id === localMachineId,
  );
}

const WORKSPACE_TABLE_REFRESH_CHANNELS: WorkspaceEventChannel[] = [
  'session-roots-changed',
  'workspace-binding-changed',
];

const EFFECTIVE_FEATURES_REFRESH_CHANNELS: WorkspaceEventChannel[] = ['workspace-binding-changed'];

export function WorkspacesPage() {
  const { t } = useTranslation(['workspaces', 'common']);
  const spaces = useSpaces();
  const pendingNew = usePendingWorkspaceNew();
  const clearPendingNew = useSetPendingWorkspaceNew();
  const { machineId: viewerMachineId } = useViewerIdentity();
  const [bindings, setBindings] = useState<WorkspaceBinding[]>([]);
  const [appearances, setAppearances] = useState<WorkspaceAppearance[]>([]);
  const [reportedRoots, setReportedRoots] = useState<string[]>([]);
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [localMachineId, setLocalMachineId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { toasts, success, error: showError, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();
  const openBindingPanel = useBindingPanelStore((state) => state.open);
  const isPanelOpen = useBindingPanelStore((state) => state.isOpen);

  const [searchQuery, setSearchQuery] = useState('');
  const [filter, setFilter] = useState<'all' | 'live' | 'mapped' | 'unmapped'>('all');
  const [machineFilter, setMachineFilter] = useState<string>('all');
  const [identityBannerDismissed, setIdentityBannerDismissed] = useState(false);
  const [showIdentityModal, setShowIdentityModal] = useState(false);

  const loadData = useCallback(async () => {
    setError(null);
    try {
      const [b, fs, roots, ap, machineList, localId] = await Promise.all([
        listWorkspaceBindings(),
        listFeatureSets(),
        listReportedWorkspaceRoots().catch(() => [] as string[]),
        listWorkspaceAppearances().catch(() => [] as WorkspaceAppearance[]),
        listMachines().catch(() => [] as Machine[]),
        getLocalMachineId().catch(() => null),
      ]);
      setBindings(b);
      setFeatureSets(fs);
      setReportedRoots(roots);
      setAppearances(ap);
      setMachines(machineList);
      setLocalMachineId(localId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    setIsLoading(true);
    void loadData().finally(() => setIsLoading(false));
  }, [loadData]);

  // Opened from the home "Set up a folder" CTA — open global binding panel.
  useEffect(() => {
    if (pendingNew) {
      openBindingPanel({ mode: 'create' });
      clearPendingNew(false);
    }
  }, [pendingNew, clearPendingNew, openBindingPanel]);

  // Refresh whenever something the table reflects changes outside the page:
  //   • `session-roots-changed` — a connected client newly reported a root.
  //   • `workspace-binding-changed` — binding or appearance write (same channel).
  // Without the binding listener, popup-driven saves leave this page showing
  // the stale "UNMAPPED" badge until the user navigates away and back.
  useWorkspaceEventListener(
    useCallback(() => {
      void loadData();
    }, [loadData]),
    WORKSPACE_TABLE_REFRESH_CHANNELS
  );

  const refresh = async () => {
    setIsRefreshing(true);
    try {
      await loadData();
    } finally {
      setIsRefreshing(false);
    }
  };

  const bindingsByRoot = useMemo(() => {
    const m = new Map<string, WorkspaceBinding[]>();
    for (const b of bindings) {
      if (isIdBinding(b)) continue;
      const key = b.workspace_root.toLowerCase();
      const list = m.get(key) ?? [];
      list.push(b);
      m.set(key, list);
    }
    return m;
  }, [bindings]);
  const appearancesByRoot = useMemo(() => {
    const m = new Map<string, string>();
    for (const appearance of appearances) {
      m.set(appearance.workspace_root.toLowerCase(), appearance.icon);
    }
    return m;
  }, [appearances]);
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
  const machinesById = useMemo(() => {
    const m = new Map<string, Machine>();
    for (const machine of machines) m.set(machine.id, machine);
    return m;
  }, [machines]);

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
      const binds = bindingsByRoot.get(key) ?? [];
      const primary = primaryBinding({
        id: '',
        kind: 'unmapped-live',
        root,
        bindings: binds,
        isLive: true,
      });
      const entry: Entry = {
        id: primary?.id ?? `live:${root}`,
        kind: 'unmapped-live',
        root,
        bindings: binds,
        isLive: true,
      };
      if (binds.length > 0 && entryIsBoundForCurrentMachine(entry, viewerMachineId ?? localMachineId)) {
        entry.kind = 'mapped-live';
      } else if (binds.length > 0) {
        entry.kind = 'live-elsewhere';
      }
      list.push(entry);
    }
    for (const b of bindings) {
      if (isIdBinding(b)) continue;
      const key = b.workspace_root.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      const binds = bindingsByRoot.get(key) ?? [b];
      const primary = primaryBinding({
        id: '',
        kind: 'mapped-offline',
        root: b.workspace_root,
        bindings: binds,
        isLive: false,
      });
      list.push({
        id: primary!.id,
        kind: 'mapped-offline',
        root: b.workspace_root,
        bindings: binds,
        isLive: false,
      });
    }
    for (const b of bindings) {
      if (!isIdBinding(b)) continue;
      const key = `id:${b.workspace_root.toLowerCase()}`;
      if (seen.has(key)) continue;
      seen.add(key);
      list.push({
        id: b.id,
        kind: 'mapped-offline',
        root: b.workspace_root,
        bindings: [b],
        isLive: false,
        isClientMapping: true,
      });
    }
    const rank: Record<EntryKind, number> = {
      'unmapped-live': 0,
      'live-elsewhere': 1,
      'mapped-live': 2,
      'mapped-offline': 3,
    };
    return list.sort((a, b) => {
      const o = rank[a.kind] - rank[b.kind];
      return o !== 0 ? o : a.root.localeCompare(b.root);
    });
  }, [bindings, bindingsByRoot, reportedRoots, localMachineId, viewerMachineId]);

  const machinesWithBindings = useMemo(() => {
    const ids = new Set<string>();
    for (const entry of entries) {
      for (const b of entry.bindings) {
        if (b.machine_id) ids.add(b.machine_id);
      }
    }
    return machines
      .filter((machine) => ids.has(machine.id))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [entries, machines]);

  const filtered = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return entries.filter((e) => {
      if (filter === 'live' && !e.isLive) return false;
      if (filter === 'mapped' && e.bindings.length === 0) return false;
      if (filter === 'unmapped' && e.kind !== 'unmapped-live') return false;
      if (
        machineFilter !== 'all' &&
        !e.bindings.some((b) => b.machine_id === machineFilter)
      ) {
        return false;
      }
      if (!q) return true;
      const binding = primaryBinding(e);
      const spaceName = binding ? spaceById.get(binding.space_id)?.name ?? '' : '';
      const fsNames = binding
        ? binding.feature_set_ids
            .map((id) => fsById.get(id)?.name ?? '')
            .join(' ')
        : '';
      const label = binding?.label?.toLowerCase() ?? '';
      return (
        e.root.toLowerCase().includes(q) ||
        label.includes(q) ||
        spaceName.toLowerCase().includes(q) ||
        fsNames.toLowerCase().includes(q)
      );
    });
  }, [entries, searchQuery, filter, machineFilter, spaceById, fsById]);

  const showIdentityBanner =
    !localMachineId && bindings.length > 0 && !identityBannerDismissed;

  const counts = useMemo(() => {
    let live = 0;
    let mapped = 0;
    let unmapped = 0;
    for (const e of entries) {
      if (e.isLive) live++;
      if (e.bindings.length > 0) mapped++;
      if (e.kind === 'unmapped-live') unmapped++;
    }
    return { all: entries.length, live, mapped, unmapped };
  }, [entries]);

  const resolveEntryIcon = useCallback(
    (entry: Entry): string | null =>
      primaryBinding(entry)?.icon ??
      appearancesByRoot.get(entry.root.toLowerCase()) ??
      null,
    [appearancesByRoot]
  );

  // Auto-open binding panel for the first unmapped-live entry on page load.
  // Catches `workspace-needs-binding` events that fired before the listener was
  // registered (e.g. Cursor was already connected when this page first rendered).
  useEffect(() => {
    if (isLoading || isPanelOpen) return;
    const firstUnmapped = entries.find((e) => e.kind === 'unmapped-live');
    if (!firstUnmapped) return;

    void (async () => {
      try {
        const dismissed = await isWorkspaceBindingPromptDismissed(firstUnmapped.root);
        if (dismissed || useBindingPanelStore.getState().isOpen) return;
        openBindingPanel({
          mode: 'create-from-live',
          workspaceRoot: firstUnmapped.root,
          appearanceIcon: resolveEntryIcon(firstUnmapped) ?? undefined,
        });
      } catch {
        /* best-effort — skip auto-open on check failure */
      }
    })();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isLoading]);

  const handleRegisterMachine = async (input: {
    name: string;
    icon: string | null;
    hostname: string | null;
  }) => {
    const created = await createMachine(input);
    await persistLocalMachineId(created.id);
    const persistedId = await getLocalMachineId();
    if (persistedId !== created.id) {
      throw new Error('Machine was created but this install identity was not saved');
    }
    setMachines((prev) => [...prev, created].sort((a, b) => a.name.localeCompare(b.name)));
    setLocalMachineId(created.id);
    setShowIdentityModal(false);
    setIdentityBannerDismissed(true);
    success(t('machineIdentity.success'), created.name);
  };

  const handleForgetRoot = async (root: string) => {
    try {
      await forgetReportedRoot(root);
      await loadData();
    } catch (e) {
      showError(t('clearUnmapped.errorTitle'), String(e));
    }
  };

  const handleClearUnmapped = async () => {
    const n = counts.unmapped;
    const ok = await confirm({
      title: t('clearUnmapped.title'),
      message: t('clearUnmapped.message', { count: n }),
      confirmLabel: t('clearUnmapped.confirm'),
      cancelLabel: t('common:actions.cancel'),
      variant: 'danger',
    });
    if (!ok) return;
    try {
      const cleared = await clearUnmappedReportedRoots();
      await loadData();
      success(
        cleared > 0 ? t('clearUnmapped.success', { count: cleared }) : t('clearUnmapped.nothing'),
        cleared > 0 ? t('clearUnmapped.successHint') : undefined
      );
    } catch (e) {
      showError(
        t('clearUnmapped.errorTitle'),
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
                {t('title')}
              </h1>
              <p className="text-base text-[rgb(var(--muted))] mt-2 max-w-2xl">
                {t('subtitle')}
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
                {t('common:actions.refresh')}
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={() => openBindingPanel({ mode: 'create' })}
                data-testid="workspace-binding-create-toggle"
                className="whitespace-nowrap"
              >
                <Plus className="h-4 w-4 mr-2" />
                {t('actions.newBinding')}
              </Button>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3 max-w-3xl">
            <div className="relative flex-1 min-w-[220px]">
              <Search className="absolute left-4 top-1/2 -translate-y-1/2 h-5 w-5 text-[rgb(var(--muted))]" />
              <input
                type="text"
                placeholder={t('searchPlaceholder')}
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
                { value: 'all', label: t('filter.all'), count: counts.all },
                { value: 'live', label: t('filter.live'), count: counts.live },
                { value: 'mapped', label: t('filter.mapped'), count: counts.mapped },
                { value: 'unmapped', label: t('filter.unmapped'), count: counts.unmapped },
              ]}
            />
            {machinesWithBindings.length > 0 ? (
              <div className="relative min-w-[160px]">
                <select
                  value={machineFilter}
                  onChange={(e) => setMachineFilter(e.target.value)}
                  className="w-full appearance-none px-3 py-2 pr-9 bg-[rgb(var(--surface))] border border-[rgb(var(--border))] rounded-xl text-xs font-medium focus:outline-none focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
                  data-testid="workspace-machine-filter"
                  aria-label={t('filter.machine')}
                >
                  <option value="all">{t('filter.allMachines')}</option>
                  {machinesWithBindings.map((machine) => (
                    <option key={machine.id} value={machine.id}>
                      {machine.icon ? `${machine.icon}  ` : ''}
                      {machine.name}
                    </option>
                  ))}
                </select>
                <ChevronDown className="absolute right-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))] pointer-events-none" />
              </div>
            ) : null}
            {counts.unmapped > 0 && (
              <Button
                variant="ghost"
                size="md"
                onClick={handleClearUnmapped}
                title={t('clearUnmapped.message', { count: counts.unmapped })}
                className="whitespace-nowrap text-amber-600 hover:bg-amber-50 hover:text-amber-700 dark:text-amber-400 dark:hover:bg-amber-900/20"
                data-testid="workspaces-clear-unmapped"
              >
                <Trash2 className="h-4 w-4 mr-2" />
                {t('clearUnmapped.button')}
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
          {showIdentityBanner ? (
            <div
              className="mb-6 flex items-start justify-between gap-3 p-4 rounded-xl border border-primary-200/80 dark:border-primary-800/60 bg-primary-50 dark:bg-primary-900/20"
              data-testid="machine-identity-banner"
              role="alert"
            >
              <div className="flex items-start gap-3 min-w-0">
                <Monitor className="h-5 w-5 text-primary-600 dark:text-primary-400 mt-0.5 flex-shrink-0" />
                <p className="text-sm text-primary-800 dark:text-primary-200">
                  {t('machineIdentity.bannerMessage')}
                </p>
              </div>
              <div className="flex items-center gap-2 flex-shrink-0">
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => setShowIdentityModal(true)}
                  data-testid="machine-identity-setup-btn"
                >
                  {t('machineIdentity.bannerSetup')}
                </Button>
                <button
                  type="button"
                  onClick={() => setIdentityBannerDismissed(true)}
                  className="p-1.5 rounded-lg text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
                  aria-label={t('machineIdentity.bannerDismissAria')}
                  data-testid="machine-identity-banner-dismiss"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
            </div>
          ) : null}
          {isLoading ? (
            <div className="flex items-center justify-center h-64">
              <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            </div>
          ) : filtered.length === 0 ? (
            <EmptyState
              hasAny={entries.length > 0}
              hasFilter={searchQuery.length > 0 || filter !== 'all' || machineFilter !== 'all'}
              onCreate={() => openBindingPanel({ mode: 'create' })}
              t={t}
            />
          ) : (
            <div className="grid gap-5 auto-fill-cards">
              {filtered.map((entry) => (
                  <EntryCard
                    key={entry.id}
                    entry={entry}
                    icon={resolveEntryIcon(entry)}
                    bindings={entry.bindings}
                    currentMachineId={viewerMachineId ?? localMachineId}
                    machinesById={machinesById}
                    spaceById={spaceById}
                    fsById={fsById}
                    onClick={() => {
                      const primary = primaryBinding(entry);
                      if (primary) {
                        openBindingPanel({ mode: 'edit', binding: primary });
                        return;
                      }
                      openBindingPanel({
                        mode: 'create-from-live',
                        workspaceRoot: entry.root,
                        appearanceIcon: resolveEntryIcon(entry) ?? undefined,
                      });
                    }}
                    onMachineRowClick={(bindingId) => {
                      const rowBinding = entry.bindings.find((b) => b.id === bindingId);
                      if (rowBinding) {
                        openBindingPanel({ mode: 'edit', binding: rowBinding });
                      }
                    }}
                    onCreateForCurrentMachine={() => {
                      openBindingPanel({
                        mode: 'create-from-live',
                        workspaceRoot: entry.root,
                        appearanceIcon: resolveEntryIcon(entry) ?? undefined,
                      });
                    }}
                    onForget={
                      entry.kind === 'unmapped-live'
                        ? () => handleForgetRoot(entry.root)
                        : undefined
                    }
                    t={t}
                  />
              ))}
            </div>
          )}
        </div>
      </div>

      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}
      <MachineRegistrationModal
        open={showIdentityModal}
        onClose={() => setShowIdentityModal(false)}
        onSubmit={handleRegisterMachine}
        onError={(msg) => showError(t('machineIdentity.error'), msg)}
        t={t}
      />
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
 * Primary title for a workspace entry — label when set, otherwise the path.
 */
function entryDisplayTitle(entry: Entry): string {
  const label = primaryBinding(entry)?.label?.trim();
  if (label) return label;
  if (entry.isClientMapping) return shortClientId(entry.root);
  return entry.root;
}

/** Compact OAuth client id for card badges. */
function shortClientId(clientId: string): string {
  if (clientId.length <= 14) return clientId;
  return `${clientId.slice(0, 8)}…`;
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
// Entry card — matches Clients page card anatomy (56×56 icon, 3xl size, chips)
// ---------------------------------------------------------------------------

/**
 * Resolve a display label for a binding's machine scope.
 */
function machineBindingLabel(
  binding: WorkspaceBinding,
  machinesById: Map<string, Machine>,
  t: TFunction<['workspaces', 'common']>
): string {
  if (binding.machine_id == null) return t('form.noMachine');
  return machinesById.get(binding.machine_id)?.name ?? binding.machine_id;
}

interface EntryCardRoutingRow {
  key: string;
  bindingId?: string;
  /** Opens create panel scoped to the viewer's current machine. */
  createForCurrentMachine?: boolean;
  ghost?: boolean;
  machine?: Machine;
  machineLabel: string;
  fsName: string;
  spaceName: string | undefined;
  clickable: boolean;
}

const ROUTING_GRID_COLS =
  'grid grid-cols-[minmax(0,5.5rem)_minmax(0,1fr)_minmax(0,3.5rem)] gap-x-2';

/**
 * Routing footer for EntryCard — fixed 3-column headers with each binding row
 * rendered as an aligned chip pill (solid for real bindings, dashed for ghosts).
 */
function EntryCardRoutingTable({
  rows,
  onRowClick,
  onCreateForCurrentMachine,
  t,
}: {
  rows: EntryCardRoutingRow[];
  onRowClick?: (bindingId: string) => void;
  onCreateForCurrentMachine?: () => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const headCls =
    'text-left text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]';
  const cellCls = 'min-w-0 text-[11px] text-[rgb(var(--foreground))]';

  return (
    <div className="text-xs">
      <div
        className={`${ROUTING_GRID_COLS} border-b border-[rgb(var(--border-subtle))] pb-1`}
        aria-hidden
      >
        <span className={headCls}>{t('card.machine')}</span>
        <span className={headCls}>{t('card.routesTo')}</span>
        <span className={`${headCls} whitespace-nowrap`}>{t('card.in')}</span>
      </div>
      <div className="mt-1.5 flex flex-col gap-1">
        {rows.map((row) => {
          const fsDisplay = row.fsName || '—';
          const spaceDisplay = row.spaceName ?? '—';
          const rowAction = row.createForCurrentMachine
            ? onCreateForCurrentMachine
            : row.bindingId
              ? () => onRowClick?.(row.bindingId!)
              : undefined;
          const rowProps = row.clickable && rowAction
            ? {
                role: 'button' as const,
                tabIndex: 0,
                'aria-label': row.createForCurrentMachine
                  ? t('card.addBindingForMachine', { machine: row.machineLabel })
                  : t('card.machineRow', { machine: row.machineLabel }),
                onClick: (event: ReactMouseEvent<HTMLDivElement>) => {
                  event.stopPropagation();
                  rowAction();
                },
                onKeyDown: (event: ReactKeyboardEvent<HTMLDivElement>) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    event.stopPropagation();
                    rowAction();
                  }
                },
              }
            : {};

          return (
            <div
              key={row.key}
              className={[
                ROUTING_GRID_COLS,
                'items-start rounded-md border px-1.5 py-1',
                row.ghost
                  ? 'border-dashed border-[rgb(var(--border-subtle))] opacity-70'
                  : 'border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))]',
                row.clickable && rowAction
                  ? 'cursor-pointer transition-colors hover:bg-[rgb(var(--surface-hover,var(--background)))]'
                  : '',
              ].join(' ')}
              {...rowProps}
            >
              <span className={`${cellCls} truncate whitespace-nowrap`} title={row.machineLabel}>
                <span className="inline-flex max-w-full items-center gap-1">
                  {row.machine?.icon ? (
                    <span className="shrink-0 text-[11px] leading-none">{row.machine.icon}</span>
                  ) : null}
                  <span className="truncate">{row.machineLabel}</span>
                </span>
              </span>
              <span
                className={[
                  cellCls,
                  'break-words font-medium leading-snug',
                  row.ghost
                    ? 'italic text-[rgb(var(--muted))]'
                    : 'text-primary-700 dark:text-primary-300',
                ].join(' ')}
              >
                {fsDisplay}
              </span>
              <span className={`${cellCls} truncate whitespace-nowrap`} title={spaceDisplay}>
                {spaceDisplay}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Build routing table rows for an entry — every binding is clickable; when live
 * on a foreign machine, append a ghost row for the viewer's current machine.
 */
function buildEntryRoutingRows(
  entry: Entry,
  bindings: WorkspaceBinding[],
  currentMachineId: string | null,
  machinesById: Map<string, Machine>,
  spaceById: Map<string, Space>,
  fsById: Map<string, FeatureSet>,
  t: TFunction<['workspaces', 'common']>,
): EntryCardRoutingRow[] {
  const rows: EntryCardRoutingRow[] = bindings.map((rowBinding) => {
    const rowMachine = rowBinding.machine_id
      ? machinesById.get(rowBinding.machine_id)
      : undefined;
    return {
      key: rowBinding.id,
      bindingId: rowBinding.id,
      machine: rowMachine,
      machineLabel: machineBindingLabel(rowBinding, machinesById, t),
      fsName: formatFsList(
        rowBinding.feature_set_ids.map((id) => fsById.get(id)?.name ?? id),
      ),
      spaceName: spaceById.get(rowBinding.space_id)?.name,
      clickable: true,
    };
  });

  const needsGhostRow =
    entry.kind === 'live-elsewhere' &&
    Boolean(currentMachineId) &&
    !bindings.some(
      (b) => b.machine_id == null || b.machine_id === currentMachineId,
    );

  if (needsGhostRow && currentMachineId) {
    const currentMachine = machinesById.get(currentMachineId);
    rows.push({
      key: `ghost:${currentMachineId}`,
      createForCurrentMachine: true,
      ghost: true,
      machine: currentMachine,
      machineLabel: currentMachine?.name ?? currentMachineId,
      fsName: t('card.notConfigured'),
      spaceName: undefined,
      clickable: true,
    });
  }

  if (entry.kind === 'unmapped-live' && bindings.length === 0) {
    const currentMachine = currentMachineId
      ? machinesById.get(currentMachineId)
      : undefined;
    rows.push({
      key: 'ghost:unmapped',
      createForCurrentMachine: true,
      ghost: true,
      machine: currentMachine,
      machineLabel: currentMachine?.name ?? t('card.addBinding'),
      fsName: '—',
      spaceName: undefined,
      clickable: true,
    });
  }

  return rows;
}

/**
 * Project card — identity header plus routing table footer.
 */
function EntryCard({
  entry,
  icon,
  bindings,
  currentMachineId,
  machinesById,
  spaceById,
  fsById,
  onClick,
  onMachineRowClick,
  onCreateForCurrentMachine,
  onForget,
  t,
}: {
  entry: Entry;
  icon: string | null;
  bindings: WorkspaceBinding[];
  currentMachineId: string | null;
  machinesById: Map<string, Machine>;
  spaceById: Map<string, Space>;
  fsById: Map<string, FeatureSet>;
  onClick: () => void;
  onMachineRowClick: (bindingId: string) => void;
  onCreateForCurrentMachine: () => void;
  onForget?: () => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const tone =
    entry.kind === 'unmapped-live'
      ? 'amber'
      : entry.kind === 'live-elsewhere'
        ? 'info'
        : entry.kind === 'mapped-live'
          ? 'emerald'
          : 'neutral';

  const displayTitle = entryDisplayTitle(entry);
  const binding = primaryBinding(entry);
  const hasLabel = Boolean(binding?.label?.trim());
  const routingRows = buildEntryRoutingRows(
    entry,
    bindings,
    currentMachineId,
    machinesById,
    spaceById,
    fsById,
    t,
  );
  return (
    <Card
      className="group relative h-full cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01]"
      onClick={onClick}
      data-testid={`workspace-entry-${entry.id}`}
    >
      <CardContent className="flex h-full flex-col p-6">
        {onForget && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onForget();
            }}
            title={t('card.forgetRoot')}
            className="absolute right-3 top-3 z-10 rounded-full p-1 text-[rgb(var(--muted))] opacity-0 transition-opacity group-hover:opacity-100 hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--foreground))]"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        )}
        <div className="mb-4 flex flex-1 items-start gap-4">
          <div className="relative flex-shrink-0">
            <div
              className={[
                'w-14 h-14 flex items-center justify-center rounded-xl border',
                tone === 'amber'
                  ? 'bg-amber-50 dark:bg-amber-900/20 border-amber-200/80 dark:border-amber-800/50 text-amber-600 dark:text-amber-400'
                  : tone === 'info'
                    ? 'bg-violet-50 dark:bg-violet-900/20 border-violet-200/80 dark:border-violet-800/50 text-violet-600 dark:text-violet-400'
                    : tone === 'emerald'
                      ? 'bg-emerald-50 dark:bg-emerald-900/20 border-emerald-200/80 dark:border-emerald-800/50 text-emerald-600 dark:text-emerald-400'
                      : 'bg-[rgb(var(--surface))] border-[rgb(var(--border-subtle))] text-[rgb(var(--muted))]',
              ].join(' ')}
            >
              {icon ? (
                <ServerIcon icon={icon} className="h-8 w-8 object-contain" fallback="📁" />
              ) : entry.isClientMapping ? (
                <UserRound className="h-6 w-6" />
              ) : (
                <FolderOpen className="h-6 w-6" />
              )}
            </div>
            {entry.isLive && (
              <span
                className="absolute -top-0.5 -right-0.5 h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-[rgb(var(--background))]"
                title={t('card.liveTooltip')}
              />
            )}
          </div>
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="mb-1.5 flex min-h-[1.375rem] flex-wrap items-center gap-2">
              {entry.kind === 'unmapped-live' && (
                <Pill tone="amber" title={t('card.deniedTooltip')}>
                  {t('card.badgeLiveUnbound')}
                </Pill>
              )}
              {entry.kind === 'live-elsewhere' && (
                <Pill tone="info">{t('card.badgeBoundElsewhere')}</Pill>
              )}
              {entry.kind === 'mapped-offline' && <Pill tone="neutral">{t('card.offline')}</Pill>}
              {entry.isClientMapping && (
                <Pill tone="neutral" title={entry.root}>
                  Client mapping
                </Pill>
              )}
              {entry.kind === 'mapped-live' && <Pill tone="emerald">{t('card.live')}</Pill>}
              {binding?.client_id && (
                <Pill tone="neutral" title={binding.client_id}>
                  {shortClientId(binding.client_id)}
                </Pill>
              )}
            </div>
            <p
              className={`line-clamp-2 min-h-[2.5rem] text-sm leading-snug text-[rgb(var(--foreground))] ${
                hasLabel ? 'font-semibold' : 'break-all font-mono'
              }`}
              title={displayTitle}
            >
              {displayTitle}
            </p>
            <p
              className={`mt-0.5 line-clamp-2 min-h-[2rem] font-mono text-xs leading-snug text-[rgb(var(--muted))] ${
                hasLabel || entry.isClientMapping ? 'break-all' : 'invisible'
              }`}
              title={hasLabel || entry.isClientMapping ? entry.root : undefined}
              aria-hidden={!hasLabel && !entry.isClientMapping}
            >
              {hasLabel || entry.isClientMapping ? entry.root : '\u00A0'}
            </p>
            {entry.kind === 'unmapped-live' && (
              <Button
                variant="primary"
                size="sm"
                className="mt-3 w-full"
                title={t('card.deniedTooltip')}
                onClick={(e) => {
                  e.stopPropagation();
                  onClick();
                }}
                data-testid="workspace-entry-bind-cta"
              >
                {t('card.deniedCta')}
              </Button>
            )}
          </div>
        </div>

        <div className="mt-auto -mx-6 -mb-6 rounded-b-xl bg-[rgb(var(--surface))] px-5 py-3 text-xs text-[rgb(var(--muted))]">
          <EntryCardRoutingTable
            rows={routingRows}
            onRowClick={onMachineRowClick}
            onCreateForCurrentMachine={onCreateForCurrentMachine}
            t={t}
          />
        </div>
      </CardContent>
    </Card>
  );
}

function Pill({
  children,
  tone,
  title,
}: {
  children: React.ReactNode;
  tone: 'amber' | 'emerald' | 'neutral' | 'info';
  title?: string;
}) {
  const cls =
    tone === 'amber'
      ? 'bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 border-amber-200/80 dark:border-amber-800/60'
      : tone === 'info'
        ? 'bg-violet-50 dark:bg-violet-900/20 text-violet-700 dark:text-violet-400 border-violet-200/80 dark:border-violet-800/60'
        : tone === 'emerald'
          ? 'bg-emerald-50 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-400 border-emerald-200/80 dark:border-emerald-800/60'
          : 'bg-[rgb(var(--surface))] text-[rgb(var(--muted))] border-[rgb(var(--border-subtle))]';
  return (
    <span
      className={`inline-flex items-center px-1.5 py-0.5 rounded-md border text-[10px] font-semibold uppercase tracking-wider ${cls}`}
      title={title}
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

/** Imperative handle for programmatically expanding a collapsible section. */
export type CollapsibleSectionRef = {
  expand: () => void;
};

export const CollapsibleSection = forwardRef<
  CollapsibleSectionRef,
  {
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
  }
>(function CollapsibleSection(
  {
    icon,
    tone = 'primary',
    title,
    subtitle,
    defaultOpen = true,
    badge,
    headerExtra,
    testId,
    children,
  },
  ref,
) {
  const [open, setOpen] = useState(defaultOpen);
  const toneSpec = SECTION_TONES[tone] ?? SECTION_TONES.primary;

  useImperativeHandle(ref, () => ({
    expand: () => setOpen(true),
  }));

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
            ? toneSpec.gradientOpen
            : 'bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]',
        ].join(' ')}
        aria-expanded={open}
      >
        <div className="flex items-center gap-3 min-w-0 flex-1">
          <div
            className={[
              'p-2 rounded-lg flex-shrink-0 transition-colors duration-200',
              open ? toneSpec.iconActive : toneSpec.iconQuiet,
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
                      ? toneSpec.badgeOpen
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
});

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
export function EffectiveFeaturesContent({
  root,
  machineId,
  onTotalChange,
  t,
}: {
  root: string;
  machineId?: string | null;
  onTotalChange?: (total: number | null) => void;
  t: TFunction<['workspaces', 'common']>;
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
    void getWorkspaceEffectiveFeatures(root, machineId)
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
  }, [root, machineId, onTotalChange]);

  const reloadEffectiveFeatures = useCallback(() => {
    void getWorkspaceEffectiveFeatures(root, machineId)
      .then((d) => {
        setData(d);
        onTotalChange?.(d.tools.length + d.prompts.length + d.resources.length);
      })
      .catch(() => {
        /* ignore — initial load already surfaced any error */
      });
  }, [root, machineId, onTotalChange]);

  // Re-fetch on binding / server-status changes so the panel stays honest
  // without the user reopening it.
  useWorkspaceEventListener(
    reloadEffectiveFeatures,
    EFFECTIVE_FEATURES_REFRESH_CHANNELS
  );

  useServerStatusEvents(reloadEffectiveFeatures);

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

  if (data.source === 'unbound') {
    return (
      <div className="text-center py-8 text-[rgb(var(--muted))]">
        <Package className="h-8 w-8 mx-auto mb-2 opacity-50" />
        <p className="text-sm">{t('effective.unboundEmpty')}</p>
      </div>
    );
  }

  const allAvailable = totalCount > 0 && availableCount === totalCount;
  const partialAvailable = availableCount > 0 && availableCount < totalCount;

  return (
    <div className="space-y-4">
      {/* Resolution summary — bold pills showing what this folder
          resolves to, plus a progress bar for availability. */}
      <div className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3 space-y-2.5">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-[10px] font-bold uppercase tracking-wider text-[rgb(var(--muted))]">
            {t('effective.resolvesTo')}
          </span>
          <span className="text-sm font-semibold text-[rgb(var(--foreground))] truncate">
            {formatFsList(data.feature_sets.map((fs) => fs.name)) || '—'}
          </span>
          <span className="text-xs text-[rgb(var(--muted))]">{t('effective.in')}</span>
          <span className="text-sm font-medium text-[rgb(var(--foreground))]">
            {data.space_name}
          </span>
          <span
            title={
              data.source === 'binding'
                ? t('effective.sourceBindingTooltip')
                : t('effective.sourceUnboundTooltip')
            }
            className={[
              'ml-auto text-[10px] px-2 py-0.5 rounded-full font-bold uppercase tracking-wider border',
              data.source === 'binding'
                ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 border-purple-300/70 dark:border-purple-700/70'
                : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 border-amber-300/70 dark:border-amber-700/70',
            ].join(' ')}
          >
            {data.source === 'binding' ? t('effective.sourceBinding') : t('effective.sourceUnbound')}
          </span>
        </div>

        {/* Availability progress bar. Stays quiet (green) when all servers
            are connected, leans amber when some are dim. */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs">
            <span className="text-[rgb(var(--muted))] tabular-nums">
              <span>{t('effective.availableOf', { available: availableCount, total: totalCount })}</span>
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
                {allAvailable ? t('effective.allReady') : partialAvailable ? t('effective.partial') : t('effective.offline')}
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
          <p className="text-sm">{t('effective.noFeatures')}</p>
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
                t={t}
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
  t,
}: {
  group: ServerGroup;
  open: boolean;
  onToggle: () => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const issue = serverStatusIssue(group.server_status, t);
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
          <ServerGlyph className="h-4 w-4 text-blue-500 flex-shrink-0" />
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
          <FeatureSubGroup label="tool" items={group.tools} t={t} />
          <FeatureSubGroup label="prompt" items={group.prompts} t={t} />
          <FeatureSubGroup label="resource" items={group.resources} t={t} />
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
  t,
}: {
  label: 'tool' | 'prompt' | 'resource';
  items: EffectiveFeature[];
  t: TFunction<['workspaces', 'common']>;
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
                {t(`effective.featureType.${label}`)}
              </span>
              {!item.available && (
                <span className="text-[9px] uppercase tracking-wider font-bold text-[rgb(var(--muted))]">
                  {t('effective.unavailable')}
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
  status: EffectiveFeature['server_status'],
  t: TFunction<['workspaces', 'common']>
): { label: string; tone: 'red' | 'amber' | 'muted' } | null {
  switch (status) {
    case 'connected':
      return null;
    case 'connecting':
      return { label: t('serverStatus.connecting'), tone: 'amber' };
    case 'authenticating':
      return { label: t('serverStatus.authenticating'), tone: 'amber' };
    case 'refreshing':
      return { label: t('serverStatus.refreshing'), tone: 'amber' };
    case 'auth_required':
      return { label: t('serverStatus.authNeeded'), tone: 'amber' };
    case 'error':
      return { label: t('serverStatus.error'), tone: 'red' };
    case 'disconnected':
      return { label: t('serverStatus.disconnected'), tone: 'muted' };
    case 'unknown':
    default:
      return { label: t('serverStatus.offline'), tone: 'muted' };
  }
}

// ---------------------------------------------------------------------------
// Machine registration modal (first-time identity prompt)
// ---------------------------------------------------------------------------

/**
 * Small modal for registering this McpMux install as a named machine.
 */
function MachineRegistrationModal({
  open,
  onClose,
  onSubmit,
  onError,
  t,
}: {
  open: boolean;
  onClose: () => void;
  onSubmit: (input: {
    name: string;
    icon: string | null;
    hostname: string | null;
  }) => Promise<void>;
  onError: (message: string) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const [name, setName] = useState('');
  const [icon, setIcon] = useState('');
  const [hostname, setHostname] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const nameRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    setName('');
    setIcon('');
    setSubmitting(false);
    void getHostname()
      .then((h) => setHostname(h))
      .catch(() => setHostname(''));
    const handle = setTimeout(() => nameRef.current?.focus(), 50);
    return () => clearTimeout(handle);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  const handleConfirm = async () => {
    const missingField = getMissingMachineProfileField({ name, icon, hostname });
    if (missingField) {
      onError(t(`machineIdentity.${missingField}Required`));
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(toMachineProfilePayload({ name, icon, hostname }));
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  if (!open) return null;

  const canSubmit = isMachineProfileComplete({ name, icon, hostname });

  return (
    <>
      <div
        className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/50 p-4"
        data-testid="machine-identity-modal-overlay"
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        <Card
          className="animate-in fade-in zoom-in-95 w-full max-w-md shadow-2xl duration-200"
          data-testid="machine-identity-modal"
        >
          <CardContent className="p-6 space-y-5">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-lg font-bold">{t('machineIdentity.modalTitle')}</h2>
                <p className="text-sm text-[rgb(var(--muted))] mt-1">
                  {t('machineIdentity.modalSubtitle')}
                </p>
              </div>
              <button
                type="button"
                onClick={onClose}
                className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors"
                aria-label={t('panel.closeAria')}
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            <FormField label={t('machineIdentity.nameLabel')}>
              <input
                ref={nameRef}
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t('machineIdentity.namePlaceholder')}
                className="w-full px-3 py-2 rounded-lg text-sm bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="machine-identity-name-input"
              />
            </FormField>

            <FormField label={t('machineIdentity.iconLabel')}>
              <EmojiPickerButton
                value={icon}
                onChange={setIcon}
                testId="machine-identity-icon-input"
              />
            </FormField>

            <FormField
              label={t('machineIdentity.hostnameLabel')}
              hint={t('machineIdentity.hostnameHint')}
            >
              <input
                type="text"
                value={hostname}
                onChange={(e) => setHostname(e.target.value)}
                className="w-full px-3 py-2 rounded-lg text-sm font-mono bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="machine-identity-hostname-input"
              />
            </FormField>

            <div className="flex items-center gap-2 pt-1">
              <Button
                variant="primary"
                size="md"
                onClick={() => void handleConfirm()}
                disabled={submitting || !canSubmit}
                className="flex-1"
                data-testid="machine-identity-confirm-btn"
              >
                {submitting ? (
                  <Loader2 className="h-4 w-4 animate-spin mr-1.5" />
                ) : (
                  <Check className="h-4 w-4 mr-1.5" />
                )}
                {t('machineIdentity.confirm')}
              </Button>
              <Button variant="secondary" size="md" onClick={onClose} disabled={submitting}>
                {t('common:actions.cancel')}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

function EmptyState({
  hasAny,
  hasFilter,
  onCreate,
  t,
}: {
  hasAny: boolean;
  hasFilter: boolean;
  onCreate: () => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  if (hasFilter && hasAny) {
    return (
      <Card className="max-w-2xl mx-auto">
        <CardContent className="flex flex-col items-center justify-center py-16">
          <Search className="h-16 w-16 text-[rgb(var(--muted))] mb-4" />
          <h3 className="text-lg font-medium mb-2">{t('empty.noMatchTitle')}</h3>
          <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md">
            {t('empty.noMatchSubtitle')}
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
        <h3 className="text-lg font-medium mb-2">{t('empty.nothingTitle')}</h3>
        <p className="text-sm text-[rgb(var(--muted))] text-center max-w-md mb-6">
          {t('empty.nothingSubtitle')}
        </p>
        <Button variant="primary" onClick={onCreate}>
          <Plus className="h-4 w-4 mr-2" />
          {t('empty.addBinding')}
        </Button>
      </CardContent>
    </Card>
  );
}
