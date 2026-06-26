import {
  useCallback,
  useEffect,
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
import { pickPath } from '@/lib/backend/shell';
import { isTauri } from '@/lib/backend/data/transport';
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
  Server as ServerGlyph,
  Trash2,
  Wrench,
  X,
  Monitor,
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
  createMachine,
  getHostname,
  getLocalMachineId,
  listMachines,
  setLocalMachineId as persistLocalMachineId,
  type Machine,
} from '@/lib/api/machines';
import {
  deleteWorkspaceAppearance,
  listWorkspaceAppearances,
  upsertWorkspaceAppearance,
  uploadWorkspaceIcon,
  type WorkspaceAppearance,
} from '@/lib/api/workspaceAppearances';
import {
  isStarterFeatureSet,
  listFeatureSets,
  type FeatureSet,
} from '@/lib/api/featureSets';
import { WorkspaceInstallPanel } from './WorkspaceInstallPanel';
import { WorkspaceSetupWizard } from './WorkspaceSetupWizard';
import { useSpaces, usePendingWorkspaceNew, useSetPendingWorkspaceNew } from '@/stores';
import { ServerIcon } from '@/components/ServerIcon';
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
 *   • LIVE + unmapped              → amber UNMAPPED
 *   • LIVE + bound (other machine) → amber UNBOUND
 *   • LIVE + bound (this machine)  → emerald LIVE
 *   • OFFLINE + mapped             → neutral
 */

type EntryKind = 'unmapped-live' | 'live-unbound' | 'mapped-live' | 'mapped-offline';
interface Entry {
  id: string;
  kind: EntryKind;
  root: string;
  bindings: WorkspaceBinding[];
  isLive: boolean;
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

type Selected = { mode: 'new' } | { mode: 'entry'; id: string; bindingId?: string };

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

  const [selected, setSelected] = useState<Selected | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [filter, setFilter] = useState<'all' | 'live' | 'mapped' | 'unmapped'>('all');
  const [machineFilter, setMachineFilter] = useState<string>('all');
  const [identityBannerDismissed, setIdentityBannerDismissed] = useState(false);
  const [showIdentityModal, setShowIdentityModal] = useState(false);
  /** Optimistic icon overrides while the inspector panel is open. */
  const [liveEntryIcons, setLiveEntryIcons] = useState<Map<string, string>>(new Map());

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

