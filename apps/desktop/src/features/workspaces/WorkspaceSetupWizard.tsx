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
import { getGatewayStatus } from '@/lib/api/gateway';
import { getGatewayAuthDisabled } from '@/lib/api/workspaceInstall';
import type { Space } from '@/lib/api/spaces';
import { WorkspaceInstallPanel } from './WorkspaceInstallPanel';
import { CreateFeatureSetLink } from './CreateFeatureSetLink';
import {
  buildMcpConfig,
  COPIED_LABEL,
  COPY_CONFIG_BEARER_LABEL,
  COPY_CONFIG_LABEL,
  DEFAULT_MCP_ENDPOINT,
} from './connectConfig';

/**
 * Guided "set up a mapping" walkthrough (the create path; editing an existing
 * mapping still uses the inspector). The step sequence is binding-type-aware:
 *
 *   1. Identify     — required; pick a project folder via dialog / a detected
 *                     workspace, or enter an arbitrary identifier.
 *   2. Connect apps — PROJECT mappings only; write the per-workspace config so
 *                     apps route here even without reporting roots. ID/virtual
 *                     mappings skip this — there's no project to write config
 *                     for, and step 1's ConnectPreview already shows the exact
 *                     config to paste — so they jump straight to tools.
 *   3. Tools        — defaults to the Space's Starter so Finish is one click;
 *                     creating the binding here is what "maps" the project.
 *
 * So a project mapping is 3 steps; an ID mapping is 2 (identify → tools).
 * Abandoning before Finish is safe: the project simply uses the default Starter
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
  // Position within the (binding-type-aware) step sequence, not a fixed 1|2|3 —
  // see `sequence` below. 0-based; the identify step is always index 0.
  const [stepIndex, setStepIndex] = useState(0);
  // A mapping is keyed by a folder PATH (the default) or an arbitrary workspace
  // identifier (any string a client sends in the X-Mcpmux-Workspace header).
  // `folder` holds whichever value the user enters.
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

  // The step sequence depends on the binding type: an ID/virtual mapping skips
  // the "connect apps" step (step 1's ConnectPreview already covers how to wire
  // a client), so it's identify → tools; a project mapping keeps all three. The
  // indicator + nav are driven from the position in this sequence rather than a
  // hardcoded 1|2|3, so they stay correct as the type toggle (only reachable on
  // the identify step) flips the length. `safeIndex` clamps defensively so a
  // shrinking sequence can never point past its end.
  type StepName = 'identify' | 'apps' | 'tools';
  const sequence: StepName[] = isId ? ['identify', 'tools'] : ['identify', 'apps', 'tools'];
  const safeIndex = Math.min(stepIndex, sequence.length - 1);
  const currentStep = sequence[safeIndex];
  const isLastStep = safeIndex === sequence.length - 1;
  const title =
    currentStep === 'identify'
      ? isId
        ? 'Choose an identifier'
        : 'Select a project'
      : currentStep === 'apps'
        ? 'Connect your apps'
        : 'Choose its tools';

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
              {isId ? 'Set up an ID mapping' : 'Set up a mapping'} · Step {safeIndex + 1} of{' '}
              {sequence.length}
            </div>
            <h2 className="mt-0.5 text-lg font-bold">{title}</h2>
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
          {sequence.map((name, i) => (
            <div
              key={name}
              className={`h-1 flex-1 rounded-full ${
                i <= safeIndex ? 'bg-primary-500' : 'bg-[rgb(var(--border))]'
              }`}
            />
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {currentStep === 'identify' && (
          <div className="space-y-4" data-testid="wizard-step-folder">
            {/* Project vs identifier — a project routes editors by the folder
                path they open; an identifier routes a client by the exact
                string it sends in the X-Mcpmux-Workspace header. */}
            <div className="flex gap-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] p-1">
              {(['path', 'id'] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => {
                    if (t === bindingType) return;
                    // Switching modes starts the new tab clean. `folder` holds
                    // the value for whichever mode is active, so carrying it (and
                    // any "already mapped" warning it raised) into the other tab
                    // would surface stale state. Reset the value and the transient
                    // validating flag; `alreadyMapped` is derived from `folder`,
                    // so clearing it dismisses the warning too.
                    setBindingType(t);
                    setFolder('');
                    setValidating(false);
                  }}
                  className={[
                    'flex-1 rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                    bindingType === t
                      ? 'bg-primary-500 text-white'
                      : 'text-[rgb(var(--muted))] hover:bg-[rgb(var(--surface-hover))]',
                  ].join(' ')}
                  data-testid={`wizard-type-${t}`}
                >
                  {t === 'path' ? 'Project' : 'Identifier'}
                </button>
              ))}
            </div>

            {isId ? (
              <>
                <div className="space-y-1.5">
                  <label className="block text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
                    Workspace identifier
                  </label>
                  <p className="text-sm text-[rgb(var(--muted))]">
                    Any string you choose. A client that sends this value in the{' '}
                    <code className="font-mono text-xs">X-Mcpmux-Workspace</code> header gets the
                    tools you select next.
                  </p>
                  <input
                    type="text"
                    value={folder}
                    onChange={(e) => setFolder(e.target.value)}
                    placeholder="e.g. my-workspace"
                    className="focus:ring-primary-500 w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 font-mono text-sm focus:outline-none focus:ring-2"
                    data-testid="wizard-id-input"
                  />
                </div>
                {alreadyMapped && (
                  <p
                    className="text-xs text-amber-700 dark:text-amber-400"
                    data-testid="wizard-folder-mapped-error"
                  >
                    That identifier is already mapped — edit it from the Mapping list instead.
                  </p>
                )}
              </>
            ) : (
              <>
                <p className="text-sm text-[rgb(var(--muted))]">
                  Which project do you want to map? Pick one, or choose a project an app already
                  opened.
                </p>
                <Button variant="primary" size="sm" onClick={pickFolder} disabled={validating}>
                  {validating ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <FolderOpen className="mr-2 h-4 w-4" />
                  )}
                  Select a project…
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
                    This project is already mapped — edit it from the Mapping list instead.
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

            {/* Fill the step's empty space with a live preview of how a client
                actually uses this mapping — the exact MCP config to paste. */}
            <ConnectPreview mode={bindingType} value={folder} />
          </div>
        )}

        {/* PROJECT mappings only — an ID/virtual mapping's sequence omits this
            step (its ConnectPreview on step 1 already shows the config to paste). */}
        {currentStep === 'apps' && (
          <div className="space-y-3" data-testid="wizard-step-apps">
            <WorkspaceInstallPanel workspaceRoot={folder} />
            <p className="text-center text-xs text-[rgb(var(--muted))]">
              Optional — you can connect apps later from this project&apos;s mapping.
            </p>
          </div>
        )}

        {currentStep === 'tools' && (
          <div className="space-y-4" data-testid="wizard-step-tools">
            <p className="text-sm text-[rgb(var(--muted))]">
              Pick the tools this project gets. The default Starter set works out of the box — change
              it only if this project should see something different.
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
              {/* Only Starter ships with a new Space — point the user at where
                  they can make another set if it needs different tools. */}
              <div className="mt-2">
                <CreateFeatureSetLink spaceId={spaceId} />
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
          onClick={() => (safeIndex === 0 ? onClose() : setStepIndex((i) => Math.max(0, i - 1)))}
          data-testid="wizard-back"
        >
          {safeIndex === 0 ? (
            'Cancel'
          ) : (
            <>
              <ArrowLeft className="mr-1.5 h-4 w-4" />
              Back
            </>
          )}
        </Button>

        {!isLastStep ? (
          <Button
            variant="primary"
            size="sm"
            onClick={() => setStepIndex((i) => i + 1)}
            disabled={currentStep === 'identify' && (!folder || alreadyMapped)}
            data-testid="wizard-next"
          >
            {currentStep === 'apps' ? 'Next' : 'Continue'}
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

/**
 * Live "how a client connects" preview for step 1 — shows the exact MCP client
 * config the user will paste, so the abstract project-vs-identifier choice has a
 * concrete payoff on screen. Derived purely from `mode` + `value`, so it
 * updates as the user types and resets cleanly when the binding-type toggle
 * clears the value.
 *
 * Both modes pin the `X-Mcpmux-Workspace` header to the entered value (or a
 * templated placeholder until one is supplied) — an identifier matches
 * verbatim, a project matches its folder path. A note reminds the user this
 * header is only ONE of three ways to route here (OAuth approval, a Bearer API
 * key, or this header). Two copy buttons are offered: the plain header config,
 * and a "with Bearer" variant for clients sending an API key (required when
 * inbound auth is on).
 */
function ConnectPreview({ mode, value }: { mode: 'path' | 'id'; value: string }) {
  const isId = mode === 'id';
  const [mcpUrl, setMcpUrl] = useState<string | null>(null);
  const [authDisabled, setAuthDisabled] = useState<boolean | null>(null);
  const [copied, setCopied] = useState<'header' | 'bearer' | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [status, disabled] = await Promise.all([
        getGatewayStatus().catch(() => null),
        getGatewayAuthDisabled().catch(() => null),
      ]);
      if (cancelled) return;
      setMcpUrl(status?.url ? `${status.url}/mcp` : null);
      setAuthDisabled(disabled);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const endpoint = mcpUrl ?? DEFAULT_MCP_ENDPOINT;
  // Show a templated placeholder until the user supplies a value — keeps the
  // header readable as a fill-in rather than an empty string.
  const workspace = value.trim() || (isId ? '<your-identifier>' : '<your-project-path>');
  const headerConfig = useMemo(() => buildMcpConfig({ endpoint, workspace }), [endpoint, workspace]);
  const bearerConfig = useMemo(
    () => buildMcpConfig({ endpoint, workspace, bearer: true }),
    [endpoint, workspace]
  );

  const copy = async (variant: 'header' | 'bearer') => {
    try {
      await navigator.clipboard.writeText(variant === 'header' ? headerConfig : bearerConfig);
      setCopied(variant);
      setTimeout(() => setCopied(null), 1500);
    } catch {
      /* clipboard unavailable — ignore */
    }
  };

  return (
    <div
      className="space-y-2 border-t border-[rgb(var(--border-subtle))] pt-4"
      data-testid="wizard-connect-preview"
    >
      <span className="text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
        How a client connects
      </span>
      <p className="text-xs text-[rgb(var(--muted))]">
        Route here with either the OAuth approval flow, a Bearer API key, or this workspace header.
      </p>
      <pre
        className="overflow-x-auto rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] p-3 font-mono text-[11px] leading-relaxed"
        data-testid="wizard-connect-preview-json"
      >
        {headerConfig}
      </pre>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => void copy('header')}
          className="inline-flex items-center gap-1 rounded-md border border-[rgb(var(--border))] px-2 py-1 text-[11px] font-medium text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))]"
          data-testid="wizard-connect-preview-copy"
        >
          {copied === 'header' ? (
            <Check className="h-3 w-3 text-green-600" />
          ) : (
            <Copy className="h-3 w-3" />
          )}
          {copied === 'header' ? COPIED_LABEL : COPY_CONFIG_LABEL}
        </button>
        <button
          type="button"
          onClick={() => void copy('bearer')}
          className="inline-flex items-center gap-1 rounded-md border border-[rgb(var(--border))] px-2 py-1 text-[11px] font-medium text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))]"
          data-testid="wizard-connect-preview-copy-bearer"
        >
          {copied === 'bearer' ? (
            <Check className="h-3 w-3 text-green-600" />
          ) : (
            <Copy className="h-3 w-3" />
          )}
          {copied === 'bearer' ? COPIED_LABEL : COPY_CONFIG_BEARER_LABEL}
        </button>
      </div>
      {authDisabled === false && (
        <p className="rounded-lg border border-amber-200 bg-amber-50 p-2.5 text-xs text-amber-800 dark:border-amber-800/60 dark:bg-amber-900/20 dark:text-amber-300">
          Authentication is on — use the <strong>Copy with Bearer</strong> variant and swap in this
          client&apos;s API key.
        </p>
      )}
    </div>
  );
}
