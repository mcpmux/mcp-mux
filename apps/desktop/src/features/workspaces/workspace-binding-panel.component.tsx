import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ChevronDown,
  FolderOpen,
  Layers,
  Loader2,
  Monitor,
  Plus,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import { Button, useConfirm, useToast } from '@mcpmux/ui';
import { useWorkspaceEvents } from '@/lib/backend/events';
import { apiCall } from '@/lib/api/transport';
import {
  createWorkspaceBinding,
  deleteWorkspaceBinding,
  updateWorkspaceBinding,
  type WorkspaceBinding,
  type WorkspaceBindingInput,
} from '@/lib/api/workspaceBindings';
import {
  createMachine,
  getClientMachineId,
  getLocalMachineId,
  listMachines,
  setClientMachineId,
  type Machine,
} from '@/lib/api/machines';
import { listFeatureSets, type FeatureSet } from '@/lib/api/featureSets';
import { listSpaces, type Space } from '@/lib/api/spaces';
import { ServerIcon } from '@/components/ServerIcon';
import { useBindingPanelStore } from '@/stores/bindingPanelStore';
import {
  BindingForm,
  SaveStatusPill,
  type SaveStatus,
} from './workspace-binding-form.component';
import {
  CollapsibleSection,
  EffectiveFeaturesContent,
} from './WorkspacesPage';

/**
 * Render a list of FeatureSet names as a single string for panel subtitles.
 */
function formatFsList(names: string[]): string {
  return names.filter((n) => n && n.length > 0).join(' + ');
}

