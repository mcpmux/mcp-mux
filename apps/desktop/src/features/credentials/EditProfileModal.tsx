import { useState, useEffect, useRef } from 'react';
import { Loader2, X, Edit3 } from 'lucide-react';
import { Button, Card, CardHeader, CardTitle, CardContent } from '@mcpmux/ui';
import { updateCredentialProfile, type CredentialProfile } from '@/lib/api/credentials';

interface EditProfileModalProps {
  profile: CredentialProfile | null;
  open: boolean;
  onClose: () => void;
  onUpdated?: (profile: CredentialProfile) => void;
}

export function EditProfileModal({ profile, open, onClose, onUpdated }: EditProfileModalProps) {
  const [name, setName] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open || !profile) return;
    setName(profile.name);
    setError(null);
    setIsSubmitting(false);
    const t = setTimeout(() => nameRef.current?.focus(), 50);
    return () => clearTimeout(t);
  }, [open, profile]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  const handleSave = async () => {
    if (!profile) return;
    if (!name.trim()) {
      setError('Profile name cannot be empty.');
      return;
    }

    try {
      setIsSubmitting(true);
      setError(null);
      const updated = await updateCredentialProfile(profile.id, name.trim(), profile.values);
      onUpdated?.(updated);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  if (!open || !profile) return null;

  return (
    <div
      className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <Card className="animate-in fade-in zoom-in-95 w-full max-w-md border-[rgb(var(--border))] shadow-2xl duration-200">
        <CardHeader className="border-b border-[rgb(var(--border-subtle))] pb-4">
          <CardTitle className="flex items-center justify-between">
            <span className="flex items-center gap-2 text-lg">
              <Edit3 className="h-5 w-5 text-[rgb(var(--primary))]" />
              Rename Profile
            </span>
            <button
              onClick={onClose}
              className="rounded p-1 hover:bg-[rgb(var(--surface-hover))]"
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4 pt-4">
          {error ? (
            <div className="rounded-lg border border-red-500/20 bg-red-500/10 p-3 text-xs text-red-600 dark:text-red-400">
              {error}
            </div>
          ) : null}

          <div>
            <label className="mb-1 block text-xs font-semibold text-[rgb(var(--foreground))]">
              Profile Name
            </label>
            <input
              ref={nameRef}
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 text-sm text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
            />
          </div>

          <div className="flex items-center justify-end gap-2 border-t border-[rgb(var(--border-subtle))] pt-2">
            <Button variant="ghost" size="sm" onClick={onClose} disabled={isSubmitting}>
              Cancel
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleSave}
              disabled={isSubmitting || !name.trim() || name.trim() === profile.name}
            >
              {isSubmitting ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Saving...
                </>
              ) : (
                'Save'
              )}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
