import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import {
  ChevronDown,
  Check,
  FolderOpen,
  Layers,
  Loader2,
  Monitor,
  Plus,
  Route,
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
  dismissWorkspaceBindingPrompt,
  listWorkspaceBindings,
  updateWorkspaceBinding,
  validateWorkspaceRoot,
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
import {
  deleteWorkspaceAppearance,
  listWorkspaceAppearances,
  upsertWorkspaceAppearance,
} from '@/lib/api/workspaceAppearances';
import { listFeatureSets, type FeatureSet } from '@/lib/api/featureSets';
import { listSpaces, type Space } from '@/lib/api/spaces';
import { ServerIcon } from '@/components/ServerIcon';
import { useBindingPanelStore } from '@/stores/bindingPanelStore';
import { useViewerIdentity } from '@/hooks/use-viewer-identity.hook';
import { RoutingFields, SaveStatusPill, ScopeFields } from './workspace-binding-form.component';
import {
  bindingMachineId,
  bindingScopeConflicts,
  buildBindingPayload,
  adoptBindingSeed,
  findAdoptableSiblingBindings,
  folderName,
  normalizeIcon,
  sameBindingInput,
  type RootValidationState,
  type SaveStatus,
} from './workspace-binding-form.helpers';
import {
  CollapsibleSection,
  EffectiveFeaturesContent,
  type CollapsibleSectionRef,
} from './WorkspacesPage';

/**
 * Render a list of FeatureSet names as a single string for panel subtitles.
 */
function formatFsList(names: string[]): string {
  return names.filter((n) => n && n.length > 0).join(' + ');
}

/**
 * Resolve machine badge label for header and Scope subtitle.
 */
function resolveMachineBadgeLabel(
  mode: 'create' | 'edit' | 'create-from-live',
  machineId: string,
  machineIds: string[],
  machinesById: Map<string, Machine>,
  t: TFunction<['workspaces', 'common']>,
): string {
  if (mode === 'edit') {
    return machineId && machinesById.has(machineId)
      ? machinesById.get(machineId)!.name
      : t('panel.machineGlobal');
  }
  if (machineIds.length === 0) return t('panel.machineGlobal');
  if (machineIds.length === 1) {
    const machine = machinesById.get(machineIds[0]);
    return machine?.name ?? t('panel.machineGlobal');
  }
  return t('panel.machineCount', { count: machineIds.length });
}

/**
 * Resolve machine badge icon for the header pill.
 */
function resolveMachineBadgeIcon(
  mode: 'create' | 'edit' | 'create-from-live',
  machineId: string,
  machineIds: string[],
  machinesById: Map<string, Machine>,
): string | null {
  if (mode === 'edit') {
    return machineId ? (machinesById.get(machineId)?.icon ?? null) : null;
  }
  if (machineIds.length === 1) {
    return machinesById.get(machineIds[0])?.icon ?? null;
  }
  return null;
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
 * Workspace identity in the panel header: display icon, title, path, and machine badge.
 */
function PanelIdentityHeader({
  mode,
  label,
  icon,
  workspaceRoot,
  machineBadgeLabel,
  machineBadgeIcon,
  onMachineBadgeClick,
  badges,
  footer,
  t,
}: {
  mode: 'create' | 'edit' | 'create-from-live';
  label: string;
  icon: string;
  workspaceRoot?: string;
  machineBadgeLabel: string;
  machineBadgeIcon: string | null;
  onMachineBadgeClick: () => void;
  badges?: ReactNode;
  footer?: ReactNode;
  t: TFunction<['workspaces', 'common']>;
}) {
  const trimmedIcon = icon.trim();
  const displayTitle =
    label.trim() ||
    (workspaceRoot
      ? (workspaceRoot.split(/[/\\]/).filter(Boolean).pop() ?? workspaceRoot)
      : mode === 'create'
        ? t('panel.identityPlaceholder')
        : '');

  return (
    <div className="flex items-start gap-3 flex-1 min-w-0">
      <div
        className="w-11 h-11 flex-shrink-0 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg border border-[rgb(var(--border-subtle))]"
        data-testid="workspace-binding-header-icon"
      >
        {trimmedIcon ? (
          <ServerIcon icon={trimmedIcon} className="h-6 w-6 object-contain" fallback="📁" />
        ) : (
          <FolderOpen className="h-5 w-5 text-[rgb(var(--muted))]" />
        )}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1 flex-wrap">
          <button
            type="button"
            className="flex-shrink-0 rounded-md transition-colors hover:bg-[rgb(var(--surface-hover))] focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="workspace-binding-header-machine-badge"
            title={t('panel.machineQuickSwitch')}
            onClick={onMachineBadgeClick}
          >
            <Pill tone="neutral">
              <span className="inline-flex items-center gap-1 normal-case tracking-normal">
                {machineBadgeIcon ? (
                  <span className="text-xs leading-none">{machineBadgeIcon}</span>
                ) : null}
                {machineBadgeLabel}
              </span>
            </Pill>
          </button>
          {badges}
        </div>
        <p
          className="w-full text-lg font-bold text-[rgb(var(--foreground))] break-words"
          data-testid="workspace-binding-header-title"
        >
          {displayTitle}
        </p>
        {workspaceRoot ? (
          <p className="text-xs text-[rgb(var(--muted))] break-all font-mono mt-0.5">{workspaceRoot}</p>
        ) : mode === 'create' ? (
          <p className="text-xs text-[rgb(var(--muted))] break-words mt-0.5">{t('panel.newSubtitle')}</p>
        ) : null}
        {footer}
      </div>
    </div>
  );
}

/**
 * Synthetic binding seed for create-from-live panel state initialization.
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
    icon: payload.appearanceIcon ?? null,
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
  const { machineId: viewerMachineId } = useViewerIdentity();
  const { subscribe } = useWorkspaceEvents();
  const { confirm, ConfirmDialogElement } = useConfirm();
  const { error: showError } = useToast();

  const [spaces, setSpaces] = useState<Space[]>([]);
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  const [machines, setMachines] = useState<Machine[]>([]);
  const [allBindings, setAllBindings] = useState<WorkspaceBinding[]>([]);
  const [localMachineId, setLocalMachineId] = useState<string | null>(null);
  const [clientMachineId, setClientMachineIdState] = useState<string | null>(null);
  const [showMachineCallout, setShowMachineCallout] = useState(false);
  const [adoptDismissed, setAdoptDismissed] = useState(false);
  const [appearanceIcon, setAppearanceIcon] = useState<string | null>(null);
  const [assignMachineId, setAssignMachineId] = useState('');
  const [creatingMachine, setCreatingMachine] = useState(false);
  const [newMachineName, setNewMachineName] = useState('');
  const [assigningMachine, setAssigningMachine] = useState(false);
  const [loadingData, setLoadingData] = useState(false);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>({ kind: 'idle' });
  const [effectiveTotal, setEffectiveTotal] = useState<number | null>(null);
  const [label, setLabel] = useState('');
  const [icon, setIcon] = useState('');
  const [spaceId, setSpaceId] = useState('');
  const [fsIds, setFsIds] = useState<string[]>([]);
  const [machineId, setMachineId] = useState('');
  const [machineIds, setMachineIds] = useState<string[]>([]);
  const [root, setRoot] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [rootValidation, setRootValidation] = useState<RootValidationState>({ state: 'idle' });

  const validationSeq = useRef(0);
  const saveSeqRef = useRef(0);
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastSavedRef = useRef<WorkspaceBindingInput | null>(null);
  const lastSavedAppearanceRef = useRef<string | null>(null);
  const pendingPayloadRef = useRef<WorkspaceBindingInput | null>(null);
  const onSubmitRef = useRef<(input: WorkspaceBindingInput) => Promise<void>>(async () => undefined);
  const onSaveStatusChangeRef = useRef(setSaveStatus);

  const panelOpenRef = useRef(isOpen);
  const scopeSectionRef = useRef<CollapsibleSectionRef>(null);
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
        spaceLocked: eventPayload.space_locked ?? false,
      });
    });
  }, [subscribe, open]);

  useEffect(() => {
    if (!isOpen || !payload) return;
    let cancelled = false;
    setLoadingData(true);
    setSaveStatus({ kind: 'idle' });
    setEffectiveTotal(null);
    setCreatingMachine(false);
    setNewMachineName('');
    setAdoptDismissed(false);
    setAppearanceIcon(null);

    void (async () => {
      try {
        const [loadedSpaces, loadedFs, loadedMachines, loadedBindings, loadedAppearances, loadedLocalId] =
          await Promise.all([
          listSpaces(),
          listFeatureSets(),
          listMachines().catch(() => [] as Machine[]),
          listWorkspaceBindings().catch(() => [] as WorkspaceBinding[]),
          listWorkspaceAppearances().catch(() => []),
          getLocalMachineId().catch(() => null),
        ]);
        if (cancelled) return;
        setSpaces(loadedSpaces);
        setFeatureSets(loadedFs);
        setMachines(loadedMachines);
        setAllBindings(loadedBindings);
        setLocalMachineId(loadedLocalId);

        const rootForAppearance = payload.workspaceRoot ?? payload.binding?.workspace_root;
        if (rootForAppearance) {
          const appearance = loadedAppearances.find(
            (entry) => entry.workspace_root.toLowerCase() === rootForAppearance.toLowerCase(),
          );
          setAppearanceIcon(appearance?.icon ?? null);
        }

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

  const formInitial = useMemo(
    () => (payload ? buildFormInitial(payload, spaces) : null),
    [payload, spaces],
  );

  const defaultSpaceId = useMemo(
    () => spaces.find((s) => s.is_default)?.id ?? spaces[0]?.id ?? '',
    [spaces],
  );

  const workspaceRoot =
    payload?.binding?.workspace_root ?? payload?.workspaceRoot ?? formInitial?.workspace_root;
  const mode = payload?.mode ?? 'create';
  const isEdit = mode === 'edit';
  const rootEditable = mode !== 'create-from-live';
  const spaceLocked = payload?.spaceLocked ?? false;
  const panelKey = `${mode}:${payload?.binding?.id ?? workspaceRoot ?? 'new'}:${spaces.length}:${allBindings.length}`;

  const defaultTargetMachineId =
    clientMachineId ?? viewerMachineId ?? localMachineId ?? null;

  const siblingBindings = useMemo(() => {
    if (mode !== 'create-from-live' || !workspaceRoot) return [];
    return findAdoptableSiblingBindings(allBindings, workspaceRoot, defaultTargetMachineId);
  }, [mode, workspaceRoot, allBindings, defaultTargetMachineId]);

  const adoptSource =
    mode === 'create-from-live' && !adoptDismissed ? siblingBindings[0] ?? null : null;

  const effectiveMachineId = isEdit
    ? bindingMachineId(machineId)
    : machineIds[0] ?? null;

  /** Close the panel; record a dismissal for create-from-live prompts with a client id. */
  const handlePanelClose = useCallback(() => {
    const rootToDismiss =
      payload?.workspaceRoot ?? payload?.binding?.workspace_root ?? workspaceRoot;
    if (payload?.mode === 'create-from-live' && payload.clientId && rootToDismiss) {
      void dismissWorkspaceBindingPrompt(payload.clientId, rootToDismiss).catch(
        () => undefined,
      );
    }
    close();
  }, [payload, workspaceRoot, close]);

  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') handlePanelClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen, handlePanelClose]);

  useEffect(() => {
    if (!isOpen || !payload || loadingData) return;
    const initial = formInitial;
    const rootValue = initial?.workspace_root ?? payload.workspaceRoot ?? '';
    const adopted = adoptSource ? adoptBindingSeed(adoptSource, rootValue) : null;
    const resolvedIcon =
      adopted?.icon ??
      initial?.icon ??
      payload.appearanceIcon ??
      appearanceIcon ??
      '';
    const resolvedLabel = adopted?.label ?? initial?.label ?? '';

    setRoot(rootValue);
    setLabel(resolvedLabel);
    setIcon(resolvedIcon);
    setSpaceId(adopted?.space_id ?? initial?.space_id ?? defaultSpaceId);
    setFsIds(adopted?.feature_set_ids ?? initial?.feature_set_ids ?? []);
    setMachineId(initial?.machine_id ?? '');
    setMachineIds(
      mode === 'edit' ? [] : defaultTargetMachineId ? [defaultTargetMachineId] : [],
    );
    setRootValidation({ state: 'idle' });
    setSubmitting(false);
    lastSavedRef.current = null;
    pendingPayloadRef.current = null;
    lastSavedAppearanceRef.current =
      mode === 'create-from-live' ? normalizeIcon(resolvedIcon) : null;
  }, [
    panelKey,
    loadingData,
    isOpen,
    payload,
    formInitial,
    defaultSpaceId,
    mode,
    defaultTargetMachineId,
    adoptSource,
    appearanceIcon,
  ]);

  useEffect(() => {
    if (!rootEditable) {
      setRootValidation({ state: 'ok', normalized: root });
      return;
    }
    if (!root.trim()) {
      setRootValidation({ state: 'idle' });
      return;
    }
    const seq = ++validationSeq.current;
    setRootValidation({ state: 'checking' });
    const handle = setTimeout(() => {
      void validateWorkspaceRoot(root)
        .then(async (normalized) => {
          if (validationSeq.current !== seq) return;
          if (!isEdit) {
            const targets = machineIds.length > 0 ? machineIds : [null as string | null];
            try {
              const existing = await listWorkspaceBindings();
              if (validationSeq.current !== seq) return;
              const hasDuplicate = targets.some((mId) => {
                const resolvedClient = mId ? null : (payload?.clientId ?? null);
                return existing.some((b) =>
                  bindingScopeConflicts(b, normalized, mId, resolvedClient),
                );
              });
              if (hasDuplicate) {
                setRootValidation({
                  state: 'error',
                  reason: t('form.duplicateRoot', { path: normalized }),
                  duplicate: true,
                });
                return;
              }
            } catch {
              /* duplicate pre-check is best-effort */
            }
          }
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
  }, [root, rootEditable, isEdit, machineIds, payload?.clientId, t]);

  useEffect(() => {
    if (mode !== 'create-from-live') return;
    const workspaceRootValue = root.trim();
    if (!workspaceRootValue) return;
    const normalizedIcon = normalizeIcon(icon);
    const baseline = lastSavedAppearanceRef.current;
    if (normalizedIcon === baseline) return;

    const handle = setTimeout(() => {
      void (async () => {
        try {
          if (normalizedIcon) {
            await upsertWorkspaceAppearance({
              workspace_root: workspaceRootValue,
              icon: normalizedIcon,
            });
          } else {
            await deleteWorkspaceAppearance(workspaceRootValue);
          }
          lastSavedAppearanceRef.current = normalizedIcon;
        } catch (e) {
          showError(t('toast.couldNotSave'), e instanceof Error ? e.message : String(e));
        }
      })();
    }, 600);
    return () => clearTimeout(handle);
  }, [mode, root, icon, showError, t]);

  const canSubmit =
    !submitting &&
    !!spaceId &&
    fsIds.length > 0 &&
    (rootValidation.state === 'ok' || !rootEditable);

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

  const handleFormSubmit = useCallback(
    async (machineTargets: (string | null)[]) => {
      if (!root.trim()) {
        showError(t('toast.couldNotSave'), t('form.errors.rootRequired'));
        return;
      }
      if (rootValidation.state === 'error') {
        showError(t('toast.couldNotSave'), rootValidation.reason);
        return;
      }
      if (!spaceId) {
        showError(t('toast.couldNotSave'), t('form.errors.pickSpace'));
        return;
      }
      if (fsIds.length === 0) {
        showError(t('toast.couldNotSave'), t('form.errors.pickFeatureSet'));
        return;
      }
      setSubmitting(true);
      try {
        if (isEdit) {
          await handleSubmit(
            buildBindingPayload({
              root,
              label,
              icon,
              spaceId,
              fsIds,
              machineId,
              clientId: payload?.clientId,
              resolvedMachineId: bindingMachineId(machineId),
            }),
          );
          return;
        }
        for (const mId of machineTargets) {
          await handleSubmit(
            buildBindingPayload({
              root,
              label,
              icon,
              spaceId,
              fsIds,
              machineId,
              clientId: payload?.clientId,
              resolvedMachineId: mId,
            }),
          );
        }
      } catch (e) {
        showError(t('toast.couldNotSave'), e instanceof Error ? e.message : String(e));
      } finally {
        setSubmitting(false);
      }
    },
    [
      root,
      rootValidation,
      spaceId,
      fsIds,
      isEdit,
      handleSubmit,
      label,
      icon,
      machineId,
      payload?.clientId,
      showError,
      t,
    ],
  );

  const handlePersistIcon = useCallback(
    async (nextIcon: string) => {
      if (!formInitial || !canSubmit) return;
      const payloadInput = buildBindingPayload({
        root,
        label,
        icon,
        spaceId,
        fsIds,
        machineId,
        clientId: payload?.clientId,
        resolvedMachineId: bindingMachineId(machineId),
      });
      payloadInput.icon = normalizeIcon(nextIcon);
      lastSavedRef.current = payloadInput;
      pendingPayloadRef.current = null;
      await handleSubmit(payloadInput);
    },
    [
      formInitial,
      canSubmit,
      root,
      label,
      icon,
      spaceId,
      fsIds,
      machineId,
      payload?.clientId,
      handleSubmit,
    ],
  );

  useEffect(() => {
    onSubmitRef.current = handleSubmit;
    onSaveStatusChangeRef.current = setSaveStatus;
  }, [handleSubmit]);

  /** Flush a debounced autosave immediately (panel close or binding switch). */
  const flushPendingSave = useCallback(() => {
    const pending = pendingPayloadRef.current;
    if (!pending) return;
    saveSeqRef.current += 1;
    onSaveStatusChangeRef.current({ kind: 'saving' });
    onSubmitRef
      .current(pending)
      .then(() => {
        onSaveStatusChangeRef.current({ kind: 'saved' });
      })
      .catch((e) => {
        console.warn(
          '[workspace-binding] flush-on-close save failed:',
          e instanceof Error ? e.message : String(e),
        );
      });
  }, []);

  useEffect(() => {
    if (!isOpen || !isEdit || !formInitial) return;
    if (!canSubmit) return;

    const candidate = buildBindingPayload({
      root,
      label,
      icon,
      spaceId,
      fsIds,
      machineId,
      clientId: payload?.clientId,
      resolvedMachineId: bindingMachineId(machineId),
    });

    const baseline = lastSavedRef.current ?? {
      workspace_root: formInitial.workspace_root,
      label: formInitial.label,
      icon: formInitial.icon,
      space_id: formInitial.space_id,
      feature_set_ids: formInitial.feature_set_ids,
      machine_id: formInitial.machine_id,
    };
    if (sameBindingInput(candidate, baseline)) {
      pendingPayloadRef.current = null;
      return;
    }

    pendingPayloadRef.current = candidate;
    const seq = ++saveSeqRef.current;
    setSaveStatus({ kind: 'idle' });
    const handle = setTimeout(async () => {
      if (saveSeqRef.current !== seq) return;
      setSaveStatus({ kind: 'saving' });
      setSubmitting(true);
      try {
        await handleSubmit(candidate);
        if (saveSeqRef.current !== seq) return;
        lastSavedRef.current = candidate;
        pendingPayloadRef.current = null;
        setSaveStatus({ kind: 'saved' });
        if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
        savedTimerRef.current = setTimeout(() => {
          setSaveStatus({ kind: 'idle' });
        }, 1800);
      } catch (e) {
        if (saveSeqRef.current !== seq) return;
        const msg = e instanceof Error ? e.message : String(e);
        setSaveStatus({ kind: 'error', message: msg });
        showError(t('toast.couldNotSave'), msg);
      } finally {
        setSubmitting(false);
      }
    }, 1500);
    return () => clearTimeout(handle);
  }, [
    isOpen,
    isEdit,
    formInitial,
    root,
    label,
    icon,
    spaceId,
    fsIds,
    machineId,
    payload?.clientId,
    canSubmit,
    handleSubmit,
    showError,
    t,
  ]);

  useEffect(() => {
    if (isOpen) return;
    flushPendingSave();
  }, [isOpen, flushPendingSave]);

  useEffect(() => {
    return () => {
      flushPendingSave();
    };
  }, [panelKey, flushPendingSave]);

  const handleDelete = useCallback(async () => {
    if (!payload?.binding) return;
    const binding = payload.binding;
    const displayName = binding.label?.trim() || folderName(binding.workspace_root);
    const machineName = binding.machine_id
      ? machines.find((m) => m.id === binding.machine_id)?.name
      : null;
    const message =
      machineName != null
        ? t('confirm.removeMessageMachine', { machine: machineName, name: displayName })
        : t('confirm.removeMessageGlobal', { name: displayName });
    const ok = await confirm({
      title: t('confirm.removeTitle'),
      message,
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
  }, [payload, machines, confirm, close, showError, t]);

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

  const machinesById = useMemo(() => new Map(machines.map((m) => [m.id, m])), [machines]);

  const machineBadgeLabel = resolveMachineBadgeLabel(
    mode,
    machineId,
    machineIds,
    machinesById,
    t,
  );

  const machineBadgeIcon = resolveMachineBadgeIcon(mode, machineId, machineIds, machinesById);

  const createSubmitLabel =
    mode === 'create-from-live' ? t('form.saveBinding') : t('form.createBinding');

  if (!isOpen || !payload) return null;

  const binding = payload.binding ?? null;
  const showEffectiveFeatures =
    (mode === 'edit' && binding != null) || (mode === 'create-from-live' && !!workspaceRoot);

  const routingSubtitle =
    mode === 'create'
      ? t('panel.routingSubtitleCreate')
      : mode === 'create-from-live'
        ? t('sheet.descNew')
        : isEdit
          ? t('panel.routingSubtitleRoutes', {
              featureSets:
                formatFsList(
                  fsIds.map((id) => featureSets.find((f) => f.id === id)?.name ?? id),
                ) || '—',
              space: spaces.find((s) => s.id === spaceId)?.name ?? '—',
            })
          : t('panel.routingSubtitleAutosave');

  const scopeSubtitle = t('panel.scopeSubtitle', {
    machine: machineBadgeLabel,
    path: root.trim() || workspaceRoot || '—',
  });

  return (
    <>
      <div
        className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-40 animate-in fade-in duration-200"
        onClick={handlePanelClose}
        data-testid="workspace-binding-panel-backdrop"
      />
      <div
        className="fixed right-0 top-0 bottom-0 w-full max-w-[480px] min-w-[420px] bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-50"
        data-testid="workspace-binding-panel"
      >
        <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
          <div className="flex items-start justify-between gap-2">
            <PanelIdentityHeader
              mode={mode}
              label={label}
              icon={icon}
              workspaceRoot={workspaceRoot}
              machineBadgeLabel={machineBadgeLabel}
              machineBadgeIcon={machineBadgeIcon}
              onMachineBadgeClick={() => scopeSectionRef.current?.expand()}
              badges={
                <>
                  {mode === 'create-from-live' && (
                    <Pill tone="primary">
                      <span className="inline-flex items-center gap-1">
                        <Sparkles className="h-2.5 w-2.5" />
                        {t('sheet.badgeNew')}
                      </span>
                    </Pill>
                  )}
                  {mode === 'create-from-live' && (
                    <Pill tone="amber">{t('card.badgeLiveUnbound')}</Pill>
                  )}
                  {mode === 'edit' && binding && <Pill tone="neutral">{t('card.offline')}</Pill>}
                </>
              }
              footer={
                mode === 'create-from-live' ? (
                  <p className="text-xs text-[rgb(var(--muted))] mt-1">
                    {t('panel.targeting', { machine: machineBadgeLabel })}
                  </p>
                ) : null
              }
              t={t}
            />
            <button
              type="button"
              onClick={handlePanelClose}
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
              {mode === 'create-from-live' && (
                <div
                  className="rounded-xl border border-amber-200/80 dark:border-amber-800/50 bg-amber-50/80 dark:bg-amber-900/15 p-4"
                  data-testid="workspace-binding-no-tools-banner"
                >
                  <p className="text-sm text-amber-900 dark:text-amber-100">{t('sheet.noToolsBanner')}</p>
                </div>
              )}
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

              {mode === 'create-from-live' && siblingBindings.length > 1 && !adoptDismissed && (
                <div
                  className="rounded-xl border border-primary-200/80 dark:border-primary-800/50 bg-primary-50/50 dark:bg-primary-900/10 p-4 space-y-3"
                  data-testid="workspace-binding-adopt-card"
                >
                  <div>
                    <p className="text-sm font-semibold text-[rgb(var(--foreground))]">
                      {t('panel.adoptTitle')}
                    </p>
                    <p className="text-xs text-[rgb(var(--muted))] mt-1">{t('panel.adoptDesc')}</p>
                  </div>
                  <div className="overflow-x-auto rounded-lg border border-[rgb(var(--border-subtle))]">
                    <table className="w-full text-xs">
                      <thead>
                        <tr className="border-b border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]">
                          <th className="px-2 py-1.5 text-left font-semibold text-[rgb(var(--muted))]">
                            {t('panel.adoptColMachine')}
                          </th>
                          <th className="px-2 py-1.5 text-left font-semibold text-[rgb(var(--muted))]">
                            {t('panel.adoptColPath')}
                          </th>
                          <th className="px-2 py-1.5 text-left font-semibold text-[rgb(var(--muted))]">
                            {t('panel.adoptColSpace')}
                          </th>
                          <th className="px-2 py-1.5 text-left font-semibold text-[rgb(var(--muted))]">
                            {t('panel.adoptColToolSet')}
                          </th>
                          <th className="px-2 py-1.5" />
                        </tr>
                      </thead>
                      <tbody>
                        {siblingBindings.map((sibling) => {
                          const machine = sibling.machine_id
                            ? machinesById.get(sibling.machine_id)
                            : null;
                          const machineLabel = machine?.name ?? t('panel.machineGlobal');
                          const spaceName =
                            spaces.find((s) => s.id === sibling.space_id)?.name ?? '—';
                          const fsNames = formatFsList(
                            sibling.feature_set_ids.map(
                              (id) => featureSets.find((f) => f.id === id)?.name ?? id,
                            ),
                          );
                          return (
                            <tr
                              key={sibling.id}
                              className="border-b border-[rgb(var(--border-subtle))] last:border-0"
                            >
                              <td className="px-2 py-2 whitespace-nowrap">
                                <span className="inline-flex items-center gap-1">
                                  {machine?.icon ? (
                                    <span className="text-sm leading-none">{machine.icon}</span>
                                  ) : null}
                                  {machineLabel}
                                </span>
                              </td>
                              <td className="px-2 py-2 font-mono text-[10px] break-all max-w-[120px]">
                                {sibling.workspace_root}
                              </td>
                              <td className="px-2 py-2 whitespace-nowrap">{spaceName}</td>
                              <td className="px-2 py-2">{fsNames || '—'}</td>
                              <td className="px-2 py-2 whitespace-nowrap">
                                <Button
                                  variant="secondary"
                                  size="sm"
                                  onClick={() => {
                                    const seed = adoptBindingSeed(sibling, root);
                                    setSpaceId(seed.space_id);
                                    setFsIds(seed.feature_set_ids);
                                    setLabel(seed.label ?? '');
                                    setIcon(seed.icon ?? '');
                                    setAdoptDismissed(true);
                                  }}
                                  data-testid={`workspace-binding-adopt-use-${sibling.id}`}
                                >
                                  {t('panel.adoptUseThis')}
                                </Button>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                  <button
                    type="button"
                    onClick={() => setAdoptDismissed(true)}
                    className="text-xs text-[rgb(var(--muted))] underline-offset-2 hover:text-[rgb(var(--foreground))] hover:underline"
                    data-testid="workspace-binding-adopt-start-fresh"
                  >
                    {t('panel.adoptStartFresh')}
                  </button>
                </div>
              )}

              <CollapsibleSection
                ref={scopeSectionRef}
                icon={<Monitor className="h-5 w-5" />}
                tone="primary"
                title={t('panel.scope')}
                subtitle={scopeSubtitle}
                defaultOpen
                testId="workspace-scope-section"
              >
                <ScopeFields
                  key={`scope-${panelKey}`}
                  mode={mode}
                  label={label}
                  setLabel={setLabel}
                  machines={machines}
                  machineId={machineId}
                  setMachineId={setMachineId}
                  machineIds={machineIds}
                  setMachineIds={setMachineIds}
                  icon={icon}
                  setIcon={setIcon}
                  onPersistIcon={isEdit ? handlePersistIcon : undefined}
                  root={root}
                  setRoot={setRoot}
                  rootValidation={rootValidation}
                  rootEditable={rootEditable}
                  onError={(msg) => showError(t('toast.couldNotSave'), msg)}
                  t={t}
                />
              </CollapsibleSection>

              <CollapsibleSection
                icon={<Route className="h-5 w-5" />}
                tone="primary"
                title={t('panel.routing')}
                subtitle={routingSubtitle}
                defaultOpen
                headerExtra={isEdit ? <SaveStatusPill status={saveStatus} t={t} /> : null}
                testId="workspace-routing-section"
              >
                <RoutingFields
                  key={`routing-${panelKey}`}
                  spaces={spaces}
                  featureSets={featureSets}
                  spaceId={spaceId}
                  setSpaceId={setSpaceId}
                  fsIds={fsIds}
                  setFsIds={setFsIds}
                  spacePickerDisabled={spaceLocked}
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
                    machineId={effectiveMachineId}
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

        {!isEdit && !loadingData && (
          <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
            <div className="flex items-center gap-2">
              <Button
                variant="primary"
                size="md"
                onClick={() => {
                  const targets = machineIds.length > 0 ? machineIds : [null as string | null];
                  void handleFormSubmit(targets);
                }}
                disabled={!canSubmit}
                className="flex-1"
                data-testid="workspace-binding-submit"
              >
                {submitting ? (
                  <Loader2 className="h-4 w-4 animate-spin mr-1.5" />
                ) : (
                  <Check className="h-4 w-4 mr-1.5" />
                )}
                {createSubmitLabel}
              </Button>
              <Button variant="secondary" size="md" onClick={handlePanelClose} disabled={submitting}>
                {t('common:actions.cancel')}
              </Button>
            </div>
          </div>
        )}

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
