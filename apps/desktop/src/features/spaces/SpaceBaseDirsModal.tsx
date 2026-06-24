import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { FolderPlus, FolderOpen, Loader2, Trash2, X } from 'lucide-react';
import { Button, useToast, ToastContainer } from '@mcpmux/ui';
import {
  addSpaceBaseDir,
  listSpaceBaseDirs,
  removeSpaceBaseDir,
  type Space,
  type SpaceBaseDir,
} from '@/lib/api/spaces';

/**
 * Manage a Space's base directories.
 *
 * A base dir scopes any workspace root opened at or under it to this Space:
 * an unmapped folder there uses this Space's tools, and self-optimize stays
 * in this Space. Longest match wins when base dirs nest, and a folder can
 * belong to only one Space.
 */
export function SpaceBaseDirsModal({
  space,
  onClose,
}: {
  space: Space | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('spaces');
  const [dirs, setDirs] = useState<SpaceBaseDir[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const { toasts, error: showError, dismiss } = useToast();

  const spaceId = space?.id ?? null;

  const load = useCallback(async () => {
    if (!spaceId) return;
    setLoading(true);
    try {
      setDirs(await listSpaceBaseDirs(spaceId));
    } catch (e) {
      showError('Could not load base directories', e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [spaceId, showError]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const handleAdd = async () => {
    if (!spaceId || busy) return;
    let picked: string | string[] | null;
    try {
      picked = await openDialog({ directory: true, multiple: true, title: 'Add base directory' });
    } catch {
      return;
    }
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (paths.length === 0) return; // cancelled — nothing added

    setBusy(true);
    for (const p of paths) {
      try {
        await addSpaceBaseDir(spaceId, p);
      } catch (e) {
        showError('Could not add folder', e instanceof Error ? e.message : String(e));
      }
    }
    await load();
    setBusy(false);
  };

  const handleRemove = async (dir: SpaceBaseDir) => {
    if (busy) return;
    setBusy(true);
    try {
      await removeSpaceBaseDir(dir.id);
      setDirs((prev) => prev.filter((d) => d.id !== dir.id));
    } catch (e) {
      showError('Could not remove folder', e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!space) return null;

  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-2xl border border-[rgb(var(--border))] bg-[rgb(var(--background))] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="space-base-dirs-modal"
      >
        {/* Header */}
        <div className="flex items-start justify-between border-b border-[rgb(var(--border-subtle))] p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] text-xl">
              {space.icon || '🌐'}
            </div>
            <div>
              <h2 className="text-lg font-semibold">Base directories</h2>
              <p className="text-xs text-[rgb(var(--muted))]">
                Scoped to <span className="font-medium">{space.name}</span>
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--foreground))]"
            aria-label="Close"
            data-testid="space-base-dirs-close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Body */}
        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          <p className="mb-4 text-sm text-[rgb(var(--muted))]">
            Folders you open here (or under them) are scoped to this space.
          </p>

          {loading ? (
            <div className="flex items-center justify-center py-8 text-[rgb(var(--muted))]">
              <Loader2 className="h-5 w-5 animate-spin" />
            </div>
          ) : (
            <>
              {dirs.length > 0 && (
                <ul className="mb-3 space-y-2" data-testid="space-base-dirs-list">
                  {dirs.map((dir) => (
                    <li
                      key={dir.id}
                      className="flex items-center gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-3 py-2.5"
                    >
                      <FolderOpen className="text-primary-500 h-4 w-4 flex-shrink-0" />
                      <span
                        className="min-w-0 flex-1 truncate font-mono text-xs text-[rgb(var(--foreground))]"
                        title={dir.path}
                      >
                        {dir.path}
                      </span>
                      <button
                        onClick={() => handleRemove(dir)}
                        disabled={busy}
                        className="flex-shrink-0 rounded-lg p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-red-50 hover:text-red-500 disabled:opacity-50 dark:hover:bg-red-900/20"
                        title={t('baseDirs.removeTitle')}
                        aria-label={`Remove ${dir.path}`}
                        data-testid={`remove-base-dir-${dir.id}`}
                      >
                        <Trash2 className="h-4 w-4" />
                      </button>
                    </li>
                  ))}
                </ul>
              )}

              {/* Add row — a clearly optional action, not the only way out. */}
              <button
                type="button"
                onClick={handleAdd}
                disabled={busy}
                className="hover:border-primary-400 hover:text-primary-600 dark:hover:text-primary-400 flex w-full items-center justify-center gap-2 rounded-xl border border-dashed border-[rgb(var(--border))] px-3 py-3 text-sm font-medium text-[rgb(var(--muted))] transition-colors disabled:opacity-50"
                data-testid="add-base-dir-btn"
              >
                {busy ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <FolderPlus className="h-4 w-4" />
                )}
                Add folder…
              </button>

              {dirs.length === 0 && !busy && (
                <p className="mt-3 text-center text-xs text-[rgb(var(--muted))]">
                  No base directories yet.
                </p>
              )}
            </>
          )}
        </div>

        {/* Footer — close without adding. */}
        <div className="flex justify-end border-t border-[rgb(var(--border-subtle))] p-4">
          <Button
            variant="primary"
            onClick={onClose}
            className="px-6"
            data-testid="space-base-dirs-done"
          >
            Done
          </Button>
        </div>
      </div>
      <ToastContainer toasts={toasts} onClose={dismiss} />
    </div>
  );
}
