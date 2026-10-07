import { useEffect, useRef, useState } from 'react';
import { Loader2, Pencil, X } from 'lucide-react';
import { Button, Card, CardContent, CardHeader, CardTitle } from '@mcpmux/ui';
import type { Space } from '@/lib/api/spaces';

interface RenameSpaceModalProps {
  space: Space | null;
  onClose: () => void;
  onRename: (space: Space, name: string) => Promise<void>;
}

/** Small focused dialog for changing a Space's display name. */
export function RenameSpaceModal({ space, onClose, onRename }: RenameSpaceModalProps) {
  const [name, setName] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!space) return;
    setName(space.name);
    setIsSaving(false);
    const timeout = setTimeout(() => {
      nameRef.current?.focus();
      nameRef.current?.select();
    }, 50);
    return () => clearTimeout(timeout);
  }, [space]);

  useEffect(() => {
    if (!space) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [space, onClose]);

  if (!space) return null;

  const trimmedName = name.trim();
  const unchanged = trimmedName === space.name;
  const submit = async () => {
    if (!trimmedName || unchanged || isSaving) return;
    setIsSaving(true);
    try {
      await onRename(space, trimmedName);
      onClose();
    } catch {
      // The page displays the error toast and leaves this dialog open for correction.
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/50 p-4"
      data-testid="rename-space-modal-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <Card className="animate-in fade-in zoom-in-95 w-full max-w-md shadow-2xl duration-200">
        <CardHeader>
          <CardTitle className="flex items-center justify-between">
            <span className="flex items-center gap-2">
              <Pencil className="h-5 w-5" />
              Rename Space
            </span>
            <button
              type="button"
              onClick={onClose}
              className="rounded p-1 hover:bg-[rgb(var(--surface-hover))]"
              aria-label="Close"
              data-testid="rename-space-cancel-x"
            >
              <X className="h-4 w-4" />
            </button>
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div>
            <label htmlFor="rename-space-name" className="mb-1.5 block text-sm font-medium">
              Name
            </label>
            <input
              ref={nameRef}
              id="rename-space-name"
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') void submit();
              }}
              className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2.5 focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]"
              data-testid="rename-space-name-input"
            />
          </div>
          <div className="flex gap-3 pt-1">
            <Button variant="ghost" onClick={onClose} className="flex-1">
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => void submit()}
              disabled={isSaving || !trimmedName || unchanged}
              className="flex-1"
              data-testid="rename-space-submit-btn"
            >
              {isSaving ? <Loader2 className="h-4 w-4 animate-spin" /> : 'Save name'}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
