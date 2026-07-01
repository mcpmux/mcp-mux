import { useState, useEffect, useRef } from 'react';
import { Plus, Loader2, X } from 'lucide-react';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardContent,
  useToast,
  ToastContainer,
} from '@mcpmux/ui';
import { useAppStore } from '@/stores';
import { createSpace, type Space } from '@/lib/api/spaces';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';

const DEFAULT_ICON = '🌐';

/** Curated icon set — three rows of eight in the picker grid. */
const ICON_CHOICES = [
  '🌐', '💼', '🏢', '🏠', '🚀', '🎯', '📦', '🗂️',
  '💻', '🐳', '☁️', '🗄️', '🔌', '⚙️', '🤖', '🧪',
  '🔒', '🔥', '⭐', '✨', '🎨', '📚', '🌱', '☕',
];

interface CreateSpaceModalProps {
  open: boolean;
  onClose: () => void;
  /** Called after a Space is created (and added to the store) — e.g. to
   *  switch the viewed Space. The Space is already persisted + in the store. */
  onCreated?: (space: Space) => void;
}

/**
 * Shared "Create Space" dialog used by both the Spaces page and the sidebar
 * SpaceSwitcher, so naming + icon selection feel identical everywhere.
 * Owns the create call, store update, and success/error toasts; the parent
 * only decides when to open it and what to do with the new Space.
 */
export function CreateSpaceModal({ open, onClose, onCreated }: CreateSpaceModalProps) {
  const addSpace = useAppStore((state) => state.addSpace);
  const { toasts, success, error: showError, dismiss } = useToast();

  const [name, setName] = useState('');
  const [icon, setIcon] = useState(DEFAULT_ICON);
  const [isCreating, setIsCreating] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  // Reset to a clean slate each time the dialog opens, and focus the name.
  useEffect(() => {
    if (!open) return;
    setName('');
    setIcon(DEFAULT_ICON);
    setIsCreating(false);
    const t = setTimeout(() => nameRef.current?.focus(), 50);
    return () => clearTimeout(t);
  }, [open]);

  // Escape closes the dialog.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  const trimmed = name.trim();

  const handleCreate = async () => {
    if (!trimmed || isCreating) return;
    setIsCreating(true);
    try {
      const space = await createSpace(trimmed, icon.trim() || DEFAULT_ICON);
      addSpace(space);
      success('Space created', `"${space.name}" has been created`);
      onCreated?.(space);
      onClose();
    } catch (e) {
      showError('Failed to create space', e instanceof Error ? e.message : String(e));
    } finally {
      setIsCreating(false);
    }
  };

  if (!open) return null;

  return (
    <>
      <ToastContainer toasts={toasts} onClose={dismiss} />
      <div
        className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/50 p-4"
        data-testid="create-space-modal-overlay"
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        <Card
          className="animate-in fade-in zoom-in-95 w-full max-w-md shadow-2xl duration-200"
          data-testid="create-space-modal"
        >
          <CardHeader>
            <CardTitle className="flex items-center justify-between">
              <span className="flex items-center gap-2">
                <Plus className="h-5 w-5" />
                Create Space
              </span>
              <button
                onClick={onClose}
                className="rounded p-1 hover:bg-[rgb(var(--surface-hover))]"
                aria-label="Close"
                data-testid="create-space-cancel-x"
              >
                <X className="h-4 w-4" />
              </button>
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Live preview */}
            <div className="flex items-center gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-3">
              <span
                className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-lg bg-[rgb(var(--primary))/12] text-2xl"
                data-testid="create-space-preview-icon"
              >
                {icon || DEFAULT_ICON}
              </span>
              <div className="min-w-0">
                <div className="truncate text-base font-semibold">{trimmed || 'New Space'}</div>
                <div className="text-xs text-[rgb(var(--muted))]">Preview</div>
              </div>
            </div>

            {/* Name */}
            <div>
              <label className="mb-1.5 block text-sm font-medium">Name</label>
              <input
                ref={nameRef}
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleCreate();
                }}
                placeholder="e.g. Personal, Work, Project X"
                className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2.5 focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]"
                data-testid="create-space-name-input"
              />
            </div>

            {/* Icon picker */}
            <div>
              <label className="mb-1.5 block text-sm font-medium">Icon</label>
              <div className="grid grid-cols-8 gap-1.5" data-testid="create-space-icon-grid">
                {ICON_CHOICES.map((emoji) => (
                  <button
                    key={emoji}
                    type="button"
                    onClick={() => setIcon(emoji)}
                    aria-pressed={icon === emoji}
                    className={`flex h-9 w-9 items-center justify-center rounded-lg border text-lg transition-all ${
                      icon === emoji
                        ? 'border-[rgb(var(--primary))] bg-[rgb(var(--primary))/12] ring-2 ring-[rgb(var(--primary))/20]'
                        : 'border-[rgb(var(--border))] bg-[rgb(var(--surface))] hover:bg-[rgb(var(--surface-hover))]'
                    }`}
                    data-testid={`create-space-icon-${emoji}`}
                  >
                    {emoji}
                  </button>
                ))}
              </div>
              <div className="mt-2 flex items-center gap-2">
                <span className="text-xs text-[rgb(var(--muted))]">Or pick any</span>
                <EmojiPickerButton
                  value={icon}
                  onChange={setIcon}
                  testId="create-space-icon-custom"
                />
              </div>
            </div>

            <div className="flex gap-3 pt-1">
              <Button
                variant="ghost"
                onClick={onClose}
                className="flex-1"
                data-testid="create-space-cancel-btn"
              >
                Cancel
              </Button>
              <Button
                variant="primary"
                onClick={handleCreate}
                disabled={isCreating || !trimmed}
                className="flex-1"
                data-testid="create-space-submit-btn"
              >
                {isCreating ? <Loader2 className="h-4 w-4 animate-spin" /> : 'Create Space'}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </>
  );
}

export default CreateSpaceModal;
