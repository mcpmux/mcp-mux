import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import type { TFunction } from 'i18next';
import { pickPath } from '@/lib/backend/shell';
import { isTauri } from '@/lib/backend/data/transport';
import {
  AlertCircle,
  Check,
  ChevronDown,
  FolderSearch,
  Loader2,
} from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  type WorkspaceBinding,
  type WorkspaceBindingInput,
} from '@/lib/api/workspaceBindings';
import {
  deleteWorkspaceAppearance,
  upsertWorkspaceAppearance,
} from '@/lib/api/workspaceAppearances';
import { isStarterFeatureSet, type FeatureSet } from '@/lib/api/featureSets';
import { createMachine, getHostname, type Machine } from '@/lib/api/machines';
import type { Space } from '@/lib/api/spaces';
import { MachineProfileEditor } from '@/components/machine-profile-editor';

export type SaveStatus =
  | { kind: 'idle' }
  | { kind: 'saving' }
  | { kind: 'saved' }
  | { kind: 'error'; message: string };

/**
 * Small pill shown in the Mapping section header during edit-mode autosave.
 */
export function SaveStatusPill({
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

/**
 * Structural equality between two binding inputs. The autosave effect
 * uses this to skip writes when the user re-toggled their way back to
 * the last-saved state.
 */
export function normalizeLabel(label: string | null | undefined): string | null {
  const trimmed = label?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : null;
}

export function normalizeIcon(icon: string | null | undefined): string | null {
  const trimmed = icon?.trim() ?? '';
  return trimmed.length > 0 ? trimmed : null;
}

export type RootValidationState =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'ok'; normalized: string }
  | { state: 'error'; reason: string };

/** Map empty machine picker value to null for API payloads. */
export function bindingMachineId(value: string): string | null {
  return value.trim() ? value : null;
}

/** Build a workspace binding input from lifted form field values. */
export function buildBindingPayload(params: {
  root: string;
  label: string;
  icon: string;
  spaceId: string;
  fsIds: string[];
  machineId: string;
  clientId?: string;
  resolvedMachineId: string | null;
}): WorkspaceBindingInput {
  return {
    workspace_root: params.root.trim(),
    label: params.label.trim() || null,
    icon: params.icon.trim() || null,
    space_id: params.spaceId,
    feature_set_ids: params.fsIds,
    machine_id: params.resolvedMachineId,
    client_id: params.resolvedMachineId ? null : params.clientId,
  };
}

export function sameBindingInput(
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

/**
 * Workspace binding create / edit form. Edit mode autosaves; create modes
 * submit once per selected machine (or once globally when none selected).
 */
export function BindingForm({
  mode,
  spaces,
  featureSets,
  machines,
  localMachineId,
  initial: _initial,
  initialUnmappedIcon,
  icon,
  spaceId,
  setSpaceId,
  fsIds,
  setFsIds,
  machineId,
  setMachineId,
  root,
  setRoot,
  rootValidation,
  canSubmit,
  submitting,
  onFormSubmit,
  onCancel,
  onError,
  t,
}: {
  mode: 'create' | 'edit' | 'create-from-live';
  spaces: Space[];
  featureSets: FeatureSet[];
  machines: Machine[];
  localMachineId: string | null;
  initial?: WorkspaceBinding | null;
  initialUnmappedIcon?: string | null;
  icon: string;
  spaceId: string;
  setSpaceId: (value: string) => void;
  fsIds: string[];
  setFsIds: (value: string[] | ((prev: string[]) => string[])) => void;
  machineId: string;
  setMachineId: (value: string) => void;
  root: string;
  setRoot: (value: string) => void;
  rootValidation: RootValidationState;
  canSubmit: boolean;
  submitting: boolean;
  onFormSubmit: (machineTargets: (string | null)[]) => Promise<void>;
  onCancel: () => void;
  onError: (message: string) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const rootRef = useRef<HTMLInputElement | null>(null);
  const [machineIds, setMachineIds] = useState<string[]>(() =>
    mode === 'edit' ? [] : localMachineId ? [localMachineId] : []
  );
  const [localMachines, setLocalMachines] = useState<Machine[]>(machines);
  const [fsSearch, setFsSearch] = useState('');
  const [showNewMachine, setShowNewMachine] = useState(false);
  const [newMachineName, setNewMachineName] = useState('');
  const [newMachineIcon, setNewMachineIcon] = useState('');
  const [newMachineHostname, setNewMachineHostname] = useState('');
  const [creatingMachine, setCreatingMachine] = useState(false);
  const isEdit = mode === 'edit';

  const rootEditable = mode !== 'create-from-live';

  useEffect(() => {
    if (mode === 'create') rootRef.current?.focus();
  }, [mode]);

  const availableFs = useMemo(
    () => featureSets.filter((f) => f.space_id === spaceId && !f.is_deleted),
    [featureSets, spaceId]
  );

  const filteredFs = useMemo(() => {
    const q = fsSearch.trim().toLowerCase();
    if (!q) return availableFs;
    return availableFs.filter((f) => {
      if (f.name.toLowerCase().includes(q)) return true;
      if (f.description?.toLowerCase().includes(q)) return true;
      return false;
    });
  }, [availableFs, fsSearch]);

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
    setFsIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const toggleMachine = (id: string) => {
    setMachineIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  const machineOptions = useMemo(
    () => localMachines.map((m) => ({ value: m.id, label: m.name, icon: m.icon ?? undefined })),
    [localMachines]
  );

  /**
   * Open the inline new-machine form and prefill hostname from the OS.
   */
  const handleShowNewMachine = async () => {
    setShowNewMachine(true);
    try {
      const hostname = await getHostname();
      setNewMachineHostname((prev) => prev || hostname);
    } catch {
      // hostname prefill is best-effort
    }
  };

  /**
   * Create a new machine inline and auto-select it (single picker in edit, multiselect in create).
   */
  const handleCreateMachine = async () => {
    const name = newMachineName.trim();
    const icon = newMachineIcon.trim();
    const hostname = newMachineHostname.trim();
    if (!name) return onError(t('machineIdentity.nameRequired'));
    if (!icon) return onError(t('machineIdentity.iconRequired'));
    if (!hostname) return onError(t('machineIdentity.hostnameRequired'));
    if (creatingMachine) return;
    setCreatingMachine(true);
    try {
      const created = await createMachine({ name, icon, hostname });
      setLocalMachines((prev) => [...prev, created].sort((a, b) => a.name.localeCompare(b.name)));
      if (isEdit) {
        setMachineId(created.id);
      } else {
        setMachineIds((prev) => [...prev, created.id]);
      }
      setShowNewMachine(false);
      setNewMachineName('');
      setNewMachineIcon('');
      setNewMachineHostname('');
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreatingMachine(false);
    }
  };

  const lastSavedAppearanceRef = useRef<string | null>(
    mode === 'create-from-live' ? normalizeIcon(initialUnmappedIcon) : null
  );

  const submitLabel =
    mode === 'create-from-live' ? t('form.saveBinding') : t('form.createBinding');

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
      <FormField label={t('form.machine')} hint={t('form.machineHint')}>
        {isEdit ? (
          <div className="space-y-2">
            <Picker
              value={machineId}
              onChange={setMachineId}
              options={machineOptions}
              placeholder={t('form.noMachine')}
              testId="workspace-binding-machine-select"
            />
            {showNewMachine ? (
              <div className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3 space-y-3">
                <MachineProfileEditor
                  nameDraft={newMachineName}
                  iconDraft={newMachineIcon}
                  hostnameDraft={newMachineHostname}
                  onNameDraftChange={setNewMachineName}
                  onIconDraftChange={setNewMachineIcon}
                  onHostnameDraftChange={setNewMachineHostname}
                  onSave={() => void handleCreateMachine()}
                  isSaving={creatingMachine}
                  saveDisabled={!newMachineName.trim() || !newMachineIcon.trim() || !newMachineHostname.trim()}
                  nameLabel={t('machineIdentity.nameLabel')}
                  iconLabel={t('machineIdentity.iconLabel')}
                  hostnameLabel={t('machineIdentity.hostnameLabel')}
                  saveLabel={t('sheet.continue')}
                  testIdPrefix="inline-machine-edit"
                />
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    setShowNewMachine(false);
                    setNewMachineName('');
                    setNewMachineIcon('');
                    setNewMachineHostname('');
                  }}
                >
                  {t('common:actions.cancel')}
                </Button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => void handleShowNewMachine()}
                className="text-left text-xs text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] px-0.5 transition-colors"
              >
                + {t('sheet.newMachine')}
              </button>
            )}
          </div>
        ) : (
          <div
            className="rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))]"
            data-testid="workspace-binding-machine-select"
          >
            {localMachines.length === 0 && !showNewMachine ? (
              <p className="text-xs text-[rgb(var(--muted))] italic px-3 py-2">
                {t('form.noMachine')}
              </p>
            ) : (
              <div className="max-h-56 overflow-y-auto p-1.5 space-y-1">
                {localMachines.map((m) => {
                  const isSelected = machineIds.includes(m.id);
                  return (
                    <button
                      key={m.id}
                      type="button"
                      onClick={() => toggleMachine(m.id)}
                      className={[
                        'w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded text-left text-sm transition-colors',
                        isSelected
                          ? 'bg-primary-500/10 hover:bg-primary-500/15'
                          : 'hover:bg-[rgb(var(--surface-hover))]',
                      ].join(' ')}
                      data-testid={`workspace-binding-machine-toggle-${m.id}`}
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
                          <Check className="h-3 w-3 text-white" strokeWidth={3} />
                        ) : null}
                      </div>
                      {m.icon && (
                        <span className="text-base leading-none flex-shrink-0">{m.icon}</span>
                      )}
                      <div className="flex-1 min-w-0">
                        <p className="font-medium truncate">{m.name}</p>
                        {m.hostname && (
                          <p className="text-[11px] text-[rgb(var(--muted))] truncate">
                            {m.hostname}
                          </p>
                        )}
                      </div>
                    </button>
                  );
                })}
              </div>
            )}

            {showNewMachine ? (
              <div className="border-t border-[rgb(var(--border))] p-3 space-y-3">
                <MachineProfileEditor
                  nameDraft={newMachineName}
                  iconDraft={newMachineIcon}
                  hostnameDraft={newMachineHostname}
                  onNameDraftChange={setNewMachineName}
                  onIconDraftChange={setNewMachineIcon}
                  onHostnameDraftChange={setNewMachineHostname}
                  onSave={() => void handleCreateMachine()}
                  isSaving={creatingMachine}
                  saveDisabled={!newMachineName.trim() || !newMachineIcon.trim() || !newMachineHostname.trim()}
                  nameLabel={t('machineIdentity.nameLabel')}
                  iconLabel={t('machineIdentity.iconLabel')}
                  hostnameLabel={t('machineIdentity.hostnameLabel')}
                  saveLabel={t('sheet.continue')}
                  testIdPrefix="inline-machine-create"
                />
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    setShowNewMachine(false);
                    setNewMachineName('');
                    setNewMachineIcon('');
                    setNewMachineHostname('');
                  }}
                >
                  {t('common:actions.cancel')}
                </Button>
              </div>
            ) : (
              <div className="border-t border-[rgb(var(--border))] px-2 py-1.5">
                <button
                  type="button"
                  onClick={() => void handleShowNewMachine()}
                  className="w-full text-left text-xs text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] px-1.5 py-1 rounded transition-colors"
                >
                  + {t('sheet.newMachine')}
                </button>
              </div>
            )}
          </div>
        )}
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
                          <Check className="h-3 w-3 text-white" strokeWidth={3} />
                        ) : null}
                      </div>
                      {f.icon && (
                        <span className="text-base leading-none flex-shrink-0">{f.icon}</span>
                      )}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-1.5">
                          <p className="font-medium truncate">{f.name}</p>
                          {isStarterFeatureSet(f) && (
                            <span
                              className="text-[10px] uppercase tracking-wide text-[rgb(var(--muted))] bg-[rgb(var(--surface))] px-1 py-0.5 rounded flex-shrink-0"
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
            onClick={() => {
              const targets = machineIds.length > 0 ? machineIds : [null as string | null];
              void onFormSubmit(targets);
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

/** Inline hint under the workspace_root input. */
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
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">{t('form.rootHint.idle')}</p>
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
      <p className="mt-1.5 text-[11px] text-red-600 dark:text-red-400">{state.reason}</p>
    );
  }
  const changed = state.normalized !== originalValue.trim();
  if (!changed) {
    return (
      <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">{t('form.rootHint.ready')}</p>
    );
  }
  return (
    <p className="mt-1.5 text-[11px] text-[rgb(var(--muted))]">
      {t('form.rootHint.willSaveAs', { path: state.normalized })}
    </p>
  );
}

export function FormField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
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
