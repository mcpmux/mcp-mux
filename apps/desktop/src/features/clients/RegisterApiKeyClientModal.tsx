/**
 * Register API-key client modal.
 *
 * Creates a pre-approved inbound client authenticated by a long-lived API key —
 * no browser/OAuth consent. This is the secure way to connect a headless,
 * remote, or CI client (or any client reaching the gateway over the network,
 * where the `mcpmux://` consent deep link can't complete).
 *
 * The generated key is shown ONCE. McpMux stores only its SHA-256 hash and can
 * never display it again — if lost, revoke it and issue a new one.
 */

import { useState } from 'react';
import { AlertTriangle, Check, Copy, KeyRound, Loader2, ShieldCheck, X } from 'lucide-react';
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle } from '@mcpmux/ui';
import { registerApiKeyClient, type RegisteredApiKeyClient } from '@/lib/api/gateway';

interface RegisterApiKeyClientModalProps {
  onClose: () => void;
  /** Called once the client + key are created, so the page can refresh. */
  onRegistered: (client: RegisteredApiKeyClient) => void;
}

/**
 * Modal flow to register a preregistered client and show its one-time API key.
 */
export function RegisterApiKeyClientModal({
  onClose,
  onRegistered,
}: RegisterApiKeyClientModalProps) {
  const [name, setName] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<RegisteredApiKeyClient | null>(null);
  const [copied, setCopied] = useState(false);

  const handleGenerate = async () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Give the client a name so you can recognise it later.');
      return;
    }
    setIsSubmitting(true);
    setError(null);
    try {
      const client = await registerApiKeyClient(trimmed);
      setResult(client);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleCopy = async () => {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.apiKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard can be unavailable; the field is selectable as a fallback.
    }
  };

  const handleDone = () => {
    if (result) onRegistered(result);
    onClose();
  };

  return (
    <div
      className="animate-in fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4 backdrop-blur-[2px] duration-200"
      onClick={result ? undefined : onClose}
    >
      <Card className="w-full max-w-lg shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <CardHeader className="relative">
          <button
            onClick={result ? handleDone : onClose}
            className="absolute right-4 top-4 rounded-lg p-1.5 text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface))] hover:text-[rgb(var(--text))]"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
          <div className="mb-2 flex h-11 w-11 items-center justify-center rounded-xl bg-[rgb(var(--accent))]/10">
            <KeyRound className="h-5 w-5 text-[rgb(var(--accent))]" />
          </div>
          <CardTitle data-testid="register-api-key-title">
            {result ? 'API key created' : 'Register client (API key)'}
          </CardTitle>
          <CardDescription>
            {result
              ? 'Copy the key now — this is the only time it will be shown.'
              : 'A pre-authorised client that connects with an API key instead of browser approval. Use this for headless, CI, or remote clients reaching the gateway over the network.'}
          </CardDescription>
        </CardHeader>

        <CardContent className="space-y-5">
          {result ? (
            <>
              <div>
                <label className="mb-1.5 block text-sm font-medium">API key</label>
                <div className="flex items-stretch gap-2">
                  <code
                    data-testid="register-api-key-value"
                    className="flex-1 select-all break-all rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3 py-2.5 font-mono text-sm"
                  >
                    {result.apiKey}
                  </code>
                  <Button variant="secondary" size="md" onClick={handleCopy}>
                    {copied ? (
                      <Check className="h-4 w-4 text-emerald-500" />
                    ) : (
                      <Copy className="h-4 w-4" />
                    )}
                  </Button>
                </div>
              </div>

              <div className="flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-3.5 dark:border-amber-700/60 dark:bg-amber-900/20">
                <AlertTriangle className="mt-0.5 h-5 w-5 flex-shrink-0 text-amber-600 dark:text-amber-400" />
                <p className="text-sm text-amber-800 dark:text-amber-200">
                  Store this key in your client now. McpMux keeps only a hash and{' '}
                  <strong>cannot show it again</strong>. If you lose it, revoke the key and create a
                  new one.
                </p>
              </div>

              <div className="rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-3.5">
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wide text-[rgb(var(--muted))]">
                  How the client authenticates
                </p>
                <code className="block break-all font-mono text-xs text-[rgb(var(--text))]">
                  Authorization: Bearer {result.keyPrefix}…
                </code>
              </div>

              <div className="flex justify-end">
                <Button variant="primary" size="md" onClick={handleDone}>
                  Done
                </Button>
              </div>
            </>
          ) : (
            <>
              <div>
                <label htmlFor="api-key-client-name" className="mb-1.5 block text-sm font-medium">
                  Client name
                </label>
                <input
                  id="api-key-client-name"
                  data-testid="register-api-key-name"
                  type="text"
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !isSubmitting) void handleGenerate();
                  }}
                  placeholder="e.g. CI runner, my-laptop, prod-bot"
                  className="w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3.5 py-2.5 text-sm transition-all focus:border-[rgb(var(--accent))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--accent))]/40"
                />
              </div>

              <div className="flex items-start gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-3.5">
                <ShieldCheck className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--accent))]" />
                <p className="text-xs text-[rgb(var(--muted))]">
                  The key is generated on this machine, shown once, and stored only as a SHA-256
                  hash. The client then sends it as a Bearer token — no approval prompt needed.
                </p>
              </div>

              {error && (
                <p
                  className="text-sm text-red-600 dark:text-red-400"
                  data-testid="register-api-key-error"
                >
                  {error}
                </p>
              )}

              <div className="flex justify-end gap-2">
                <Button variant="ghost" size="md" onClick={onClose} disabled={isSubmitting}>
                  Cancel
                </Button>
                <Button
                  variant="primary"
                  size="md"
                  onClick={handleGenerate}
                  disabled={isSubmitting}
                  data-testid="register-api-key-generate"
                >
                  {isSubmitting ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      Generating…
                    </>
                  ) : (
                    <>
                      <KeyRound className="mr-2 h-4 w-4" />
                      Generate key
                    </>
                  )}
                </Button>
              </div>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
