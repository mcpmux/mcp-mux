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
import { Check, ChevronDown, FolderOpen, Loader2, Sparkles, X } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import { createWorkspaceBinding } from '@/lib/api/workspaceBindings';
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

export function WorkspaceBindingSheet() {
  const { t } = useTranslation('workspaces');
  const [payload, setPayload] = useState<WorkspaceNeedsBindingPayload | null>(null);
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
    });
  }, [subscribe]);

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
        // Pre-select the auto-seeded Starter as a sensible default in
        // the sheet — operator can change it before approving.
        const seedFs = visible.find(isStarterFeatureSet) ?? visible[0];
        if (seedFs) setSelectedFsId(seedFs.id);
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
              disabled={saving}
            >
              {t('sheet.notNow')}
            </Button>
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
          </div>
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
        </div>
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
