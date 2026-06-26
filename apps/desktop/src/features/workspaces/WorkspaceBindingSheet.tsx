/**
 * Workspace Binding Sheet
 *
 * Fires when a connected client session resolves via source=Default for a
 * workspace root that has no binding yet. The user picks a Space + a
 * FeatureSet in that space, and we write a WorkspaceBinding locking both.
 *
 *  • Space picker  — defaults to the caller's current space, can be changed.
 *  • FS picker     — always includes a "space default" option (follow
 *                    whichever FS is active for the selected Space) plus
 *                    every Default + Custom set in that space.
 *  • Dismiss       — nothing written, ask again next session.
 *
 * Committing the binding emits `WorkspaceBindingChanged` on the backend,
 * which triggers `notifications/tools/list_changed` — the client re-fetches
 * its tool list under the new routing decision without reconnecting.
 */

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { apiCall } from '@/lib/api/transport';
import { useWorkspaceEvents } from '@/lib/backend/events';
import { Check, ChevronDown, FolderOpen, Loader2, Plus, Sparkles, X } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  createWorkspaceBinding,
  listWorkspaceBindings,
  type WorkspaceBinding,
} from '@/lib/api/workspaceBindings';
import {
  createMachine,
  getClientMachineId,
  listMachines,
  setClientMachineId,
  type Machine,
} from '@/lib/api/machines';
import {
  isStarterFeatureSet,
  listFeatureSetsBySpace,
  type FeatureSet,
} from '@/lib/api/featureSets';
import { listSpaces, type Space } from '@/lib/api/spaces';

interface WorkspaceNeedsBindingPayload {
  client_id: string;
  session_id: string;
  space_id: string;
  workspace_root: string;
  collision_client_id?: string | null;
  /** When true, the folder is scoped to `space_id` by a Space base directory. */
  space_locked?: boolean;
}

/** Last path segment, normalized for cross-platform roots. */
function folderName(root: string): string {
  const parts = root.replace(/\\/g, '/').replace(/\/$/, '').split('/');
  return parts[parts.length - 1] || root;
}

/**
 * Display-friendly path — strip the long prefix so a root like
 * `/home/user/code/project` or `d:\dev\project` renders compactly, while
 * keeping the full text accessible as a `title` tooltip.
 */
function shortenPath(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  if (parts.length <= 3) return path;
  const head = parts[0];
  const tail = parts.slice(-2).join('/');
  return `${head}/…/${tail}`;
}

interface AdoptRow {
  bindingId: string;
  machineName: string | null;
  workspaceRoot: string;
  spaceName: string;
  fsNames: string[];
}

type SheetStep = 'machine' | 'adopt' | 'binding';

/**
 * Pick the next step after machine assignment (or on open when machine is known).
 */
function stepAfterMachine(hasSiblings: boolean): 'adopt' | 'binding' {
  return hasSiblings ? 'adopt' : 'binding';
}

