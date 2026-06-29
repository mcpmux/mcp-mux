import { useEffect, useMemo, useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  Check,
  Copy,
  FolderOpen,
  FolderSearch,
  Layers,
  Loader2,
  Wrench,
  X,
} from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  validateWorkspaceRoot,
  type WorkspaceBinding,
  type WorkspaceBindingInput,
} from '@/lib/api/workspaceBindings';
import { isStarterFeatureSet, type FeatureSet } from '@/lib/api/featureSets';
import type { Space } from '@/lib/api/spaces';
import { WorkspaceInstallPanel } from './WorkspaceInstallPanel';

/**
 * Guided "set up a folder" walkthrough (the create path; editing an existing
 * mapping still uses the inspector). Three steps, by deliberate UX order:
 *
 *   1. Folder       — required; pick via dialog or a detected workspace.
 *   2. Connect apps — OPTIONAL; write the per-workspace config (header) so
 *                     apps route here even without reporting roots.
 *   3. Tools        — defaults to the Space's Starter so Finish is one click;
 *                     creating the binding here is what "maps" the folder.
 *
 * Abandoning before Finish is safe: the folder simply uses the default Starter
 * set until it's mapped, and any installed config still points at it.
 */
export function WorkspaceSetupWizard({
  spaces,
  featureSets,
  reportedRoots,
  existingBindings,
  onClose,
  onCreate,
  onError,
}: {
  spaces: Space[];
  featureSets: FeatureSet[];
  reportedRoots: string[];
  existingBindings: WorkspaceBinding[];
  onClose: () => void;
  onCreate: (input: WorkspaceBindingInput) => Promise<WorkspaceBinding>;
  onError: (msg: string) => void;
}) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  // A mapping is keyed by a folder PATH (the default) or an arbitrary ID/label
  // (a client id, machine name, … — for headless/remote clients). `folder`
  // holds whichever value the user enters.
  const [bindingType, setBindingType] = useState<'path' | 'id'>('path');
  const isId = bindingType === 'id';
  const [folder, setFolder] = useState('');
  const [validating, setValidating] = useState(false);
  const [saving, setSaving] = useState(false);

  const defaultSpaceId = useMemo(
    () => spaces.find((s) => s.is_default)?.id ?? spaces[0]?.id ?? '',
    [spaces]
  );
  const [spaceId, setSpaceId] = useState(defaultSpaceId);
  useEffect(() => {
    if (!spaceId && defaultSpaceId) setSpaceId(defaultSpaceId);
  }, [defaultSpaceId, spaceId]);

  const spaceFeatureSets = useMemo(
    () => featureSets.filter((f) => f.space_id === spaceId),
    [featureSets, spaceId]
  );
  const starterId = useMemo(
    () => spaceFeatureSets.find((f) => isStarterFeatureSet(f))?.id,
    [spaceFeatureSets]
  );
  const [fsIds, setFsIds] = useState<Set<string>>(new Set());
  // Default to the Space's Starter whenever the Space changes — keeps Finish a
  // single click and guarantees a non-empty selection (bindings require one).
  useEffect(() => {
    setFsIds(starterId ? new Set([starterId]) : new Set());
  }, [spaceId, starterId]);

  // Detected folders not already mapped — quick-pick targets for step 1.
  const boundRoots = useMemo(
    () => new Set(existingBindings.map((b) => b.workspace_root.toLowerCase())),
    [existingBindings]
  );
  const unmappedRoots = useMemo(
    () => reportedRoots.filter((r) => !boundRoots.has(r.toLowerCase())),
    [reportedRoots, boundRoots]
  );
  // Block picking a folder that already has a mapping (e.g. chosen via the
  // folder dialog) — it must be edited from the Workspaces list, not re-created.
  const alreadyMapped = useMemo(
    () => !!folder && boundRoots.has(folder.toLowerCase()),
    [folder, boundRoots]
  );

  const pickFolder = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false, title: 'Pick a folder' });
      if (typeof picked !== 'string') return;
      setValidating(true);
      const normalized = await validateWorkspaceRoot(picked).catch(() => picked);
      setFolder(normalized);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setValidating(false);
    }
  };

  const toggleFs = (id: string) =>
    setFsIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const finish = async () => {
    if (!folder || fsIds.size === 0 || !spaceId) return;
    setSaving(true);
    try {
      await onCreate({
        workspace_root: folder,
        space_id: spaceId,
        feature_set_ids: Array.from(fsIds),
        binding_type: bindingType,
      });
      // The parent transitions to the new mapping's inspector (which shows its
      // effective features) — don't close here, or that view would be lost.
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
      setSaving(false);
    }
  };

  const TITLES = isId
    ? (['Choose an id', 'How clients connect', 'Choose its tools'] as const)
    : (['Choose a folder', 'Connect your apps', 'Choose its tools'] as const);

  return (
    <div
      className="animate-in slide-in-from-right fixed bottom-0 right-0 top-0 z-50 flex w-full min-w-[420px] max-w-[480px] flex-col border-l border-[rgb(var(--border))] bg-[rgb(var(--surface))] shadow-2xl duration-300"
      data-testid="workspace-setup-wizard"
    >
      {/* Header + progress */}
      <div className="flex-shrink-0 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        <div className="flex items-start justify-between">
          <div className="min-w-0">
            <div className="text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
              {isId ? 'Set up an ID mapping' : 'Set up a folder'} · Step {step} of 3
            </div>
            <h2 className="mt-0.5 text-lg font-bold">{TITLES[step - 1]}</h2>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 transition-colors hover:bg-[rgb(var(--surface-hover))]"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
        <div className="mt-3 flex gap-1.5">
          {[1, 2, 3].map((n) => (
            <div
              key={n}
              className={`h-1 flex-1 rounded-full ${
                n <= step ? 'bg-primary-500' : 'bg-[rgb(var(--border))]'
              }`}
            />
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {step === 1 && (
          <div className="space-y-4" data-testid="wizard-step-folder">
            {/* Folder vs ID — a folder routes editors by the path they open; an
                id routes a headless/remote client by an exact label it sends. */}
            <div className="flex gap-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] p-1">
              {(['path', 'id'] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setBindingType(t)}
                  className={[
                    'flex-1 rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                    bindingType === t
                      ? 'bg-primary-500 text-white'
                      : 'text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))]',
                  ].join(' ')}
                  data-testid={`wizard-type-${t}`}
                >
                  {t === 'path' ? 'Folder' : 'ID / label'}
                </button>
              ))}
            </div>

            {isId ? (
              <>
                <p className="text-sm text-[rgb(var(--muted))]">
                  Enter an id or label — a client id, machine name, or any string. A headless or
                  remote client that sends this exact value in the{' '}
                  <code className="font-mono text-xs">X-Mcpmux-Workspace</code> header gets the
                  tools you choose next.
                </p>
                <input
                  type="text"
                  value={folder}
                  onChange={(e) => setFolder(e.target.value)}
                  placeholder="e.g. a client id or machine name"
                  className="focus:ring-primary-500 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 font-mono text-sm focus:outline-none focus:ring-2"
                  data-testid="wizard-id-input"
                />
                {alreadyMapped && (
                  <p
                    className="text-xs text-amber-700 dark:text-amber-400"
                    data-testid="wizard-folder-mapped-error"
                  >
                    That id is already mapped — edit it from the Mapping list instead.
                  </p>
                )}
              </>
            ) : (
              <>
                <p className="text-sm text-[rgb(var(--muted))]">
                  Which project folder do you want to map? Pick one, or choose a folder an app
                  already opened.
                </p>
                <Button variant="primary" size="sm" onClick={pickFolder} disabled={validating}>
                  {validating ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <FolderOpen className="mr-2 h-4 w-4" />
                  )}
                  Choose folder…
                </Button>

                {folder && (
                  <div
                    className={`flex items-center gap-2 rounded-lg border px-3 py-2 ${
                      alreadyMapped
                        ? 'border-amber-300 bg-amber-50 dark:border-amber-800/60 dark:bg-amber-900/20'
                        : 'border-[rgb(var(--border))] bg-[rgb(var(--background))]'
                    }`}
                  >
                    {alreadyMapped ? (
                      <AlertCircle className="h-4 w-4 flex-shrink-0 text-amber-600" />
                    ) : (
                      <Check className="h-4 w-4 flex-shrink-0 text-green-600" />
                    )}
                    <span className="truncate font-mono text-xs" title={folder}>
                      {folder}
                    </span>
                  </div>
                )}
                {alreadyMapped && (
                  <p
                    className="text-xs text-amber-700 dark:text-amber-400"
                    data-testid="wizard-folder-mapped-error"
                  >
                    This folder is already mapped — edit it from the Workspaces list instead.
                  </p>
                )}

                {unmappedRoots.length > 0 && (
                  <div>
                    <div className="mb-1.5 flex items-center gap-1.5 text-xs font-medium text-[rgb(var(--muted))]">
                      <FolderSearch className="h-3.5 w-3.5" />
                      Detected workspaces
                    </div>
                    <div className="overflow-hidden rounded-lg border border-[rgb(var(--border))]">
                      {unmappedRoots.slice(0, 6).map((r, i) => (
                        <button
                          key={r}
                          type="button"
                          onClick={() => setFolder(r)}
                          className={`flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-[rgb(var(--surface-hover))] ${
                            i > 0 ? 'border-t border-[rgb(var(--border-subtle))]' : ''
                          } ${folder === r ? 'bg-primary-500/5' : ''}`}
                        >
                          <FolderOpen className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
                          <span className="truncate font-mono text-xs" title={r}>
                            {r}
                          </span>
                        </button>
                      ))}
                    </div>
                  </div>
                )}
              </>
            )}
          </div>
        )}

        {step === 2 && (
          <div className="space-y-3" data-testid="wizard-step-apps">
            {isId ? (
              <div className="space-y-3">
                <p className="text-sm text-[rgb(var(--muted))]">
                  A headless or remote client routes here by sending this id in the{' '}
                  <code className="font-mono text-xs">X-Mcpmux-Workspace</code> header. There&apos;s
                  no folder to auto-write app config for — copy the value into your client.
                </p>
                <div className="flex items-stretch gap-2">
                  <code className="flex-1 select-all break-all rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 font-mono text-xs">
                    {folder || '—'}
                  </code>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void navigator.clipboard.writeText(folder).catch(() => {})}
                  >
                    <Copy className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            ) : (
              <>
                <WorkspaceInstallPanel workspaceRoot={folder} />
                <p className="text-center text-xs text-[rgb(var(--muted))]">
                  Optional — you can connect apps later from this folder&apos;s mapping.
                </p>
              </>
            )}
          </div>
        )}

        {step === 3 && (
          <div className="space-y-4" data-testid="wizard-step-tools">
            <p className="text-sm text-[rgb(var(--muted))]">
              Pick the tools this folder gets. The default Starter set works out of the box — change
              it only if this folder should see something different.
            </p>

            <div>
              <label className="mb-1 block text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
                Space
              </label>
              <select
                value={spaceId}
                onChange={(e) => setSpaceId(e.target.value)}
                className="w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2.5 text-sm"
                data-testid="wizard-space-select"
              >
                {spaces.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.icon ? `${s.icon} ` : ''}
                    {s.name}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <label className="mb-1 block text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
                Feature sets
              </label>
              <div className="overflow-hidden rounded-xl border border-[rgb(var(--border))]">
                {spaceFeatureSets.length === 0 ? (
                  <div className="px-3 py-4 text-center text-xs text-[rgb(var(--muted))]">
                    This Space has no feature sets yet.
                  </div>
                ) : (
                  spaceFeatureSets.map((fs, i) => (
                    <label
                      key={fs.id}
                      className={`flex cursor-pointer items-center gap-3 px-3 py-2.5 transition-colors hover:bg-[rgb(var(--surface-hover))] ${
                        i > 0 ? 'border-t border-[rgb(var(--border-subtle))]' : ''
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={fsIds.has(fs.id)}
                        onChange={() => toggleFs(fs.id)}
                        className="accent-primary-500 h-4 w-4 flex-shrink-0"
                      />
                      <Layers className="text-primary-500 h-4 w-4 flex-shrink-0" />
                      <span className="min-w-0 flex-1 truncate text-sm font-medium">{fs.name}</span>
                      {isStarterFeatureSet(fs) && (
                        <span className="flex-shrink-0 rounded-full bg-[rgb(var(--surface))] px-1.5 text-[10px] font-semibold uppercase tracking-wider text-[rgb(var(--muted))]">
                          Default
                        </span>
                      )}
                    </label>
                  ))
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Footer nav */}
      <div className="flex flex-shrink-0 items-center justify-between gap-3 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] p-4">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => (step === 1 ? onClose() : setStep((s) => (s - 1) as 1 | 2 | 3))}
          data-testid="wizard-back"
        >
          {step === 1 ? (
            'Cancel'
          ) : (
            <>
              <ArrowLeft className="mr-1.5 h-4 w-4" />
              Back
            </>
          )}
        </Button>

        {step < 3 ? (
          <Button
            variant="primary"
            size="sm"
            onClick={() => setStep((s) => (s + 1) as 1 | 2 | 3)}
            disabled={step === 1 && (!folder || alreadyMapped)}
            data-testid="wizard-next"
          >
            {step === 2 ? 'Next' : 'Continue'}
            <ArrowRight className="ml-1.5 h-4 w-4" />
          </Button>
        ) : (
          <Button
            variant="primary"
            size="sm"
            onClick={finish}
            disabled={saving || fsIds.size === 0 || !folder}
            data-testid="wizard-finish"
          >
            {saving ? (
              <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
            ) : (
              <Wrench className="mr-1.5 h-4 w-4" />
            )}
            Finish
          </Button>
        )}
      </div>
    </div>
  );
}
