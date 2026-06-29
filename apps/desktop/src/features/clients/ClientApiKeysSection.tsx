/**
 * API keys for a preregistered (API-key) client — rendered in the client side
 * panel. Lists the client's keys (prefix + metadata, never the secret), and
 * lets the user revoke a key or mint a new one (rotation). A freshly-minted key
 * is shown ONCE inline.
 */

import { useEffect, useState } from 'react';
import { AlertTriangle, Check, Copy, Loader2, Plus, Trash2 } from 'lucide-react';
import { Button } from '@mcpmux/ui';
import {
  createClientApiKey,
  listClientApiKeys,
  revokeClientApiKey,
  type ApiKeyInfo,
  type RegisteredApiKeyClient,
} from '@/lib/api/gateway';

interface ClientApiKeysSectionProps {
  clientId: string;
  onError: (title: string, body?: string) => void;
  onSuccess: (title: string, body?: string) => void;
}

export function ClientApiKeysSection({ clientId, onError, onSuccess }: ClientApiKeysSectionProps) {
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isCreating, setIsCreating] = useState(false);
  const [revokingId, setRevokingId] = useState<string | null>(null);
  const [newKey, setNewKey] = useState<RegisteredApiKeyClient | null>(null);
  const [copied, setCopied] = useState(false);

  const load = async () => {
    setIsLoading(true);
    try {
      setKeys(await listClientApiKeys(clientId));
    } catch (e) {
      onError('Failed to load API keys', e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clientId]);

  const handleCreate = async () => {
    setIsCreating(true);
    try {
      const issued = await createClientApiKey(clientId);
      setNewKey(issued);
      setCopied(false);
      await load();
    } catch (e) {
      onError('Failed to create key', e instanceof Error ? e.message : String(e));
    } finally {
      setIsCreating(false);
    }
  };

  const handleCopy = async () => {
    if (!newKey) return;
    try {
      await navigator.clipboard.writeText(newKey.apiKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard can be unavailable; the field is selectable as a fallback.
    }
  };

  const handleRevoke = async (keyId: string) => {
    setRevokingId(keyId);
    try {
      await revokeClientApiKey(keyId);
      onSuccess('Key revoked', 'It can no longer authenticate.');
      await load();
    } catch (e) {
      onError('Failed to revoke key', e instanceof Error ? e.message : String(e));
    } finally {
      setRevokingId(null);
    }
  };

  const liveKeys = keys.filter((k) => !k.revoked);

  return (
    <section>
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-[rgb(var(--muted))]">
          API keys
        </h3>
        <Button
          size="sm"
          variant="ghost"
          onClick={handleCreate}
          disabled={isCreating}
          data-testid="client-new-api-key"
        >
          {isCreating ? (
            <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
          ) : (
            <Plus className="mr-1.5 h-3.5 w-3.5" />
          )}
          New key
        </Button>
      </div>

      {newKey && (
        <div className="mb-3 rounded-xl border border-amber-300 bg-amber-50 p-3 dark:border-amber-700/60 dark:bg-amber-900/20">
          <div className="mb-2 flex items-start gap-2">
            <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
            <p className="text-xs text-amber-800 dark:text-amber-200">
              Copy this key now — it won&apos;t be shown again.
            </p>
          </div>
          <div className="flex items-stretch gap-2">
            <code className="flex-1 select-all break-all rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-2.5 py-2 font-mono text-xs">
              {newKey.apiKey}
            </code>
            <Button size="sm" variant="secondary" onClick={handleCopy}>
              {copied ? (
                <Check className="h-3.5 w-3.5 text-emerald-500" />
              ) : (
                <Copy className="h-3.5 w-3.5" />
              )}
            </Button>
          </div>
        </div>
      )}

      {isLoading ? (
        <div className="flex justify-center py-4">
          <Loader2 className="h-5 w-5 animate-spin text-[rgb(var(--muted))]" />
        </div>
      ) : liveKeys.length === 0 ? (
        <p className="rounded-lg border border-dashed border-[rgb(var(--border))] px-3 py-3 text-center text-xs text-[rgb(var(--muted))]">
          No active keys. Create one so this client can authenticate.
        </p>
      ) : (
        <ul className="space-y-2">
          {liveKeys.map((k) => (
            <li
              key={k.keyId}
              className="flex items-center justify-between gap-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2"
            >
              <div className="min-w-0 flex-1">
                <code className="font-mono text-xs">{k.keyPrefix}…</code>
                <p className="mt-0.5 text-[11px] text-[rgb(var(--muted))]">
                  {k.lastUsedAt
                    ? `Last used ${new Date(k.lastUsedAt).toLocaleDateString()}`
                    : 'Never used'}
                </p>
              </div>
              <button
                onClick={() => handleRevoke(k.keyId)}
                disabled={revokingId === k.keyId}
                className="flex-shrink-0 rounded-md p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/20"
                aria-label="Revoke key"
              >
                {revokingId === k.keyId ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Trash2 className="h-4 w-4" />
                )}
              </button>
            </li>
          ))}
        </ul>
      )}

      <p className="mt-2 text-xs text-[rgb(var(--muted))]">
        This client authenticates with an API key as a Bearer token. Keys are stored hashed — revoke
        a leaked one and mint a new key.
      </p>
    </section>
  );
}
