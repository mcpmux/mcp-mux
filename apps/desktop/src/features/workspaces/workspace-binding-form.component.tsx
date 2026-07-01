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
  FolderOpen,
  FolderSearch,
  Loader2,
} from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  type WorkspaceBinding,
  type WorkspaceBindingInput,
} from '@/lib/api/workspaceBindings';
import { uploadWorkspaceIcon } from '@/lib/api/workspaceAppearances';
import { ServerIcon } from '@/components/ServerIcon';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';
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
 * Small pill shown in the Routing section header during edit-mode autosave.
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

/**
 * Last path segment of a workspace root, normalized for cross-platform matching.
 */
export function folderName(root: string): string {
  const segments = root.replace(/\\/g, '/').replace(/\/$/, '').split('/');
  return segments[segments.length - 1] ?? root;
}

/**
 * Bindings on other machines (or scopes) that can seed a new create-from-live row.
 * Same folder name is enough; identical absolute paths count when machine differs.
 */
export function findAdoptableSiblingBindings(
  allBindings: WorkspaceBinding[],
  workspaceRoot: string,
  targetMachineId: string | null,
): WorkspaceBinding[] {
  const currentFolder = folderName(workspaceRoot).toLowerCase();
  const normalizedRoot = workspaceRoot.toLowerCase();
  return allBindings.filter((binding) => {
    if (folderName(binding.workspace_root).toLowerCase() !== currentFolder) return false;
    const samePath = binding.workspace_root.toLowerCase() === normalizedRoot;
    if (!samePath) return true;
    return (binding.machine_id ?? null) !== targetMachineId;
  });
}

/**
 * Space, feature sets, label, and icon to copy from an adopt source binding.
 */
export function adoptBindingSeed(
  source: WorkspaceBinding,
  workspaceRoot: string,
): Pick<WorkspaceBinding, 'space_id' | 'feature_set_ids' | 'label' | 'icon'> {
  const trimmedLabel = source.label?.trim() ?? '';
  return {
    space_id: source.space_id,
    feature_set_ids: source.feature_set_ids,
    label: trimmedLabel.length > 0 ? trimmedLabel : folderName(workspaceRoot),
    icon: source.icon,
  };
}

/** True when the icon value is an uploaded file ref or URL, not a plain emoji. */
function isWorkspaceFileIcon(icon: string): boolean {
  const trimmed = icon.trim();
  return trimmed.startsWith('local:') || trimmed.startsWith('http://') || trimmed.startsWith('https://');
}

export type RootValidationState =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'ok'; normalized: string }
  | { state: 'error'; reason: string; duplicate?: boolean };

/** True when two bindings would collide on the partial unique indexes. */
export function bindingScopeConflicts(
  existing: WorkspaceBinding,
  root: string,
  machineId: string | null,
  clientId: string | null | undefined,
): boolean {
  if (existing.workspace_root !== root) return false;
  return (
    (existing.machine_id ?? null) === machineId &&
    (existing.client_id ?? null) === (clientId ?? null)
  );
}

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
 * Space picker and feature-set multiselect for workspace binding routing.
 */