  // Opened from the home "Set up a folder" CTA — launch the create walkthrough.
  useEffect(() => {
    if (pendingNew) {
      setSelected({ mode: 'new' });
      clearPendingNew(false);
    }
  }, [pendingNew, clearPendingNew]);

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
      if (binds.length > 0 && entryIsBoundForCurrentMachine(entry, localMachineId)) {
        entry.kind = 'mapped-live';
      } else if (binds.length > 0) {
        entry.kind = 'live-unbound';
      }
      list.push(entry);
    }
    for (const b of bindings) {
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
    const rank: Record<EntryKind, number> = {
      'unmapped-live': 0,
      'live-unbound': 1,
      'mapped-live': 2,
      'mapped-offline': 3,
    };
    return list.sort((a, b) => {
      const o = rank[a.kind] - rank[b.kind];
      return o !== 0 ? o : a.root.localeCompare(b.root);
    });
  }, [bindings, bindingsByRoot, reportedRoots, localMachineId]);

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

  const selectedEntry: Entry | null =
    selected?.mode === 'entry' ? entries.find((e) => e.id === selected.id) ?? null : null;
  const selectedBindingId =
    selected?.mode === 'entry' ? selected.bindingId : undefined;
  const activeBinding = selectedEntry
    ? selectedBindingId
      ? selectedEntry.bindings.find((b) => b.id === selectedBindingId) ??
        primaryBinding(selectedEntry)
      : primaryBinding(selectedEntry)
    : null;
  const resolveEntryIcon = useCallback(
    (entry: Entry): string | null =>
      liveEntryIcons.get(entry.id) ??
      primaryBinding(entry)?.icon ??
      appearancesByRoot.get(entry.root.toLowerCase()) ??
      null,
    [appearancesByRoot, liveEntryIcons]
  );

  const selectedIsNew = selected?.mode === 'new';
  const panelOpen = selected !== null;

  useEffect(() => {
    if (!panelOpen) {
      setLiveEntryIcons(new Map());
    }
  }, [panelOpen]);

  const handleCreate = async (input: WorkspaceBindingInput): Promise<WorkspaceBinding> => {
    const created = await createWorkspaceBinding(input);
    setBindings((prev) =>
      [...prev, created].sort((a, b) => a.workspace_root.localeCompare(b.workspace_root))
    );
    success(t('toast.bindingSaved'), created.workspace_root);
    return created;
  };

  const handleUpdate = async (id: string, input: WorkspaceBindingInput) => {
    const updated = await updateWorkspaceBinding(id, input);
    setBindings((prev) =>
      prev
        .map((b) => (b.id === id ? updated : b))
        .sort((a, b) => a.workspace_root.localeCompare(b.workspace_root))
    );
    success(t('toast.bindingUpdated'), updated.workspace_root);
  };

  const handleDelete = async (binding: WorkspaceBinding) => {
    const ok = await confirm({
      title: t('confirm.removeTitle'),
      message: t('confirm.removeMessage', { path: binding.workspace_root }),
      confirmLabel: t('confirm.removeLabel'),
      cancelLabel: t('common:actions.cancel'),
      variant: 'danger',
    });
    if (!ok) return;
    try {
      await deleteWorkspaceBinding(binding.id);
      setBindings((prev) => prev.filter((b) => b.id !== binding.id));
      setSelected(null);
      success(t('toast.bindingRemoved'), binding.workspace_root);
    } catch (e) {
      showError(t('toast.failedToRemove'), e instanceof Error ? e.message : String(e));
    }
  };

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
                onClick={() => setSelected({ mode: 'new' })}
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
              onCreate={() => setSelected({ mode: 'new' })}
              t={t}
            />
          ) : (
            <div className="grid gap-5 auto-fill-cards">
              {filtered.map((entry) => {
                const isSelected =
                  selected?.mode === 'entry' && selected.id === entry.id;
                const binding = primaryBinding(entry);
                // For mapped entries: trust the binding. For unmapped: fall
                // back to the system's default Space + its Default FS so
                // every card answers "what tools does this folder see?".
                const resolvedSpaceName = binding
                  ? spaceById.get(binding.space_id)?.name
                  : fallback?.space.name;
                const resolvedFsName = binding
                  ? formatFsList(
                      binding.feature_set_ids.map(
                        (id) => fsById.get(id)?.name ?? id
                      )
                    )
                  : fallback?.fs?.name;
                return (
                  <EntryCard
                    key={entry.id}
                    entry={entry}
                    icon={resolveEntryIcon(entry)}
                    spaceName={resolvedSpaceName}
                    fsName={resolvedFsName}
                    bindings={entry.bindings}
                    machinesById={machinesById}
                    spaceById={spaceById}
                    fsById={fsById}
                    selected={isSelected}
                    onClick={() => setSelected({ mode: 'entry', id: entry.id })}
                    onMachineRowClick={(bindingId) =>
                      setSelected({ mode: 'entry', id: entry.id, bindingId })
                    }
                    t={t}
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
              onCreate={async (input) => {
                const created = await handleCreate(input);
                // Land on the new mapping's inspector so its effective features
                // are shown right after creation.
                setSelected({ mode: 'entry', id: created.id });
                return created;
              }}
              onError={(msg) => showError(t('toast.couldNotSave'), msg)}
            />
          ) : (
            <InspectorPanel
              key={`${selectedEntry?.id ?? 'entry'}:${selectedBindingId ?? 'primary'}`}
              entry={selectedEntry}
              binding={activeBinding}
              isNew={false}
              resolvedIcon={selectedEntry ? resolveEntryIcon(selectedEntry) : null}
              spaces={spaces}
              featureSets={featureSets}
              machines={machines}
              localMachineId={localMachineId}
              onClose={() => setSelected(null)}
              onSubmit={async (input) => {
                if (activeBinding) {
                  await handleUpdate(activeBinding.id, input);
                } else {
                  const created = await handleCreate(input);
                  setSelected({ mode: 'entry', id: created.id });
                }
              }}
              onDelete={async () => {
                if (activeBinding) await handleDelete(activeBinding);
              }}
              onIconChange={(icon) => {
                if (!selectedEntry) return;
                setLiveEntryIcons((prev) => {
                  const next = new Map(prev);
                  if (icon) {
                    next.set(selectedEntry.id, icon);
                  } else {
                    next.delete(selectedEntry.id);
                  }
                  return next;
                });
              }}
              onError={(msg) => showError(t('toast.couldNotSave'), msg)}
              t={t}
            />
          )}
        </>
      )}

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
 * Structural equality between two binding inputs. The autosave effect
 * uses this to skip writes when the user re-toggled their way back to
 * the last-saved state — avoids spamming `WorkspaceBindingChanged` for
 * a no-op edit. `feature_set_ids` order matters (it's the operator-
 * chosen render order, not just a set), so we compare positionally.
 */
function normalizeLabel(label: string | null | undefined): string | null {
  const trimmed = label?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeIcon(icon: string | null | undefined): string | null {
  const trimmed = icon?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : null;
}

/**
 * Primary title for a workspace entry — label when set, otherwise the path.
 */
function entryDisplayTitle(entry: Entry): string {
  const label = primaryBinding(entry)?.label?.trim();
  if (label) return label;
  return entry.root;
}

/** Compact OAuth client id for card badges. */
function shortClientId(clientId: string): string {
  if (clientId.length <= 14) return clientId;
  return `${clientId.slice(0, 8)}…`;
}

function sameBindingInput(
  a: WorkspaceBindingInput,
  b: {
    workspace_root: string;
    label?: string | null;
    icon?: string | null;
    space_id: string;
    feature_set_ids: string[];
    machine_id?: string | null;
  }
): boolean {
  if (a.workspace_root.trim() !== b.workspace_root.trim()) return false;
  if (normalizeLabel(a.label) !== normalizeLabel(b.label)) return false;
  if (normalizeIcon(a.icon) !== normalizeIcon(b.icon)) return false;
  if (a.space_id !== b.space_id) return false;
  if ((a.machine_id ?? null) !== (b.machine_id ?? null)) return false;
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
  machine?: Machine;
  machineLabel: string;
  fsName: string;
  spaceName: string | undefined;
  clickable: boolean;
}

/**
 * Compact routing table for EntryCard footer — machine, feature set, space.
 * Uses semantic HTML table (no Table primitive in @mcpmux/ui). Feature set
 * names wrap to additional lines when needed.
 */
function EntryCardRoutingTable({
  rows,
  showMachineColumn,
  onRowClick,
  t,
}: {
  rows: EntryCardRoutingRow[];
  showMachineColumn: boolean;
  onRowClick?: (bindingId: string) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const headCls =
    'pb-1 pr-2 text-left text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))] last:pr-0';
  const cellCls = 'py-0.5 pr-2 align-top text-[11px] text-[rgb(var(--foreground))] last:pr-0';

  return (
    <table className="w-full border-collapse text-xs">
      <colgroup>
        {showMachineColumn ? <col className="w-px" /> : null}
        <col />
        <col className="w-px" />
      </colgroup>
      <thead>
        <tr className="border-b border-[rgb(var(--border-subtle))]">
          {showMachineColumn ? <th className={headCls}>{t('card.machine')}</th> : null}
          <th className={headCls}>{t('card.routesTo')}</th>
          <th className={`${headCls} whitespace-nowrap`}>{t('card.in')}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => {
          const fsDisplay = row.fsName || '—';
          const spaceDisplay = row.spaceName ?? '—';
          const rowProps = row.clickable
            ? {
                role: 'button' as const,
                tabIndex: 0,
                className:
                  'cursor-pointer transition-colors hover:bg-[rgb(var(--surface-hover,var(--background)))]',
                'aria-label': t('card.machineRow', { machine: row.machineLabel }),
                onClick: row.bindingId
                  ? (event: ReactMouseEvent<HTMLTableRowElement>) => {
                      event.stopPropagation();
                      onRowClick?.(row.bindingId!);
                    }
                  : undefined,
                onKeyDown: row.bindingId
                  ? (event: ReactKeyboardEvent<HTMLTableRowElement>) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault();
                        event.stopPropagation();
                        onRowClick?.(row.bindingId!);
                      }
                    }
                  : undefined,
              }
            : {};

          return (
            <tr key={row.key} {...rowProps}>
              {showMachineColumn ? (
                <td className={`${cellCls} whitespace-nowrap`} title={row.machineLabel}>
                  <span className="inline-flex max-w-[7rem] items-center gap-1">
                    {row.machine?.icon ? (
                      <span className="shrink-0 text-[11px] leading-none">{row.machine.icon}</span>
                    ) : null}
                    <span className="truncate">{row.machineLabel}</span>
                  </span>
                </td>
              ) : null}
              <td className={cellCls}>
                <span className="block break-words font-medium leading-snug text-primary-700 dark:text-primary-300">
                  {fsDisplay}
                </span>
              </td>
              <td className={`${cellCls} whitespace-nowrap`} title={spaceDisplay}>
                {spaceDisplay}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/**
 * Project card — identity header plus routing table footer.
 */
function EntryCard({
  entry,
  icon,
  spaceName,
  fsName,
  bindings,
  machinesById,
  spaceById,
  fsById,
  selected,
  onClick,
  onMachineRowClick,
  t,
}: {
  entry: Entry;
  icon: string | null;
  spaceName: string | undefined;
  fsName: string | undefined;
  bindings: WorkspaceBinding[];
  machinesById: Map<string, Machine>;
  spaceById: Map<string, Space>;
  fsById: Map<string, FeatureSet>;
  selected: boolean;
  onClick: () => void;
  onMachineRowClick: (bindingId: string) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const tone =
    entry.kind === 'unmapped-live' || entry.kind === 'live-unbound'
      ? 'amber'
      : entry.kind === 'mapped-live'
        ? 'emerald'
        : 'neutral';

  const displayTitle = entryDisplayTitle(entry);
  const binding = primaryBinding(entry);
  const hasLabel = Boolean(binding?.label?.trim());
  const isMultiMachine = bindings.length > 1;
  const singleMachineBinding =
    bindings.length === 1 && bindings[0].machine_id != null ? bindings[0] : null;
  const singleMachine = singleMachineBinding
    ? machinesById.get(singleMachineBinding.machine_id!)
    : undefined;
  const showMachineColumn = bindings.some((b) => b.machine_id != null);
  const routingRows: EntryCardRoutingRow[] = isMultiMachine
    ? bindings.map((rowBinding) => {
        const rowMachine = rowBinding.machine_id
          ? machinesById.get(rowBinding.machine_id)
          : undefined;
        return {
          key: rowBinding.id,
          bindingId: rowBinding.id,
          machine: rowMachine,
          machineLabel: machineBindingLabel(rowBinding, machinesById, t),
          fsName: formatFsList(
            rowBinding.feature_set_ids.map((id) => fsById.get(id)?.name ?? id)
          ),
          spaceName: spaceById.get(rowBinding.space_id)?.name,
          clickable: true,
        };
      })
    : [
        {
          key: binding?.id ?? entry.id,
          machine: singleMachine,
          machineLabel: singleMachine?.name ?? t('form.noMachine'),
          fsName: fsName ?? '',
          spaceName,
          clickable: false,
        },
      ];

  return (
    <Card
      className={`h-full cursor-pointer transition-all hover:shadow-lg hover:scale-[1.01] ${
        selected ? 'ring-2 ring-primary-500 shadow-lg' : ''
      }`}
      onClick={onClick}
      data-testid={`workspace-entry-${entry.id}`}
    >
      <CardContent className="flex h-full flex-col p-6">
        <div className="mb-4 flex flex-1 items-start gap-4">
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
              {icon ? (
                <ServerIcon icon={icon} className="h-8 w-8 object-contain" fallback="📁" />
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
              {entry.kind === 'unmapped-live' && <Pill tone="amber">{t('card.unmapped')}</Pill>}
              {entry.kind === 'live-unbound' && (
                <Pill tone="amber">{t('card.badgeLiveUnbound')}</Pill>
              )}
              {entry.kind === 'mapped-offline' && <Pill tone="neutral">{t('card.offline')}</Pill>}
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
                hasLabel ? 'break-all' : 'invisible'
              }`}
              title={hasLabel ? entry.root : undefined}
              aria-hidden={!hasLabel}
            >
              {hasLabel ? entry.root : '\u00A0'}
            </p>
          </div>
        </div>

        <div className="mt-auto -mx-6 -mb-6 rounded-b-xl bg-[rgb(var(--surface))] px-5 py-3 text-xs text-[rgb(var(--muted))]">
          <EntryCardRoutingTable
            rows={routingRows}
            showMachineColumn={showMachineColumn}
            onRowClick={isMultiMachine ? onMachineRowClick : undefined}
            t={t}
          />
          {!binding && (
            <span
              className="mt-2 inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] font-medium uppercase tracking-wider bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 border border-amber-200/70 dark:border-amber-800/60"
              title={t('card.unboundTooltip')}
            >
              {t('card.unbound')}
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
  title,
}: {
  children: React.ReactNode;
  tone: 'amber' | 'emerald' | 'neutral';
  title?: string;
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
      title={title}
    >
      {children}
    </span>
  );
}

function Chip({
  children,
  tone,
  title,
}: {
  children: React.ReactNode;
  tone: 'primary' | 'neutral';
  title?: string;
}) {
  const styles =
    tone === 'primary'
      ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-300 border-primary-200 dark:border-primary-800/60'
      : 'bg-[rgb(var(--surface))] border-[rgb(var(--border-subtle))] text-[rgb(var(--foreground))]';
  return (
    <span
      className={`inline-flex max-w-full items-center whitespace-nowrap rounded-md border px-1.5 py-0.5 text-[11px] font-medium ${styles}`}
      title={title}
    >
      <span className="truncate">{children}</span>
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
  const toneSpec = SECTION_TONES[tone] ?? SECTION_TONES.primary;

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
  binding,
  isNew,
  resolvedIcon,
  spaces,
  featureSets,
  machines,
  localMachineId,
  onClose,
  onSubmit,
  onDelete,
  onError,
  onIconChange,
  t,
}: {
  entry: Entry | null;
  binding: WorkspaceBinding | null;
  isNew: boolean;
  resolvedIcon: string | null;
  spaces: Space[];
  featureSets: FeatureSet[];
  machines: Machine[];
  localMachineId: string | null;
  onClose: () => void;
  onSubmit: (input: WorkspaceBindingInput) => Promise<void>;
  onDelete: () => Promise<void>;
  onError: (msg: string) => void;
  /** Live icon edits from the binding form (before autosave lands in entry state). */
  onIconChange?: (icon: string | null) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const [editedIcon, setEditedIcon] = useState<string | null | undefined>(undefined);
  const [prevResolvedIcon, setPrevResolvedIcon] = useState(resolvedIcon);

  if (resolvedIcon !== prevResolvedIcon) {
    setPrevResolvedIcon(resolvedIcon);
    setEditedIcon(undefined);
  }

  const liveIcon = editedIcon !== undefined ? editedIcon : resolvedIcon;

  const handleIconChange = useCallback(
    (icon: string | null) => {
      setEditedIcon(icon);
      onIconChange?.(icon);
    },
    [onIconChange]
  );
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const isMapped = binding !== null;
  const mode: 'create' | 'edit' | 'create-from-live' = isNew
    ? 'create'
    : isMapped
      ? 'edit'
      : 'create-from-live';
  const title = isNew
    ? t('panel.newBinding')
    : isMapped
      ? t('panel.binding')
      : t('panel.configureWorkspace');
  const displayTitle = entry ? entryDisplayTitle(entry) : '';
  const subtitle = isNew
    ? t('panel.newSubtitle')
    : displayTitle !== entry?.root
      ? entry?.root ?? ''
      : displayTitle;

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
              {liveIcon ? (
                <ServerIcon icon={liveIcon} className="h-6 w-6 object-contain" fallback="📁" />
              ) : (
                <FolderOpen className="h-5 w-5 text-[rgb(var(--muted))]" />
              )}
            </div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-0.5 flex-wrap">
                {!isNew && entry?.isLive && <Pill tone="emerald">{t('card.live')}</Pill>}
                {!isNew && entry && !isMapped && <Pill tone="amber">{t('card.unmapped')}</Pill>}
                {!isNew && entry && isMapped && !entry.isLive && <Pill tone="neutral">{t('card.offline')}</Pill>}
              </div>
              <h2 className="text-lg font-bold break-words">
                {!isNew && entry ? displayTitle : title}
              </h2>
              {!isNew && entry && displayTitle !== entry.root && (
                <p className="text-xs text-[rgb(var(--muted))] break-all font-mono">{entry.root}</p>
              )}
              {isNew && <p className="text-xs text-[rgb(var(--muted))] break-words">{subtitle}</p>}
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
            aria-label={t('panel.closeAria')}
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6 space-y-5">
        <CollapsibleSection
          icon={<FolderOpen className="h-5 w-5" />}
          tone="primary"
          title={t('panel.mapping')}
          subtitle={
            mode === 'create'
              ? t('panel.mappingSubtitleCreate')
              : mode === 'create-from-live'
                ? t('panel.mappingSubtitleLive')
                : isMapped && binding
                  ? t('panel.mappingSubtitleRoutes', {
                      featureSets:
                        formatFsList(
                          binding.feature_set_ids.map(
                            (id) => featureSets.find((f) => f.id === id)?.name ?? id
                          )
                        ) || '—',
                      space:
                        spaces.find((s) => s.id === binding.space_id)?.name ?? '—',
                    })
                  : t('panel.mappingSubtitleAutosave')
          }
          defaultOpen={isNew || !isMapped}
          headerExtra={mode === 'edit' ? <SaveStatusPill status={saveStatus} t={t} /> : null}
          testId="workspace-mapping-section"
        >
          <BindingForm
            mode={mode}
            spaces={spaces}
            featureSets={featureSets}
            machines={machines}
            localMachineId={localMachineId}
            initial={binding}
            prefillRoot={entry && !isMapped ? entry.root : undefined}
            initialUnmappedIcon={!isMapped ? resolvedIcon : null}
            onCancel={onClose}
            onSubmit={onSubmit}
            onError={onError}
            onSaveStatusChange={setSaveStatus}
            onIconChange={handleIconChange}
            t={t}
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
            title={t('panel.effectiveFeatures')}
            subtitle={t('panel.effectiveFeaturesSubtitle')}
            defaultOpen={true}
            badge={effectiveTotal ?? undefined}
            testId="workspace-effective-features-section"
          >
            <EffectiveFeaturesContent
              root={entry.root}
              onTotalChange={setEffectiveTotal}
              t={t}
            />
          </CollapsibleSection>
        )}

      </div>

      {binding && (
        <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void onDelete()}
            className="w-full text-red-600 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
            data-testid={`workspace-binding-delete-${binding.id}`}
          >
            <Trash2 className="h-4 w-4 mr-2" />
            {t('actions.removeBinding')}
          </Button>
        </div>
      )}
    </div>
  );
}

function SaveStatusPill({
  status,
  t,
}: {
  status: SaveStatus;
  t: TFunction<['workspaces', 'common']>;
}) {
  if (status.kind === 'idle') return null;
  const base =
    'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider border';
  if (status.kind === 'saving') {
    return (
      <span
        className={`${base} bg-[rgb(var(--surface-dim))] text-[rgb(var(--muted))] border-[rgb(var(--border))]`}
      >
        <Loader2 className="h-2.5 w-2.5 animate-spin" />
        {t('saveStatus.saving')}
      </span>
    );
  }
  if (status.kind === 'saved') {
    return (
      <span
        className={`${base} bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 border-green-300/70 dark:border-green-700/70 animate-in fade-in duration-200`}
      >
        <Check className="h-2.5 w-2.5" strokeWidth={2.5} />
        {t('saveStatus.saved')}
      </span>
    );
  }
  return (
    <span
      className={`${base} bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-400 border-red-200 dark:border-red-800`}
      title={status.message}
    >
      <AlertCircle className="h-2.5 w-2.5" />
      {t('saveStatus.error')}
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
  t,
}: {
  root: string;
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

  const reloadEffectiveFeatures = useCallback(() => {
    void getWorkspaceEffectiveFeatures(root)
      .then((d) => {
        setData(d);
        onTotalChange?.(d.tools.length + d.prompts.length + d.resources.length);
      })
      .catch(() => {
        /* ignore — initial load already surfaced any error */
      });
  }, [root, onTotalChange]);

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
// Binding form
// ---------------------------------------------------------------------------

function BindingForm({
  mode,
  spaces,
  featureSets,
  machines,
  localMachineId,
  initial,
  prefillRoot,
  initialUnmappedIcon,
  onCancel,
  onSubmit,
  onError,
  onSaveStatusChange,
  onIconChange,
  t,
}: {
  mode: 'create' | 'edit' | 'create-from-live';
  spaces: Space[];
  featureSets: FeatureSet[];
  machines: Machine[];
  localMachineId: string | null;
  initial?: WorkspaceBinding | null;
  prefillRoot?: string;
  initialUnmappedIcon?: string | null;
  onCancel: () => void;
  onSubmit: (input: WorkspaceBindingInput) => Promise<void>;
  onError: (message: string) => void;
  /** Surfaced upward so the section header can show a Saving / Saved pill. */
  onSaveStatusChange?: (status: SaveStatus) => void;
  /** Propagate icon edits to the inspector header and card list. */
  onIconChange?: (icon: string | null) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const defaultSpaceId = useMemo(
    () => spaces.find((s) => s.is_default)?.id ?? spaces[0]?.id ?? '',
    [spaces]
  );

  const rootRef = useRef<HTMLInputElement | null>(null);
  const [root, setRoot] = useState(initial?.workspace_root ?? prefillRoot ?? '');
  const [label, setLabel] = useState(initial?.label ?? '');
  const [icon, setIcon] = useState(initial?.icon ?? initialUnmappedIcon ?? '');
  const [spaceId, setSpaceId] = useState<string>(initial?.space_id ?? defaultSpaceId);
  // Multi-FS: a binding may resolve to N FeatureSets (the resolver merges
  // their members into one allow set). Order is preserved so the operator
  // can rank a "primary" FS first; the resolver itself doesn't care.
  const [fsIds, setFsIds] = useState<string[]>(initial?.feature_set_ids ?? []);
  const [machineId, setMachineId] = useState<string>(
    initial?.machine_id ??
      (mode === 'create' || mode === 'create-from-live' ? (localMachineId ?? '') : '')
  );
  const [fsSearch, setFsSearch] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [iconFilePath, setIconFilePath] = useState('');
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

  const machineOptions = useMemo(
    () => machines.map((m) => ({ value: m.id, label: m.name, icon: m.icon ?? undefined })),
    [machines]
  );

  const bindingMachineId = (value: string): string | null => (value.trim() ? value : null);

  const handleSubmit = async () => {
    if (!root.trim()) {
      onError(t('form.errors.rootRequired'));
      return;
    }
    if (rootValidation.state === 'error') {
      onError(rootValidation.reason);
      return;
    }
    if (!spaceId) {
      onError(t('form.errors.pickSpace'));
      return;
    }
    if (fsIds.length === 0) {
      onError(t('form.errors.pickFeatureSet'));
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit({
        workspace_root: root.trim(),
        label: label.trim() || null,
        icon: icon.trim() || null,
        space_id: spaceId,
        feature_set_ids: fsIds,
        machine_id: bindingMachineId(machineId),
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
      label: label.trim() || null,
      icon: icon.trim() || null,
      space_id: spaceId,
      feature_set_ids: fsIds,
      machine_id: bindingMachineId(machineId),
    };

    // Dedupe baseline: last-saved if we've saved during this session,
    // otherwise the initial payload from when the panel opened.
    const baseline = lastSavedRef.current ?? {
      workspace_root: initial.workspace_root,
      label: initial.label,
      icon: initial.icon,
      space_id: initial.space_id,
      feature_set_ids: initial.feature_set_ids,
      machine_id: initial.machine_id,
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
    label,
    icon,
    spaceId,
    fsIds,
    machineId,
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
    mode === 'create-from-live' ? t('form.saveBinding') : t('form.createBinding');

  const lastSavedAppearanceRef = useRef<string | null>(
    mode === 'create-from-live' ? normalizeIcon(initialUnmappedIcon) : null
  );

  /**
   * Persist icon immediately after upload so the card updates without waiting
   * for the debounced autosave or a panel close flush.
   */
  const persistIconNow = async (nextIcon: string) => {
    const workspaceRoot = root.trim();
    if (!workspaceRoot) return;
    const normalizedIcon = normalizeIcon(nextIcon);

    if (mode === 'edit' && initial && canSubmit) {
      const payload: WorkspaceBindingInput = {
        workspace_root: workspaceRoot,
        label: label.trim() || null,
        icon: normalizedIcon,
        space_id: spaceId,
        feature_set_ids: fsIds,
        machine_id: bindingMachineId(machineId),
      };
      lastSavedRef.current = payload;
      pendingPayloadRef.current = null;
      await onSubmit(payload);
      return;
    }

    if (mode === 'create-from-live') {
      if (normalizedIcon) {
        await upsertWorkspaceAppearance({
          workspace_root: workspaceRoot,
          icon: normalizedIcon,
        });
        lastSavedAppearanceRef.current = normalizedIcon;
      } else {
        await deleteWorkspaceAppearance(workspaceRoot);
        lastSavedAppearanceRef.current = null;
      }
    }
  };

  useEffect(() => {
    if (mode !== 'create-from-live') return;
    const workspaceRoot = root.trim();
    if (!workspaceRoot) return;
    const normalizedIcon = normalizeIcon(icon);
    const baseline = lastSavedAppearanceRef.current;
    if (normalizedIcon === baseline) return;

    const handle = setTimeout(() => {
      void (async () => {
        try {
          if (normalizedIcon) {
            await upsertWorkspaceAppearance({
              workspace_root: workspaceRoot,
              icon: normalizedIcon,
            });
          } else {
            await deleteWorkspaceAppearance(workspaceRoot);
          }
          lastSavedAppearanceRef.current = normalizedIcon;
        } catch (e) {
          onError(e instanceof Error ? e.message : String(e));
        }
      })();
    }, 600);
    return () => clearTimeout(handle);
  }, [mode, root, icon, onError]);

  return (
    <div className="space-y-5">
      <FormField label={t('form.label')} hint={t('form.labelHint')}>
        <input
          type="text"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder={t('form.labelPlaceholder')}
          className="w-full px-3 py-2 rounded-lg text-sm bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
          data-testid="workspace-binding-label-input"
        />
      </FormField>

      <FormField label={t('form.icon')} hint={t('form.iconHint')}>
        <div className="space-y-2.5">
          <div className="flex items-start gap-3">
            <div className="w-14 h-14 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] flex items-center justify-center flex-shrink-0">
              {icon.trim() ? (
                <ServerIcon icon={icon.trim()} className="h-9 w-9 object-contain" fallback="📁" />
              ) : (
                <FolderOpen className="h-6 w-6 text-[rgb(var(--muted))]" />
              )}
            </div>
            <div className="flex-1 min-w-0 space-y-2">
              <input
                type="text"
                value={icon}
                onChange={(e) => {
                  const next = e.target.value;
                  setIcon(next);
                  onIconChange?.(normalizeIcon(next));
                }}
                placeholder={t('form.iconPlaceholder')}
                className="w-full px-3 py-2 rounded-lg text-sm bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="workspace-binding-icon-input"
              />
              <div className="flex items-center gap-2 flex-wrap">
                {isTauri() ? (
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={async () => {
                      try {
                        const picked = await pickPath({
                          directory: false,
                          multiple: false,
                          title: t('form.pickIconTitle'),
                          filters: [
                            {
                              name: t('form.imagesFilter'),
                              extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
                            },
                          ],
                        });
                        if (typeof picked !== 'string' || picked.length === 0) return;
                        const localRef = await uploadWorkspaceIcon(picked);
                        setIcon(localRef);
                        onIconChange?.(localRef);
                        await persistIconNow(localRef);
                      } catch (e) {
                        onError(e instanceof Error ? e.message : String(e));
                      }
                    }}
                    data-testid="workspace-binding-icon-upload"
                  >
                    {t('form.upload')}
                  </Button>
                ) : (
                  <>
                    <input
                      type="text"
                      value={iconFilePath}
                      onChange={(e) => setIconFilePath(e.target.value)}
                      placeholder="Enter absolute path"
                      className="min-w-0 flex-1 px-3 py-2 rounded-lg text-sm bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                      data-testid="workspace-binding-icon-path-input"
                    />
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={!iconFilePath.trim()}
                      onClick={async () => {
                        const picked = iconFilePath.trim();
                        if (!picked) return;
                        try {
                          const localRef = await uploadWorkspaceIcon(picked);
                          setIcon(localRef);
                          onIconChange?.(localRef);
                          await persistIconNow(localRef);
                          setIconFilePath('');
                        } catch (e) {
                          onError(e instanceof Error ? e.message : String(e));
                        }
                      }}
                      data-testid="workspace-binding-icon-upload"
                    >
                      {t('form.upload')}
                    </Button>
                  </>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setIcon('');
                    onIconChange?.(null);
                    void persistIconNow('').catch((e) =>
                      onError(e instanceof Error ? e.message : String(e))
                    );
                  }}
                  disabled={!icon.trim()}
                  data-testid="workspace-binding-icon-clear"
                >
                  {t('form.clear')}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </FormField>

      <FormField label={t('form.machine')} hint={t('form.machineHint')}>
        <Picker
          value={machineId}
          onChange={setMachineId}
          options={machineOptions}
          placeholder={t('form.noMachine')}
          testId="workspace-binding-machine-select"
        />
      </FormField>

      <FormField label={t('form.workspaceRoot')}>
        <div className="flex gap-2">
          <input
            ref={rootRef}
            type="text"
            value={root}
            onChange={(e) => setRoot(e.target.value)}
            readOnly={!rootEditable}
            placeholder={t('form.rootPlaceholder')}
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
          {rootEditable && isTauri() && (
            <button
              type="button"
              onClick={async () => {
                try {
                  const picked = await pickPath({
                    directory: true,
                    multiple: false,
                    title: t('form.pickFolderTitle'),
                  });
                  if (typeof picked === 'string' && picked.length > 0) {
                    setRoot(picked);
                  }
                } catch (e) {
                  onError(e instanceof Error ? e.message : String(e));
                }
              }}
              className="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] hover:bg-[rgb(var(--surface-hover))] text-sm font-medium text-[rgb(var(--foreground))] transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 flex-shrink-0"
              title={t('form.pickFolder')}
              data-testid="workspace-binding-browse"
            >
              <FolderSearch className="h-4 w-4" />
              <span className="hidden sm:inline">{t('form.browse')}</span>
            </button>
          )}
        </div>
        <RootValidationHint state={rootValidation} editable={rootEditable} originalValue={root} t={t} />
      </FormField>

      <FormField label={t('form.space')} hint={t('form.spaceHint')}>
        <Picker
          value={spaceId}
          onChange={setSpaceId}
          placeholder={t('form.pickSpace')}
          options={spaces.map((s) => ({
            value: s.id,
            label: s.is_default ? `${s.name}${t('form.defaultSuffix')}` : s.name,
            icon: s.icon ?? undefined,
          }))}
          testId="workspace-binding-space"
        />
      </FormField>

      <FormField
        label={
          fsIds.length > 1
            ? t('form.featureSetsSelected', { count: fsIds.length })
            : t('form.featureSet')
        }
        hint={t('form.featureSetHint')}
      >
        {!spaceId ? (
          <p className="text-xs text-[rgb(var(--muted))] italic px-3 py-2">
            {t('form.pickSpaceFirst')}
          </p>
        ) : availableFs.length === 0 ? (
          <p className="text-xs text-[rgb(var(--muted))] italic px-3 py-2">
            {t('form.noFeatureSets')}
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
                placeholder={t('form.searchFeatureSets', { count: availableFs.length })}
                className="w-full px-2.5 py-1.5 text-xs bg-[rgb(var(--surface))] border border-[rgb(var(--border-subtle))] rounded focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="workspace-binding-fs-search"
              />
            </div>
            <div className="max-h-56 overflow-y-auto p-1.5 space-y-1">
              {filteredFs.length === 0 ? (
                <p className="text-xs text-[rgb(var(--muted))] italic px-2 py-3 text-center">
                  {t('form.noMatch', { query: fsSearch })}
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
                              title={t('form.starterTooltip')}
                            >
                              {t('form.starter')}
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
                          title={t('form.renderOrderTooltip')}
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
                {t('form.shownCount', { shown: filteredFs.length, total: availableFs.length })}
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
            {t('common:actions.cancel')}
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
  t,
}: {
  state:
    | { state: 'idle' }
    | { state: 'checking' }
    | { state: 'ok'; normalized: string }
    | { state: 'error'; reason: string };
  editable: boolean;
  originalValue: string;
  t: TFunction<['workspaces', 'common']>;
}) {
  if (!editable) {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        {t('form.rootHint.reportedByClient')}
      </p>
    );
  }
  if (state.state === 'idle') {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        {t('form.rootHint.idle')}
      </p>
    );
  }
  if (state.state === 'checking') {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))] inline-flex items-center gap-1.5">
        <Loader2 className="h-3 w-3 animate-spin" />
        {t('form.rootHint.checking')}
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
  const changed = state.normalized !== originalValue.trim();
  if (!changed) {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
        {t('form.rootHint.ready')}
      </p>
    );
  }
  return (
    <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
      {t('form.rootHint.willSaveAs', { path: state.normalized })}
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
    const trimmedName = name.trim();
    if (!trimmedName) {
      onError(t('machineIdentity.nameRequired'));
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit({
        name: trimmedName,
        icon: icon.trim() || null,
        hostname: hostname.trim() || null,
      });
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  if (!open) return null;

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
              <input
                type="text"
                value={icon}
                onChange={(e) => setIcon(e.target.value)}
                placeholder={t('machineIdentity.iconPlaceholder')}
                className="w-full px-3 py-2 rounded-lg text-sm bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="machine-identity-icon-input"
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
                disabled={submitting || !name.trim()}
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
