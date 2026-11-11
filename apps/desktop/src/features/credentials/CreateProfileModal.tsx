import { useState, useEffect, useRef } from 'react';
import { Plus, Loader2, X, Eye, EyeOff, Trash2, KeyRound } from 'lucide-react';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardContent,
  useToast,
  ToastContainer,
} from '@mcpmux/ui';
import { createCredentialProfile, type CredentialProfile } from '@/lib/api/credentials';

interface CreateProfileModalProps {
  open: boolean;
  onClose: () => void;
  onCreated?: (profile: CredentialProfile) => void;
}

export function CreateProfileModal({ open, onClose, onCreated }: CreateProfileModalProps) {
  const { toasts, success, error: showError, dismiss } = useToast();

  const [name, setName] = useState('');
  const [serverId, setServerId] = useState('');
  const [serverName, setServerName] = useState('');
  const [keyValues, setKeyValues] = useState<Array<{ key: string; value: string; show?: boolean }>>(
    [{ key: '', value: '', show: false }]
  );
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setName('');
    setServerId('');
    setServerName('');
    setKeyValues([{ key: '', value: '', show: false }]);
    setFormError(null);
    setIsSubmitting(false);
    const t = setTimeout(() => nameRef.current?.focus(), 50);
    return () => clearTimeout(t);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  const addKeyValueRow = () => {
    setKeyValues((prev) => [...prev, { key: '', value: '', show: false }]);
  };

  const removeKeyValueRow = (index: number) => {
    setKeyValues((prev) => prev.filter((_, i) => i !== index));
  };

  const updateKeyValue = (
    index: number,
    field: 'key' | 'value' | 'show',
    val: string | boolean
  ) => {
    setKeyValues((prev) => prev.map((item, i) => (i === index ? { ...item, [field]: val } : item)));
  };

  const handleCreate = async () => {
    if (!name.trim()) {
      setFormError('Please enter a profile name.');
      return;
    }
    if (!serverId.trim()) {
      setFormError('Please enter a target server definition ID (e.g. "github", "com.openai").');
      return;
    }

    const values: Record<string, string> = {};
    for (const item of keyValues) {
      if (item.key.trim()) {
        values[item.key.trim()] = item.value;
      }
    }

    try {
      setIsSubmitting(true);
      setFormError(null);
      const profile = await createCredentialProfile(
        name.trim(),
        serverId.trim(),
        serverName.trim() || null,
        values
      );
      success('Profile Created', `Credential profile "${profile.name}" created successfully.`);
      onCreated?.(profile);
      onClose();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setFormError(msg);
      showError('Failed to create profile', msg);
    } finally {
      setIsSubmitting(false);
    }
  };

  if (!open) return null;

  return (
    <>
      <ToastContainer toasts={toasts} onClose={dismiss} />
      <div
        className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
        data-testid="create-credential-profile-modal-overlay"
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        <Card
          className="animate-in fade-in zoom-in-95 w-full max-w-lg border-[rgb(var(--border))] shadow-2xl duration-200"
          data-testid="create-credential-profile-modal"
        >
          <CardHeader className="border-b border-[rgb(var(--border-subtle))] pb-4">
            <CardTitle className="flex items-center justify-between">
              <span className="flex items-center gap-2 text-lg">
                <KeyRound className="h-5 w-5 text-[rgb(var(--primary))]" />
                New Credential Profile
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
          <CardContent className="max-h-[75vh] space-y-4 overflow-y-auto pt-4">
            {formError ? (
              <div className="rounded-lg border border-red-500/20 bg-red-500/10 p-3 text-xs text-red-600 dark:text-red-400">
                {formError}
              </div>
            ) : null}

            <div>
              <label className="mb-1 block text-xs font-semibold text-[rgb(var(--foreground))]">
                Profile Name <span className="text-red-500">*</span>
              </label>
              <input
                ref={nameRef}
                type="text"
                placeholder="e.g. Work GitHub, Personal OpenAI"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 text-sm text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                data-testid="create-profile-name-input"
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="mb-1 block text-xs font-semibold text-[rgb(var(--foreground))]">
                  Server ID <span className="text-red-500">*</span>
                </label>
                <input
                  type="text"
                  placeholder="e.g. com.github-mcp, openai"
                  value={serverId}
                  onChange={(e) => setServerId(e.target.value)}
                  className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 font-mono text-xs text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                  data-testid="create-profile-server-id-input"
                />
              </div>
              <div>
                <label className="mb-1 block text-xs font-semibold text-[rgb(var(--foreground))]">
                  Server Display Name
                </label>
                <input
                  type="text"
                  placeholder="e.g. GitHub MCP"
                  value={serverName}
                  onChange={(e) => setServerName(e.target.value)}
                  className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2 text-sm text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                  data-testid="create-profile-server-name-input"
                />
              </div>
            </div>

            <div>
              <div className="mb-2 flex items-center justify-between">
                <label className="text-xs font-semibold text-[rgb(var(--foreground))]">
                  Secrets &amp; Environment Variables
                </label>
                <button
                  type="button"
                  onClick={addKeyValueRow}
                  className="flex items-center gap-1 text-xs font-medium text-[rgb(var(--primary))] hover:underline"
                >
                  <Plus className="h-3.5 w-3.5" />
                  Add Variable
                </button>
              </div>

              <div className="space-y-2">
                {keyValues.map((item, idx) => (
                  <div key={idx} className="flex items-center gap-2">
                    <input
                      type="text"
                      placeholder="KEY_NAME (e.g. API_KEY)"
                      value={item.key}
                      onChange={(e) => updateKeyValue(idx, 'key', e.target.value)}
                      className="w-1/3 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-2.5 py-1.5 font-mono text-xs text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                    />
                    <div className="relative flex-1">
                      <input
                        type={item.show ? 'text' : 'password'}
                        placeholder="Secret value"
                        value={item.value}
                        onChange={(e) => updateKeyValue(idx, 'value', e.target.value)}
                        className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-2.5 py-1.5 pr-8 font-mono text-xs text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--primary))]/40"
                      />
                      <button
                        type="button"
                        onClick={() => updateKeyValue(idx, 'show', !item.show)}
                        className="absolute right-2 top-1/2 -translate-y-1/2 text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]"
                        tabIndex={-1}
                      >
                        {item.show ? (
                          <EyeOff className="h-3.5 w-3.5" />
                        ) : (
                          <Eye className="h-3.5 w-3.5" />
                        )}
                      </button>
                    </div>
                    {keyValues.length > 1 ? (
                      <button
                        type="button"
                        onClick={() => removeKeyValueRow(idx)}
                        className="p-1 text-[rgb(var(--muted))] hover:text-red-500"
                        title="Remove variable"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    ) : null}
                  </div>
                ))}
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 border-t border-[rgb(var(--border-subtle))] pt-4">
              <Button variant="ghost" size="sm" onClick={onClose} disabled={isSubmitting}>
                Cancel
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={handleCreate}
                disabled={isSubmitting || !name.trim() || !serverId.trim()}
                data-testid="create-profile-submit-btn"
              >
                {isSubmitting ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    Saving...
                  </>
                ) : (
                  'Create Profile'
                )}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </>
  );
}