export function RoutingFields({
  spaces,
  featureSets,
  spaceId,
  setSpaceId,
  fsIds,
  setFsIds,
  spacePickerDisabled = false,
  t,
}: {
  spaces: Space[];
  featureSets: FeatureSet[];
  spaceId: string;
  setSpaceId: (value: string) => void;
  fsIds: string[];
  setFsIds: (value: string[] | ((prev: string[]) => string[])) => void;
  spacePickerDisabled?: boolean;
  t: TFunction<['workspaces', 'common']>;
}) {
  const [fsSearch, setFsSearch] = useState('');

  const availableFs = useMemo(
    () => featureSets.filter((f) => f.space_id === spaceId && !f.is_deleted),
    [featureSets, spaceId],
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

  return (
    <div className="space-y-5">
      <FormField
        label={t('form.space')}
        hint={spacePickerDisabled ? t('form.spaceLockedHint') : t('form.spaceHint')}
      >
        <Picker
          value={spaceId}
          onChange={setSpaceId}
          placeholder={t('form.pickSpace')}
          disabled={spacePickerDisabled}
          options={spaces.map((s) => ({
            value: s.id,
            label: s.is_default ? `${s.name}${t('form.defaultSuffix')}` : s.name,
            icon: s.icon ?? undefined,
          }))}
          testId="workspace-binding-space-picker"
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
    </div>
  );
}

/**
 * Machine scope, workspace root, and advanced icon fields for workspace bindings.
 */
export function ScopeFields({
  mode,
  label,
  setLabel,
  machines,
  machineId,
  setMachineId,
  machineIds,
  setMachineIds,
  icon,
  setIcon,
  onPersistIcon,
  root,
  setRoot,
  rootValidation,
  rootEditable,
  onError,
  t,
}: {
  mode: 'create' | 'edit' | 'create-from-live';
  label: string;
  setLabel: (value: string) => void;
  machines: Machine[];
  machineId: string;
  setMachineId: (value: string) => void;
  machineIds: string[];
  setMachineIds: (value: string[] | ((prev: string[]) => string[])) => void;
  icon: string;
  setIcon: (value: string) => void;
  onPersistIcon?: (nextIcon: string) => Promise<void>;
  root: string;
  setRoot: (value: string) => void;
  rootValidation: RootValidationState;
  rootEditable: boolean;
  onError: (message: string) => void;
  t: TFunction<['workspaces', 'common']>;
}) {
  const rootRef = useRef<HTMLInputElement | null>(null);
  const iconPathRef = useRef<HTMLInputElement | null>(null);
  const [iconFilePath, setIconFilePath] = useState('');
  const [localMachines, setLocalMachines] = useState<Machine[]>(machines);
  const [showNewMachine, setShowNewMachine] = useState(false);
  const [newMachineName, setNewMachineName] = useState('');
  const [newMachineIcon, setNewMachineIcon] = useState('');
  const [newMachineHostname, setNewMachineHostname] = useState('');
  const [creatingMachine, setCreatingMachine] = useState(false);
  const isEdit = mode === 'edit';
  const trimmedIcon = icon.trim();
  const isFileIcon = isWorkspaceFileIcon(trimmedIcon);
  const emojiPickerValue = !isFileIcon ? trimmedIcon : '';

  useEffect(() => {
    setLocalMachines(machines);
  }, [machines]);

  useEffect(() => {
    if (mode === 'create') rootRef.current?.focus();
  }, [mode]);

  const toggleMachine = (id: string) => {
    setMachineIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  /**
   * Persist icon immediately after upload so the card updates without waiting for autosave.
   */
  const persistIconNow = async (nextIcon: string) => {
    if (onPersistIcon) {
      await onPersistIcon(nextIcon);
    }
  };

  /**
   * Replace any uploaded icon with an emoji and persist when editing.
   */
  const handleEmojiPick = (emoji: string) => {
    setIcon(emoji);
    void persistIconNow(emoji).catch((e) => onError(e instanceof Error ? e.message : String(e)));
  };

  /**
   * Open the native image picker and upload the chosen file as the workspace icon.
   */
  const handlePickIconFile = async () => {
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
      await persistIconNow(localRef);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  /**
   * Launch icon upload — native picker on desktop, path field focus on web.
   */
  const handleIconPreviewClick = () => {
    if (isTauri()) {
      void handlePickIconFile();
      return;
    }
    iconPathRef.current?.focus();
  };

  const machineOptions = useMemo(
    () => localMachines.map((m) => ({ value: m.id, label: m.name, icon: m.icon ?? undefined })),
    [localMachines],
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
            <button
              type="button"
              onClick={handleIconPreviewClick}
              className="w-14 h-14 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--background))] flex items-center justify-center flex-shrink-0 hover:bg-[rgb(var(--surface-hover))] transition-colors cursor-pointer focus:outline-none focus:ring-2 focus:ring-primary-500"
              aria-label={t('form.pickIconTitle')}
              data-testid="workspace-binding-icon-preview"
            >
              {trimmedIcon ? (
                <ServerIcon icon={trimmedIcon} className="h-9 w-9 object-contain" fallback="📁" />
              ) : (
                <FolderOpen className="h-6 w-6 text-[rgb(var(--muted))]" />
              )}
            </button>
            <div className="flex-1 min-w-0 space-y-2">
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={isFileIcon ? trimmedIcon : ''}
                  onChange={(e) => setIcon(e.target.value)}
                  placeholder={t('form.iconPlaceholder')}
                  className="min-w-0 flex-1 h-10 px-3 rounded-lg text-sm bg-[rgb(var(--background))] border border-[rgb(var(--border))] focus:outline-none focus:ring-2 focus:ring-primary-500"
                  data-testid="workspace-binding-icon-input"
                />
                <EmojiPickerButton
                  value={emojiPickerValue}
                  onChange={handleEmojiPick}
                  testId="workspace-binding-icon-emoji"
                />
              </div>
              <div className="flex items-center gap-2 flex-wrap">
                {isTauri() ? (
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void handlePickIconFile()}
                    data-testid="workspace-binding-icon-upload"
                  >
                    {t('form.upload')}
                  </Button>
                ) : (
                  <>
                    <input
                      ref={iconPathRef}
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
                    void persistIconNow('').catch((e) =>
                      onError(e instanceof Error ? e.message : String(e)),
                    );
                  }}
                  disabled={!trimmedIcon}
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
                  saveDisabled={
                    !newMachineName.trim() || !newMachineIcon.trim() || !newMachineHostname.trim()
                  }
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
                  saveDisabled={
                    !newMachineName.trim() || !newMachineIcon.trim() || !newMachineHostname.trim()
                  }
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
  state: RootValidationState;
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
      <p
        className="mt-1.5 text-[11px] text-red-600 dark:text-red-400"
        data-testid={state.duplicate ? 'workspace-binding-duplicate-error' : undefined}
      >
        {state.reason}
      </p>
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