/** Compact status pill matching WorkspacesPage card badges. */
function Pill({
  children,
  tone,
  title,
}: {
  children: React.ReactNode;
  tone: 'amber' | 'emerald' | 'neutral' | 'primary';
  title?: string;
}) {
  const cls =
    tone === 'amber'
      ? 'bg-amber-50 dark:bg-amber-900/20 text-amber-700 dark:text-amber-400 border-amber-200/80 dark:border-amber-800/60'
      : tone === 'emerald'
        ? 'bg-emerald-50 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-400 border-emerald-200/80 dark:border-emerald-800/60'
        : tone === 'primary'
          ? 'bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-400 border-primary-200/80 dark:border-primary-800/60'
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

/**
 * Synthetic binding seed for create-from-live so BindingForm picks up event hints.
 */
function buildFormInitial(
  payload: NonNullable<ReturnType<typeof useBindingPanelStore.getState>['payload']>,
  spaces: Space[],
): WorkspaceBinding | null {
  if (payload.binding) return payload.binding;
  if (payload.mode !== 'create-from-live' || !payload.workspaceRoot) return null;
  const spaceId =
    payload.spaceId ?? spaces.find((s) => s.is_default)?.id ?? spaces[0]?.id ?? '';
  return {
    id: '',
    workspace_root: payload.workspaceRoot,
    space_id: spaceId,
    feature_set_ids: [],
    machine_id: null,
    label: null,
    icon: null,
    client_id: payload.clientId ?? null,
    created_at: '',
    updated_at: '',
  };
}

/**
 * Global workspace binding overlay — create, edit, and live-connection flows.
 */
export function WorkspaceBindingPanel() {
  const { t } = useTranslation(['workspaces', 'common']);
  const { isOpen, payload, open, close } = useBindingPanelStore();
  const { subscribe } = useWorkspaceEvents();
  const { confirm, ConfirmDialogElement } = useConfirm();
  const { error: showError } = useToast();

  const [spaces, setSpaces] = useState<Space[]>([]);
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [localMachineId, setLocalMachineId] = useState<string | null>(null);
  const [clientMachineId, setClientMachineIdState] = useState<string | null>(null);
  const [showMachineCallout, setShowMachineCallout] = useState(false);
  const [assignMachineId, setAssignMachineId] = useState('');
  const [creatingMachine, setCreatingMachine] = useState(false);
  const [newMachineName, setNewMachineName] = useState('');
  const [assigningMachine, setAssigningMachine] = useState(false);
  const [loadingData, setLoadingData] = useState(false);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ kind: 'idle' });
  const [effectiveTotal, setEffectiveTotal] = useState<number | null>(null);
  const [resolvedIcon, setResolvedIcon] = useState<string | null>(null);

  const panelOpenRef = useRef(isOpen);
  panelOpenRef.current = isOpen;

  useEffect(() => {
    return subscribe('workspace-needs-binding', async (eventPayload) => {
      if (panelOpenRef.current) return;
      try {
        const enabled = await apiCall<boolean>('get_workspace_mapping_prompt_enabled');
        if (!enabled) return;
      } catch {
        /* setting unavailable → default to showing */
      }
      if (panelOpenRef.current) return;
      open({
        mode: 'create-from-live',
        workspaceRoot: eventPayload.workspace_root,
        clientId: eventPayload.client_id,
        spaceId: eventPayload.space_id,
        collisionClientId: eventPayload.collision_client_id ?? undefined,
      });
    });
  }, [subscribe, open]);

  useEffect(() => {
    return subscribe('workspace-binding-changed', (changedPayload) => {
      const state = useBindingPanelStore.getState();
      if (!state.isOpen || !state.payload) return;
      const changed = changedPayload.workspace_root.toLowerCase();
      const payloadRoot = state.payload.workspaceRoot?.toLowerCase();
      const bindingRoot = state.payload.binding?.workspace_root.toLowerCase();
      if (changed === payloadRoot || changed === bindingRoot) {
        close();
      }
    });
  }, [subscribe, close]);

  useEffect(() => {
    if (!isOpen || !payload) return;
    let cancelled = false;
    setLoadingData(true);
    setSaveStatus({ kind: 'idle' });
    setEffectiveTotal(null);
    setResolvedIcon(null);
    setCreatingMachine(false);
    setNewMachineName('');

    void (async () => {
      try {
        const [loadedSpaces, loadedFs, loadedMachines, loadedLocalId] = await Promise.all([
          listSpaces(),
          listFeatureSets(),
          listMachines().catch(() => [] as Machine[]),
          getLocalMachineId().catch(() => null),
        ]);
        if (cancelled) return;
        setSpaces(loadedSpaces);
        setFeatureSets(loadedFs);
        setMachines(loadedMachines);
        setLocalMachineId(loadedLocalId);

        if (payload.mode === 'create-from-live' && payload.clientId) {
          const existingClientMachine = await getClientMachineId(payload.clientId).catch(
            () => null,
          );
          if (cancelled) return;
          setClientMachineIdState(existingClientMachine);
          setShowMachineCallout(existingClientMachine == null);
          setAssignMachineId(existingClientMachine ?? loadedLocalId ?? '');
        } else {
          setClientMachineIdState(null);
          setShowMachineCallout(false);
          setAssignMachineId('');
        }
      } catch (e) {
        if (!cancelled) {
          showError(t('toast.couldNotSave'), e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setLoadingData(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [isOpen, payload, showError, t]);

  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen, close]);

  const formInitial = useMemo(
    () => (payload ? buildFormInitial(payload, spaces) : null),
    [payload, spaces],
  );

  const effectiveMachineId = clientMachineId ?? localMachineId;
  const workspaceRoot =
    payload?.binding?.workspace_root ?? payload?.workspaceRoot ?? formInitial?.workspace_root;
  const mode = payload?.mode ?? 'create';
  const isEdit = mode === 'edit';

  const handleSubmit = useCallback(
    async (input: WorkspaceBindingInput) => {
      if (!payload) return;
      if (isEdit && payload.binding) {
        await updateWorkspaceBinding(payload.binding.id, input);
        return;
      }
      await createWorkspaceBinding(input);
      // Auto-link the client to the machine when a machine is chosen —
      // the user picking a machine for the binding is the same intent as saying
      // "this Cursor is running on that machine." Override any prior assignment
      // so the resolver's client-machine lookup stays in sync with the binding.
      if (payload.clientId && input.machine_id && clientMachineId !== input.machine_id) {
        await setClientMachineId(payload.clientId, input.machine_id).catch(() => undefined);
        setClientMachineIdState(input.machine_id);
      }
      close();
    },
    [payload, isEdit, clientMachineId, close],
  );

  const handleDelete = useCallback(async () => {
    if (!payload?.binding) return;
    const binding = payload.binding;
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
      close();
    } catch (e) {
      showError(t('toast.failedToRemove'), e instanceof Error ? e.message : String(e));
    }
  }, [payload, confirm, close, showError, t]);

  const handleAssignMachine = async () => {
    if (!payload?.clientId || !assignMachineId || assigningMachine) return;
    setAssigningMachine(true);
    try {
      await setClientMachineId(payload.clientId, assignMachineId);
      setClientMachineIdState(assignMachineId);
      setShowMachineCallout(false);
    } catch (e) {
      showError(t('toast.couldNotSave'), e instanceof Error ? e.message : String(e));
    } finally {
      setAssigningMachine(false);
    }
  };

  const handleCreateMachine = async () => {
    const name = newMachineName.trim();
    if (!name) {
      showError(t('sheet.machineNameRequired'));
      return;
    }
    setAssigningMachine(true);
    try {
      const created = await createMachine({ name });
      setMachines((prev) => [...prev, created].sort((a, b) => a.name.localeCompare(b.name)));
      setAssignMachineId(created.id);
      setCreatingMachine(false);
      setNewMachineName('');
    } catch (e) {
      showError(t('toast.couldNotSave'), e instanceof Error ? e.message : String(e));
    } finally {
      setAssigningMachine(false);
    }
  };

  const handleDisablePrompt = async () => {
    try {
      await apiCall('set_workspace_mapping_prompt_enabled', { enabled: false });
      close();
    } catch (e) {
      showError(t('toast.couldNotSave'), e instanceof Error ? e.message : String(e));
    }
  };

  if (!isOpen || !payload) return null;

  const title =
    mode === 'create'
      ? t('panel.newBinding')
      : mode === 'edit'
        ? t('panel.binding')
        : payload.collisionClientId
          ? t('sheet.titleCollision')
          : t('sheet.titleNew');

  const displayTitle = formInitial?.label?.trim() || workspaceRoot || title;
  const binding = payload.binding ?? null;
  const showEffectiveFeatures =
    (mode === 'edit' && binding != null) || (mode === 'create-from-live' && !!workspaceRoot);

  const mappingSubtitle =
    mode === 'create'
      ? t('panel.mappingSubtitleCreate')
      : mode === 'create-from-live'
        ? t('panel.mappingSubtitleLive')
        : binding
          ? t('panel.mappingSubtitleRoutes', {
              featureSets:
                formatFsList(
                  binding.feature_set_ids.map(
                    (id) => featureSets.find((f) => f.id === id)?.name ?? id,
                  ),
                ) || '—',
              space: spaces.find((s) => s.id === binding.space_id)?.name ?? '—',
            })
          : t('panel.mappingSubtitleAutosave');

  const panelKey = `${mode}:${binding?.id ?? workspaceRoot ?? 'new'}:${spaces.length}`;

  return (
    <>
      <div
        className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-40 animate-in fade-in duration-200"
        onClick={close}
        data-testid="workspace-binding-panel-backdrop"
      />
      <div
        className="fixed right-0 top-0 bottom-0 w-full max-w-[480px] min-w-[420px] bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-50"
        data-testid="workspace-binding-panel"
      >
        <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
          <div className="flex items-start justify-between">
            <div className="flex items-center gap-3 flex-1 min-w-0">
              <div className="w-11 h-11 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg flex-shrink-0 border border-[rgb(var(--border-subtle))]">
                {resolvedIcon ? (
                  <ServerIcon icon={resolvedIcon} className="h-6 w-6 object-contain" fallback="📁" />
                ) : (
                  <FolderOpen className="h-5 w-5 text-[rgb(var(--muted))]" />
                )}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-0.5 flex-wrap">
                  {mode === 'create-from-live' && (
                    <Pill tone={payload.collisionClientId ? 'amber' : 'primary'}>
                      <span className="inline-flex items-center gap-1">
                        <Sparkles className="h-2.5 w-2.5" />
                        {payload.collisionClientId ? t('sheet.badgeCollision') : t('sheet.badgeNew')}
                      </span>
                    </Pill>
                  )}
                  {mode === 'create-from-live' && !payload.collisionClientId && (
                    <Pill tone="amber">{t('card.unmapped')}</Pill>
                  )}
                  {mode === 'edit' && binding && <Pill tone="neutral">{t('card.offline')}</Pill>}
                </div>
                <h2 className="text-lg font-bold break-words">
                  {mode === 'create' ? title : displayTitle}
                </h2>
                {mode === 'create-from-live' && payload.collisionClientId && (
                  <p className="text-xs text-[rgb(var(--muted))] mt-1">{t('sheet.descCollision')}</p>
                )}
                {mode === 'create-from-live' && !payload.collisionClientId && (
                  <p className="text-xs text-[rgb(var(--muted))] mt-1">{t('sheet.descNew')}</p>
                )}
                {mode !== 'create' && workspaceRoot && displayTitle !== workspaceRoot && (
                  <p className="text-xs text-[rgb(var(--muted))] break-all font-mono mt-0.5">
                    {workspaceRoot}
                  </p>
                )}
                {mode === 'create' && (
                  <p className="text-xs text-[rgb(var(--muted))] break-words mt-0.5">
                    {t('panel.newSubtitle')}
                  </p>
                )}
              </div>
            </div>
            <button
              type="button"
              onClick={close}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
              aria-label={t('panel.closeAria')}
            >
              <X className="h-5 w-5" />
            </button>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-6 space-y-5">
          {loadingData ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            </div>
          ) : (
            <>
              {mode === 'create-from-live' && payload.clientId && showMachineCallout && (
                <div
                  className="rounded-xl border-2 border-amber-300/80 dark:border-amber-700/60 bg-amber-50/80 dark:bg-amber-900/15 p-4 space-y-3"
                  data-testid="workspace-binding-machine-callout"
                >
                  <div className="flex items-start gap-2">
                    <Monitor className="h-4 w-4 text-amber-600 dark:text-amber-400 mt-0.5 flex-shrink-0" />
                    <div className="min-w-0 flex-1">
                      <p className="text-sm font-semibold text-amber-900 dark:text-amber-100">
                        {t('sheet.badgeNew')}
                      </p>
                      <p className="text-xs text-amber-800/90 dark:text-amber-200/90 mt-1">
                        {t('sheet.machineDesc')}
                      </p>
                    </div>
                  </div>
                  <div className="relative">
                    <select
                      value={assignMachineId}
                      onChange={(e) => setAssignMachineId(e.target.value)}
                      className="w-full appearance-none px-3 py-2 pr-9 bg-[rgb(var(--background))] border border-[rgb(var(--border))] rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-primary-500"
                      data-testid="workspace-binding-callout-machine-picker"
                    >
                      <option value="">{t('form.noMachine')}</option>
                      {machines.map((m) => (
                        <option key={m.id} value={m.id}>
                          {m.icon ? `${m.icon}  ` : ''}
                          {m.name}
                        </option>
                      ))}
                    </select>
                    <ChevronDown className="absolute right-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))] pointer-events-none" />
                  </div>
                  {!creatingMachine ? (
                    <button
                      type="button"
                      onClick={() => setCreatingMachine(true)}
                      className="flex w-full items-center gap-2 rounded-xl border border-dashed border-amber-300/70 dark:border-amber-700/50 px-3 py-2 text-sm text-amber-800 dark:text-amber-200 transition-colors hover:border-amber-400"
                    >
                      <Plus className="h-4 w-4" />
                      {t('sheet.newMachine')}
                    </button>
                  ) : (
                    <div className="rounded-xl border border-amber-200/80 dark:border-amber-800/50 p-3 space-y-2">
                      <input
                        type="text"
                        value={newMachineName}
                        onChange={(e) => setNewMachineName(e.target.value)}
                        placeholder={t('sheet.machineNamePlaceholder')}
                        className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 text-sm"
                        autoFocus
                      />
                      <div className="flex gap-2">
                        <Button
                          variant="secondary"
                          size="sm"
                          className="flex-1"
                          onClick={() => {
                            setCreatingMachine(false);
                            setNewMachineName('');
                          }}
                          disabled={assigningMachine}
                        >
                          {t('sheet.notNow')}
                        </Button>
                        <Button
                          variant="primary"
                          size="sm"
                          className="flex-1"
                          onClick={() => void handleCreateMachine()}
                          disabled={assigningMachine}
                        >
                          {assigningMachine ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : (
                            t('sheet.continue')
                          )}
                        </Button>
                      </div>
                    </div>
                  )}
                  <Button
                    variant="primary"
                    size="sm"
                    className="w-full"
                    onClick={() => void handleAssignMachine()}
                    disabled={assigningMachine || !assignMachineId}
                    data-testid="workspace-binding-callout-assign"
                  >
                    {assigningMachine ? (
                      <Loader2 className="h-4 w-4 animate-spin mr-1.5" />
                    ) : null}
                    {t('sheet.continue')}
                  </Button>
                </div>
              )}

              <CollapsibleSection
                icon={<FolderOpen className="h-5 w-5" />}
                tone="primary"
                title={t('panel.mapping')}
                subtitle={mappingSubtitle}
                defaultOpen={mode !== 'edit'}
                headerExtra={isEdit ? <SaveStatusPill status={saveStatus} t={t} /> : null}
                testId="workspace-mapping-section"
              >
                <BindingForm
                  key={panelKey}
                  mode={mode}
                  spaces={spaces}
                  featureSets={featureSets}
                  machines={machines}
                  localMachineId={effectiveMachineId}
                  initial={formInitial}
                  prefillRoot={payload.workspaceRoot}
                  clientId={payload.clientId}
                  onCancel={close}
                  onSubmit={handleSubmit}
                  onError={(msg) => showError(t('toast.couldNotSave'), msg)}
                  onSaveStatusChange={setSaveStatus}
                  onIconChange={setResolvedIcon}
                  t={t}
                />
              </CollapsibleSection>

              {showEffectiveFeatures && workspaceRoot && (
                <CollapsibleSection
                  icon={<Layers className="h-5 w-5" />}
                  tone="purple"
                  title={t('panel.effectiveFeatures')}
                  subtitle={t('panel.effectiveFeaturesSubtitle')}
                  defaultOpen
                  badge={effectiveTotal ?? undefined}
                  testId="workspace-effective-features-section"
                >
                  <EffectiveFeaturesContent
                    root={workspaceRoot}
                    onTotalChange={setEffectiveTotal}
                    t={t}
                  />
                </CollapsibleSection>
              )}

              {mode === 'create-from-live' && (
                <div className="text-center pt-1">
                  <button
                    type="button"
                    onClick={() => void handleDisablePrompt()}
                    title={t('sheet.disablePromptTitle')}
                    className="text-[11px] text-[rgb(var(--muted))] underline-offset-2 transition-colors hover:text-[rgb(var(--foreground))] hover:underline"
                    data-testid="workspace-binding-disable-prompt"
                  >
                    {t('sheet.disablePrompt')}
                  </button>
                </div>
              )}
            </>
          )}
        </div>

        {isEdit && binding && (
          <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void handleDelete()}
              className="w-full text-red-600 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
              data-testid={`workspace-binding-delete-${binding.id}`}
            >
              <Trash2 className="h-4 w-4 mr-2" />
              {t('actions.removeBinding')}
            </Button>
          </div>
        )}
      </div>
      {ConfirmDialogElement}
    </>
  );
}
