import { useCallback, useEffect, useState } from 'react';
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
 * an unmapped folder there falls back to this Space's Starter set, and the
 * self-optimize meta-tools + mapping popup restrict to this Space. Longest
 * match wins when base dirs nest across Spaces, and a folder can belong to
 * only one Space.
 */
export function SpaceBaseDirsModal({
  space,
  onClose,
}: {
  space: Space | null;
  onClose: () => void;
}) {
  const [dirs, setDirs] = useState<SpaceBaseDir[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const { toasts, success, error: showError, dismiss } = useToast();

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
    if (paths.length === 0) return;

    setBusy(true);
    let added = 0;
    for (const p of paths) {
      try {
        await addSpaceBaseDir(spaceId, p);
        added++;
      } catch (e) {
        showError('Could not add folder', e instanceof Error ? e.message : String(e));
      }
    }
    await load();
    setBusy(false);
    if (added > 0) {
      success(
        added === 1 ? 'Base directory added' : `${added} base directories added`,
        'Folders here are now scoped to this space.'
      );
    }
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
        className="animate-slide-up flex max-h-[80vh] w-full max-w-lg flex-col rounded-2xl border border-[rgb(var(--border))] bg-[rgb(var(--background))] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="space-base-dirs-modal"
      >
        <div className="flex items-start justify-between border-b border-[rgb(var(--border-subtle))] p-5">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] text-xl">
              {space.icon || '🌐'}
            </div>
            <div>
              <h2 className="text-lg font-semibold">Base directories</h2>
              <p className="text-xs text-[rgb(var(--muted))]">
                Folders scoped to <span className="font-medium">{space.name}</span>
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--foreground))]"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          <p className="mb-4 text-sm text-[rgb(var(--muted))]">
            Any folder you open here (or under it) is scoped to this space — it uses this
            space&apos;s tools by default, and self-optimize only sees this space. The most specific
            base directory wins, and a folder can belong to only one space.
          </p>

          {loading ? (
            <div className="flex items-center justify-center py-10 text-[rgb(var(--muted))]">
              <Loader2 className="h-5 w-5 animate-spin" />
            </div>
          ) : dirs.length === 0 ? (
            <div className="rounded-xl border border-dashed border-[rgb(var(--border))] px-4 py-8 text-center text-sm text-[rgb(var(--muted))]">
              No base directories yet. Add one to scope its folders to this space.
            </div>
          ) : (
            <ul className="space-y-2" data-testid="space-base-dirs-list">
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
                    title="Remove base directory"
                    data-testid={`remove-base-dir-${dir.id}`}
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="border-t border-[rgb(var(--border-subtle))] p-5">
          <Button
            variant="primary"
            className="w-full"
            onClick={handleAdd}
            disabled={busy}
            data-testid="add-base-dir-btn"
          >
            {busy ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <FolderPlus className="mr-2 h-4 w-4" />
            )}
            Add folder…
          </Button>
        </div>
      </div>
      <ToastContainer toasts={toasts} onClose={dismiss} />
    </div>
  );
}
