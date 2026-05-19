import { useEffect, useState } from 'react';
import { Loader2, Save, Trash2, X } from 'lucide-react';
import { Button, useConfirm, useToast, ToastContainer } from '@mcpmux/ui';
import type { Space } from '@/lib/api/spaces';
import { deleteSpace, updateSpace } from '@/lib/api/spaces';

const SPACE_ICON_OPTIONS = ['🌐', '💻', '🚀', '🏢', '🏠', '🔒', '🧪', '📦'] as const;

export interface SpacePanelProps {
  space: Space;
  onClose: () => void;
  onSaved: (space: Space) => void;
  onDeleted: (id: string) => void;
}

/**
 * Slide-out panel for editing a Space's display metadata (name, icon, description).
 */
export function SpacePanel({ space, onClose, onSaved, onDeleted }: SpacePanelProps) {
  const [name, setName] = useState(space.name);
  const [icon, setIcon] = useState(space.icon ?? '🌐');
  const [description, setDescription] = useState(space.description ?? '');
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { toasts, success, error: showError, dismiss } = useToast();
  const { confirm, ConfirmDialogElement } = useConfirm();

  useEffect(() => {
    setName(space.name);
    setIcon(space.icon ?? '🌐');
    setDescription(space.description ?? '');
    setError(null);
  }, [space]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  /**
   * Persist name, icon, and description to the backend and notify the parent.
   */
  const handleSave = async () => {
    const trimmedName = name.trim();
    if (!trimmedName) {
      setError('Name is required.');
      return;
    }

    setIsSaving(true);
    setError(null);
    try {
      const updated = await updateSpace(space.id, {
        name: trimmedName,
        icon: icon.trim() || undefined,
        description: description.trim() || undefined,
      });
      success('Space updated', `"${updated.name}" has been saved`);
      onSaved(updated);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      showError('Failed to save space', msg);
    } finally {
      setIsSaving(false);
    }
  };

  /**
   * Delete this Space after confirmation (default Space cannot be deleted).
   */
  const handleDelete = async () => {
    const ok = await confirm({
      title: 'Delete workspace',
      message: `Are you sure you want to delete "${space.name}"? This action cannot be undone.`,
      confirmLabel: 'Delete',
      variant: 'danger',
    });
    if (!ok) return;

    setIsDeleting(true);
    try {
      await deleteSpace(space.id);
      success('Space deleted', `"${space.name}" has been deleted`);
      onDeleted(space.id);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      showError('Failed to delete space', msg);
    } finally {
      setIsDeleting(false);
    }
  };

  const hasChanges =
    name.trim() !== space.name ||
    (icon.trim() || '🌐') !== (space.icon ?? '🌐') ||
    description.trim() !== (space.description ?? '');

  return (
    <div
      className="fixed right-0 top-0 bottom-0 w-full max-w-[480px] min-w-[420px] bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-50"
      data-testid="space-panel"
    >
      <ToastContainer toasts={toasts} onClose={dismiss} />
      {ConfirmDialogElement}

      <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3 flex-1 min-w-0">
            <div className="w-11 h-11 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg text-xl border border-[rgb(var(--border-subtle))] flex-shrink-0">
              {icon}
            </div>
            <div className="flex-1 min-w-0">
              <h2 className="text-lg font-bold truncate">{space.name}</h2>
              {space.is_default && (
                <span className="inline-flex mt-1 px-2 py-0.5 rounded-full text-xs font-medium bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400">
                  Default
                </span>
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
            aria-label="Close panel"
            data-testid="space-panel-close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6 space-y-5">
        {error && (
          <p className="text-sm text-red-600 dark:text-red-400" role="alert">
            {error}
          </p>
        )}

        <div>
          <label className="block text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))] mb-2">
            Icon
          </label>
          <div className="flex gap-2 flex-wrap">
            {SPACE_ICON_OPTIONS.map((emoji) => (
              <button
                key={emoji}
                type="button"
                onClick={() => setIcon(emoji)}
                className={`w-10 h-10 flex items-center justify-center rounded-lg text-xl border transition-all ${
                  icon === emoji
                    ? 'bg-primary-50 dark:bg-primary-900/20 border-primary-500 ring-2 ring-primary-500/20'
                    : 'bg-[rgb(var(--surface))] border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))]'
                }`}
              >
                {emoji}
              </button>
            ))}
          </div>
        </div>

        <div>
          <label className="block text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))] mb-2">
            Name *
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="space-panel-name"
          />
        </div>

        <div>
          <label className="block text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))] mb-2">
            Description
          </label>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Optional description for this workspace"
            className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="space-panel-description"
          />
        </div>
      </div>

      <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))] flex flex-col gap-2">
        <Button
          variant="primary"
          size="md"
          onClick={() => void handleSave()}
          disabled={isSaving || !name.trim() || !hasChanges}
          className="w-full"
          data-testid="space-panel-save"
        >
          {isSaving ? (
            <Loader2 className="h-4 w-4 animate-spin mr-2" />
          ) : (
            <Save className="h-4 w-4 mr-2" />
          )}
          Save Changes
        </Button>
        {!space.is_default && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void handleDelete()}
            disabled={isDeleting || isSaving}
            className="w-full text-red-600 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
            data-testid="space-panel-delete"
          >
            <Trash2 className="h-4 w-4 mr-2" />
            Delete Space
          </Button>
        )}
      </div>
    </div>
  );
}