export function WorkspaceBindingSheet() {
  const { t } = useTranslation('workspaces');
  const [payload, setPayload] = useState<WorkspaceNeedsBindingPayload | null>(null);
  const [step, setStep] = useState<SheetStep>('binding');
  const [siblingBindings, setSiblingBindings] = useState<WorkspaceBinding[]>([]);
  const [fsLookup, setFsLookup] = useState<Map<string, FeatureSet>>(new Map());
  const [machines, setMachines] = useState<Machine[]>([]);
  const [loadingMachines, setLoadingMachines] = useState(false);
  const [selectedMachineId, setSelectedMachineId] = useState('');
  const [creatingMachine, setCreatingMachine] = useState(false);
  const [newMachineName, setNewMachineName] = useState('');
  const [assigningMachine, setAssigningMachine] = useState(false);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [selectedSpaceId, setSelectedSpaceId] = useState<string>('');
  const [featureSets, setFeatureSets] = useState<FeatureSet[]>([]);
  const [loadingFs, setLoadingFs] = useState(false);
  const [selectedFsId, setSelectedFsId] = useState<string>('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Only dedupe the currently-open sheet against itself — if one is already
  // showing, swallow a second emit for the same session. We deliberately
  // don't dedupe across sessions / reconnects: the backend only emits when
  // `source=Default` (i.e. no binding exists), and reconnecting a client
  // is a normal signal that the user may want to configure the folder.
  // Persisting the dismissal in a ref would black-hole later attempts
  // until the next app restart, which is how this bug surfaced before.
  const currentSessionRef = useRef<string | null>(null);
  currentSessionRef.current = payload?.session_id ?? null;
  const pendingFsPrefillRef = useRef<string | null>(null);

  const { subscribe } = useWorkspaceEvents();

  useEffect(() => {
    return subscribe('workspace-needs-binding', async (p) => {
      if (currentSessionRef.current !== null) return;
      try {
        const enabled = await apiCall<boolean>('get_workspace_mapping_prompt_enabled');
        if (!enabled) return;
      } catch {
        /* setting unavailable → default to showing */
      }
      if (currentSessionRef.current !== null) return;
      const payloadData = p as WorkspaceNeedsBindingPayload;
      setPayload(payloadData);
      setSelectedSpaceId(payloadData.space_id);
      setSelectedFsId('');
      setError(null);
      setCreatingMachine(false);
      setNewMachineName('');
      pendingFsPrefillRef.current = null;
      setSiblingBindings([]);
      setFsLookup(new Map());
      try {
        const [clientMachineId, allBindings] = await Promise.all([
          getClientMachineId(payloadData.client_id),
          listWorkspaceBindings(),
        ]);
        setSelectedMachineId(clientMachineId ?? '');
        const currentFolder = folderName(payloadData.workspace_root);
        const siblings = allBindings.filter(
          (b) =>
            b.workspace_root.toLowerCase() !== payloadData.workspace_root.toLowerCase() &&
            folderName(b.workspace_root).toLowerCase() === currentFolder.toLowerCase(),
        );
        setSiblingBindings(siblings);
        if (!clientMachineId) {
          setStep('machine');
        } else {
          setStep(stepAfterMachine(siblings.length > 0));
        }
      } catch {
        setSelectedMachineId('');
        setSiblingBindings([]);
        setStep('machine');
      }
    });
  }, [subscribe]);

  useEffect(() => {
    return subscribe('workspace-binding-changed', (p) => {
      setPayload((current) => {
        if (!current) return null;
        const changed = (p as { workspace_root: string }).workspace_root;
        if (changed.toLowerCase() === current.workspace_root.toLowerCase()) return null;
        return current;
      });
    });
  }, [subscribe]);

  useEffect(() => {
    if (!payload || (step !== 'machine' && step !== 'adopt' && step !== 'binding')) return;
    let cancelled = false;
    setLoadingMachines(true);
    listMachines()
      .then((list) => {
        if (!cancelled) setMachines(list);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoadingMachines(false);
      });
    return () => {
      cancelled = true;
    };
  }, [payload, step]);

  useEffect(() => {
    if (!payload || siblingBindings.length === 0) return;
    let cancelled = false;
    const spaceIds = [...new Set(siblingBindings.map((b) => b.space_id))];
    Promise.all(spaceIds.map((id) => listFeatureSetsBySpace(id)))
      .then((results) => {
        if (cancelled) return;
        const map = new Map<string, FeatureSet>();
        for (const list of results) {
          for (const fs of list) {
            if (!fs.is_deleted) map.set(fs.id, fs);
          }
        }
        setFsLookup(map);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [payload, siblingBindings]);

  // Load every Space once the sheet is visible so the user can pin the
  // binding to a different Space than the caller happened to land in.
  useEffect(() => {
    if (!payload) return;
    let cancelled = false;
    listSpaces()
      .then((list) => {
        if (!cancelled) setSpaces(list);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [payload]);

  // Reload FS list whenever the target space changes. After the list
  // arrives, preselect the Space's Default FS so the user has a valid
  // selection out of the box — picking a FS from a different Space would
  // fail on save.
  useEffect(() => {
    if (!payload || !selectedSpaceId) return;
    let cancelled = false;
    setLoadingFs(true);
    setSelectedFsId('');
    listFeatureSetsBySpace(selectedSpaceId)
      .then((list) => {
        if (cancelled) return;
        const visible = list.filter((fs) => !fs.is_deleted);
        setFeatureSets(visible);
        const prefill = pendingFsPrefillRef.current;
        if (prefill && visible.some((fs) => fs.id === prefill)) {
          setSelectedFsId(prefill);
          pendingFsPrefillRef.current = null;
        } else {
          const seedFs = visible.find(isStarterFeatureSet) ?? visible[0];
          if (seedFs) setSelectedFsId(seedFs.id);
        }
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoadingFs(false);
      });
    return () => {
      cancelled = true;
    };
  }, [payload, selectedSpaceId]);

  const markSeenAndClose = (_p: WorkspaceNeedsBindingPayload) => {
    setPayload(null);
  };

  const handleContinueFromMachine = async () => {
    if (!payload || assigningMachine || !selectedMachineId) {
      if (!selectedMachineId) setError(t('sheet.pickMachine'));
      return;
    }
    setAssigningMachine(true);
    setError(null);
    try {
      await setClientMachineId(payload.client_id, selectedMachineId);
      setStep(stepAfterMachine(siblingBindings.length > 0));
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    } finally {
      setAssigningMachine(false);
    }
  };

  const handleCreateMachine = async () => {
    const name = newMachineName.trim();
    if (!name) {
      setError(t('sheet.machineNameRequired'));
      return;
    }
    setAssigningMachine(true);
    setError(null);
    try {
      const created = await createMachine({ name });
      setMachines((prev) => [...prev, created]);
      setSelectedMachineId(created.id);
      setCreatingMachine(false);
      setNewMachineName('');
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    } finally {
      setAssigningMachine(false);
    }
  };

  const handleSave = async () => {
    if (!payload || saving || !selectedSpaceId) return;
    if (!selectedFsId) {
      setError(t('sheet.pickFeatureSet'));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await createWorkspaceBinding({
        workspace_root: payload.workspace_root,
        space_id: selectedSpaceId,
        feature_set_ids: [selectedFsId],
        client_id: payload.client_id,
        machine_id: selectedMachineId || null,
      });
      markSeenAndClose(payload);
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleDismiss = () => {
    if (!payload || saving) return;
    markSeenAndClose(payload);
  };

  const handleAdoptBinding = (binding: WorkspaceBinding) => {
    const fsId = binding.feature_set_ids[0];
    pendingFsPrefillRef.current = fsId ?? null;
    const sameSpace = binding.space_id === selectedSpaceId;
    if (!sameSpace) {
      setSelectedSpaceId(binding.space_id);
    } else if (fsId) {
      const hasFs =
        featureSets.some((fs) => fs.id === fsId) || fsLookup.has(fsId);
      if (hasFs) {
        setSelectedFsId(fsId);
        pendingFsPrefillRef.current = null;
      }
    }
    setStep('binding');
  };

  const handleStartFresh = () => {
    pendingFsPrefillRef.current = null;
    setStep('binding');
  };

  const handleDisablePrompt = async () => {
    if (!payload || saving) return;
    try {
      await apiCall('set_workspace_mapping_prompt_enabled', { enabled: false });
      markSeenAndClose(payload);
    } catch (e) {
      setError(typeof e === 'string' ? e : String(e));
    }
  };

  if (!payload) return null;

  const machinesById = new Map(machines.map((m) => [m.id, m]));
  const spacesById = new Map(spaces.map((s) => [s.id, s]));
  const adoptRows = buildAdoptRows(siblingBindings, machinesById, spacesById, fsLookup);

  return (
    <div
      className="fixed inset-0 z-50 flex items-stretch justify-end bg-black/40 backdrop-blur-sm animate-fade-in"
      onClick={handleDismiss}
    >
      <div
        className="relative flex h-full w-full max-w-md flex-col bg-[rgb(var(--background))] shadow-2xl animate-slide-in"
        onClick={(e) => e.stopPropagation()}
      >
        <button
          className="absolute right-4 top-4 rounded-full p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--foreground))]"
          onClick={handleDismiss}
          aria-label={t('sheet.closeAria')}
        >
          <X className="h-4 w-4" />
        </button>

        <div className="px-8 pt-10 pb-6">
          <div className="mb-5 inline-flex items-center gap-2 rounded-full border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-1 text-xs font-medium text-[rgb(var(--muted))]">
            <Sparkles className="h-3 w-3 text-[rgb(var(--accent))]" />
            {payload.collision_client_id ? t('sheet.badgeCollision') : t('sheet.badgeNew')}
          </div>
          <h2 className="text-[22px] font-semibold leading-tight tracking-tight text-[rgb(var(--foreground))]">
            {payload.collision_client_id ? t('sheet.titleCollision') : t('sheet.titleNew')}
          </h2>
          <p className="mt-2 text-sm text-[rgb(var(--muted))]">
            {payload.collision_client_id ? t('sheet.descCollision') : t('sheet.descNew')}
          </p>

          <div className="mt-5 flex items-start gap-3 rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-4 py-3">
            <FolderOpen className="mt-0.5 h-4 w-4 flex-shrink-0 text-[rgb(var(--accent))]" />
            <div className="min-w-0 flex-1">
              <div
                className="truncate font-mono text-sm text-[rgb(var(--foreground))]"
                title={payload.workspace_root}
              >
                {shortenPath(payload.workspace_root)}
              </div>
            </div>
          </div>

          {/* Self-intro: point at the per-workspace installer so apps that
              don't report this folder (e.g. Cursor) still route here. */}
          <p className="mt-3 text-xs text-[rgb(var(--muted))]" data-testid="binding-sheet-install-hint">
            Tip: app not routing here? In the Workspaces tab, open this folder and{' '}
            <span className="font-medium text-[rgb(var(--foreground))]">Connect apps to this folder</span>{' '}
            to write its config with a workspace header — it works even when the app doesn&apos;t
            report the folder.
          </p>
        </div>

        <div className="flex-1 overflow-y-auto px-8 pb-6 space-y-6">
          {step === 'machine' ? (
            <div>
              <div className="mb-3 text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
                {t('sheet.machine')}
              </div>
              <p className="mb-4 text-sm text-[rgb(var(--muted))]">{t('sheet.machineDesc')}</p>
              {loadingMachines ? (
                <div className="flex items-center justify-center py-8 text-[rgb(var(--muted))]">
                  <Loader2 className="h-4 w-4 animate-spin" />
                </div>
              ) : (
                <div className="space-y-1.5">
                  {machines.map((machine) => (
                    <ChoiceRow
                      key={machine.id}
                      selected={selectedMachineId === machine.id}
                      onSelect={() => setSelectedMachineId(machine.id)}
                      title={machine.icon ? `${machine.icon}  ${machine.name}` : machine.name}
                      subtitle={machine.hostname ?? undefined}
                    />
                  ))}
                  {!creatingMachine ? (
                    <button
                      type="button"
                      onClick={() => setCreatingMachine(true)}
                      className="flex w-full items-center gap-2 rounded-xl border border-dashed border-[rgb(var(--border))] px-4 py-3 text-sm text-[rgb(var(--muted))] transition-colors hover:border-[rgb(var(--accent))] hover:text-[rgb(var(--foreground))]"
                    >
                      <Plus className="h-4 w-4" />
                      {t('sheet.newMachine')}
                    </button>
                  ) : (
                    <div className="rounded-xl border border-[rgb(var(--border))] p-4 space-y-3">
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
                          className="flex-1"
                          onClick={handleCreateMachine}
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
                </div>
              )}
            </div>
          ) : step === 'adopt' ? (
            <AdoptStep
              rows={adoptRows}
              loading={loadingMachines}
              onAdopt={(bindingId) => {
                const binding = siblingBindings.find((b) => b.id === bindingId);
                if (binding) handleAdoptBinding(binding);
              }}
              onStartFresh={handleStartFresh}
              t={t}
            />
          ) : (
            <>
          <div>
            <div className="mb-3 text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
              {t('sheet.space')}
            </div>
            <div className="relative">
              <select
                value={selectedSpaceId}
                onChange={(e) => setSelectedSpaceId(e.target.value)}
                disabled={payload.space_locked}
                className="w-full appearance-none rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-4 py-3 pr-10 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:border-[rgb(var(--border-hover,var(--accent)))] focus:border-[rgb(var(--accent))] focus:outline-none disabled:cursor-not-allowed disabled:opacity-60"
                data-testid="workspace-binding-space-picker"
              >
                {spaces.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.icon ? `${s.icon}  ` : ''}
                    {s.name}
                    {s.is_default ? t('form.defaultSuffix') : ''}
                  </option>
                ))}
              </select>
              <ChevronDown className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))]" />
            </div>
          </div>

          <div>
            <div className="mb-3 text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
              {t('form.machine')}
            </div>
            <p className="mb-3 text-xs text-[rgb(var(--muted))]">{t('form.machineHint')}</p>
            {loadingMachines ? (
              <div className="flex items-center justify-center py-4 text-[rgb(var(--muted))]">
                <Loader2 className="h-4 w-4 animate-spin" />
              </div>
            ) : (
              <>
                <div className="relative">
                  <select
                    value={selectedMachineId}
                    onChange={(e) => setSelectedMachineId(e.target.value)}
                    className="w-full appearance-none rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-4 py-3 pr-10 text-sm font-medium text-[rgb(var(--foreground))] transition-colors hover:border-[rgb(var(--border-hover,var(--accent)))] focus:border-[rgb(var(--accent))] focus:outline-none"
                    data-testid="workspace-binding-machine-picker"
                  >
                    <option value="">{t('form.noMachine')}</option>
                    {machines.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.icon ? `${m.icon}  ` : ''}
                        {m.name}
                      </option>
                    ))}
                  </select>
                  <ChevronDown className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 text-[rgb(var(--muted))]" />
                </div>
                {!creatingMachine ? (
                  <button
                    type="button"
                    onClick={() => setCreatingMachine(true)}
                    className="mt-2 flex w-full items-center gap-2 rounded-xl border border-dashed border-[rgb(var(--border))] px-4 py-2.5 text-sm text-[rgb(var(--muted))] transition-colors hover:border-[rgb(var(--accent))] hover:text-[rgb(var(--foreground))]"
                  >
                    <Plus className="h-4 w-4" />
                    {t('sheet.newMachine')}
                  </button>
                ) : (
                  <div className="mt-2 rounded-xl border border-[rgb(var(--border))] p-4 space-y-3">
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
                        className="flex-1"
                        onClick={handleCreateMachine}
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
              </>
            )}
          </div>

          <div>
            <div className="mb-3 text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
              {t('sheet.toolSet')}
            </div>
            {loadingFs ? (
              <div className="flex items-center justify-center py-8 text-[rgb(var(--muted))]">
                <Loader2 className="h-4 w-4 animate-spin" />
              </div>
            ) : featureSets.length === 0 ? (
              <div className="rounded-xl border border-dashed border-[rgb(var(--border))] px-4 py-6 text-center text-xs text-[rgb(var(--muted))]">
                {t('sheet.noFeatureSets')}
              </div>
            ) : (
              <div className="space-y-1.5">
                {featureSets.map((fs) => (
                  <ChoiceRow
                    key={fs.id}
                    selected={selectedFsId === fs.id}
                    onSelect={() => setSelectedFsId(fs.id)}
                    title={fs.name}
                    subtitle={fs.description || describeFs(fs, t)}
                    badge={isStarterFeatureSet(fs) ? t('sheet.starter') : undefined}
                  />
                ))}
              </div>
            )}
          </div>
            </>
          )}
        </div>

        <div className="border-t border-[rgb(var(--border))] px-8 py-4">
          {error && (
            <div className="mb-3 rounded-lg bg-red-500/10 p-2.5 text-xs text-red-500">
              {error}
            </div>
          )}
          {/* "Not now" auto-sizes to its label; the primary action takes
              the rest of the row. Equal flex-1 columns wrapped the longer
              "Remember for this folder" text onto two lines. */}
          <div className="flex gap-2">
            <Button
              variant="secondary"
              className="px-5"
              onClick={handleDismiss}
              disabled={saving || assigningMachine}
            >
              {t('sheet.notNow')}
            </Button>
            {step === 'machine' ? (
              <Button
                variant="primary"
                className="flex-1 whitespace-nowrap"
                onClick={handleContinueFromMachine}
                disabled={assigningMachine || loadingMachines || !selectedMachineId}
              >
                {assigningMachine ? (
                  <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
                ) : (
                  <Check className="mr-1.5 h-4 w-4" />
                )}
                {t('sheet.continue')}
              </Button>
            ) : step === 'binding' ? (
            <Button
              variant="primary"
              className="flex-1 whitespace-nowrap"
              onClick={handleSave}
              disabled={saving || loadingFs || !selectedSpaceId}
            >
              {saving ? (
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
              ) : (
                <Check className="mr-1.5 h-4 w-4" />
              )}
              {t('sheet.remember')}
            </Button>
            ) : null}
          </div>
          {step === 'binding' && (
          <>
          <p className="mt-3 text-center text-[11px] text-[rgb(var(--muted))]">
            {t('sheet.footer')}
          </p>
          <div className="mt-1.5 text-center">
            <button
              type="button"
              onClick={handleDisablePrompt}
              disabled={saving}
              title={t('sheet.disablePromptTitle')}
              className="text-[11px] text-[rgb(var(--muted))] underline-offset-2 transition-colors hover:text-[rgb(var(--foreground))] hover:underline disabled:opacity-50"
              data-testid="workspace-binding-disable-prompt"
            >
              {t('sheet.disablePrompt')}
            </button>
          </div>
          </>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * Build display rows for sibling bindings shown in the adopt step.
 */
function buildAdoptRows(
  siblings: WorkspaceBinding[],
  machinesById: Map<string, Machine>,
  spacesById: Map<string, Space>,
  fsById: Map<string, FeatureSet>,
): AdoptRow[] {
  return siblings.map((b) => {
    const machine = b.machine_id ? machinesById.get(b.machine_id) : undefined;
    const space = spacesById.get(b.space_id);
    const fsNames = b.feature_set_ids
      .map((id) => fsById.get(id)?.name)
      .filter((name): name is string => Boolean(name));
    return {
      bindingId: b.id,
      machineName: machine?.name ?? null,
      workspaceRoot: b.workspace_root,
      spaceName: space?.name ?? '—',
      fsNames,
    };
  });
}

/**
 * Adopt step — table of folder-name-matched bindings from other machines.
 */
function AdoptStep({
  rows,
  loading,
  onAdopt,
  onStartFresh,
  t,
}: {
  rows: AdoptRow[];
  loading: boolean;
  onAdopt: (bindingId: string) => void;
  onStartFresh: () => void;
  t: TFunction<'workspaces'>;
}) {
  const headCls =
    'pb-1 pr-2 text-left text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))] last:pr-0';
  const cellCls = 'py-1.5 pr-2 align-top text-[11px] text-[rgb(var(--foreground))] last:pr-0';

  return (
    <div>
      <div className="mb-3 text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
        {t('sheet.adopt')}
      </div>
      <p className="mb-4 text-sm text-[rgb(var(--muted))]">{t('sheet.adoptDesc')}</p>
      {loading ? (
        <div className="flex items-center justify-center py-8 text-[rgb(var(--muted))]">
          <Loader2 className="h-4 w-4 animate-spin" />
        </div>
      ) : (
        <div className="overflow-x-auto rounded-xl border border-[rgb(var(--border))]">
          <table className="w-full border-collapse text-xs">
            <thead>
              <tr className="border-b border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]">
                <th className={`${headCls} pl-3 pt-2`}>{t('sheet.adoptColMachine')}</th>
                <th className={`${headCls} pt-2`}>{t('sheet.adoptColPath')}</th>
                <th className={`${headCls} pt-2`}>{t('sheet.adoptColSpace')}</th>
                <th className={`${headCls} pt-2`}>{t('sheet.adoptColToolSet')}</th>
                <th className={`${headCls} pr-3 pt-2`} aria-hidden />
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr
                  key={row.bindingId}
                  className="border-b border-[rgb(var(--border-subtle))] last:border-b-0"
                >
                  <td className={`${cellCls} pl-3 whitespace-nowrap`} title={row.machineName ?? undefined}>
                    {row.machineName ?? t('sheet.adoptNoMachine')}
                  </td>
                  <td className={cellCls} title={row.workspaceRoot}>
                    <span className="font-mono">{shortenPath(row.workspaceRoot)}</span>
                  </td>
                  <td className={`${cellCls} whitespace-nowrap`}>{row.spaceName}</td>
                  <td className={cellCls}>
                    {row.fsNames.length > 0 ? row.fsNames.join(', ') : '—'}
                  </td>
                  <td className={`${cellCls} pr-3 whitespace-nowrap`}>
                    <Button variant="secondary" className="h-7 px-2.5 text-xs" onClick={() => onAdopt(row.bindingId)}>
                      {t('sheet.adoptUseThis')}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="mt-4 text-center">
        <button
          type="button"
          onClick={onStartFresh}
          className="text-sm text-[rgb(var(--muted))] underline-offset-2 transition-colors hover:text-[rgb(var(--foreground))] hover:underline"
        >
          {t('sheet.adoptStartFresh')}
        </button>
      </div>
    </div>
  );
}

function ChoiceRow({
  selected,
  onSelect,
  title,
  subtitle,
  badge,
}: {
  selected: boolean;
  onSelect: () => void;
  title: string;
  subtitle?: string;
  badge?: string;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className={[
        'group flex w-full items-start gap-3 rounded-xl border px-4 py-3 text-left transition-all',
        selected
          ? 'border-primary-500 bg-primary-50 shadow-sm dark:bg-primary-900/20 dark:border-primary-400'
          : 'border-[rgb(var(--border))] bg-[rgb(var(--background))] hover:border-[rgb(var(--border-strong,var(--border)))] hover:bg-[rgb(var(--surface-hover,var(--surface)))]',
      ].join(' ')}
    >
      <div
        className={[
          'mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full border-2 transition-all',
          selected
            ? 'border-primary-500 bg-primary-500 dark:border-primary-400 dark:bg-primary-400'
            : 'border-[rgb(var(--border))] bg-[rgb(var(--background))] group-hover:border-[rgb(var(--muted))]',
        ].join(' ')}
      >
        {selected && <Check className="h-3 w-3 text-white" strokeWidth={3.5} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <div
            className={[
              'text-sm font-medium',
              selected
                ? 'text-primary-900 dark:text-primary-100'
                : 'text-[rgb(var(--foreground))]',
            ].join(' ')}
          >
            {title}
          </div>
          {badge && (
            <span className="inline-flex items-center px-1.5 py-0.5 rounded-md bg-[rgb(var(--surface-hover,var(--surface)))] text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]">
              {badge}
            </span>
          )}
        </div>
        {subtitle && (
          <div className="mt-0.5 text-xs text-[rgb(var(--muted))]">{subtitle}</div>
        )}
      </div>
    </button>
  );
}

/**
 * Fallback description for a feature set row when no description is set.
 */
function describeFs(fs: FeatureSet, t: TFunction<'workspaces'>): string {
  switch (fs.feature_set_type) {
    case 'default':
      return t('sheet.fsDefault');
    case 'custom':
      return t('sheet.fsMembers', { count: fs.members.length });
    default:
      return '';
  }
}
