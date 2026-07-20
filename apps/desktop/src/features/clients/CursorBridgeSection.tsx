/**
 * Global Cursor setup via mcp-remote — one `~/.cursor/mcp.json` entry for all repos.
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Check, Copy, KeyRound, Loader2 } from 'lucide-react';
import { Button, Card, CardContent } from '@mcpmux/ui';
import { registerApiKeyClient } from '@/lib/api/gateway';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import {
  buildCursorBridgeMcpJson,
  CURSOR_BRIDGE_CLIENT_NAME,
} from './cursor-bridge-config.helpers';

interface CursorBridgeSectionProps {
  gatewayUrl: string;
  gatewayRunning: boolean;
  onRegistered?: () => void;
}

/**
 * Mint an API key and render a ready-to-paste global Cursor bridge config.
 */
export function CursorBridgeSection({
  gatewayUrl,
  gatewayRunning,
  onRegistered,
}: CursorBridgeSectionProps) {
  const { t } = useTranslation('clients');
  const [snippet, setSnippet] = useState<string | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const handleGenerate = async () => {
    setIsGenerating(true);
    setError(null);
    try {
      const client = await registerApiKeyClient(CURSOR_BRIDGE_CLIENT_NAME, null);
      setSnippet(buildCursorBridgeMcpJson(client.apiKey, gatewayUrl));
      onRegistered?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsGenerating(false);
    }
  };

  const handleCopy = async () => {
    if (!snippet) return;
    try {
      await navigator.clipboard.writeText(snippet);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Snippet is selectable as a fallback.
    }
  };

  return (
    <Card data-testid="cursor-bridge-section">
      <CardContent className="p-6">
        <div className="mb-4 flex items-start gap-4">
          <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-2">
            <img src={cursorIcon} alt="Cursor" className="h-full w-full object-contain" />
          </div>
          <div className="min-w-0 flex-1">
            <h2 className="text-lg font-semibold">{t('cursorBridge.title')}</h2>
            <p className="mt-1 text-sm text-[rgb(var(--muted))]">{t('cursorBridge.description')}</p>
          </div>
        </div>

        {!gatewayRunning && (
          <div className="mb-4 flex items-start gap-2 rounded-lg border border-amber-300 bg-amber-50 p-3 text-xs dark:border-amber-700/60 dark:bg-amber-900/20">
            <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400" />
            <p className="text-amber-800 dark:text-amber-200">{t('cursorBridge.gatewayStopped')}</p>
          </div>
        )}

        {snippet ? (
          <div className="space-y-4">
            <div>
              <p className="mb-2 text-xs font-medium uppercase tracking-wide text-[rgb(var(--muted))]">
                {t('cursorBridge.pasteInto')}
              </p>
              <pre
                data-testid="cursor-bridge-snippet"
                className="max-h-72 overflow-auto rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3 font-mono text-xs"
              >
                {snippet}
              </pre>
            </div>

            <div className="flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-3.5 dark:border-amber-700/60 dark:bg-amber-900/20">
              <AlertTriangle className="mt-0.5 h-5 w-5 flex-shrink-0 text-amber-600 dark:text-amber-400" />
              <p className="text-sm text-amber-800 dark:text-amber-200">
                {t('cursorBridge.keyOnceWarning')}
              </p>
            </div>

            <div className="flex flex-wrap gap-2">
              <Button variant="primary" size="md" onClick={handleCopy} data-testid="cursor-bridge-copy">
                {copied ? (
                  <>
                    <Check className="mr-2 h-4 w-4 text-emerald-500" />
                    {t('cursorBridge.copied')}
                  </>
                ) : (
                  <>
                    <Copy className="mr-2 h-4 w-4" />
                    {t('cursorBridge.copy')}
                  </>
                )}
              </Button>
              <Button
                variant="secondary"
                size="md"
                onClick={() => void handleGenerate()}
                disabled={isGenerating || !gatewayRunning}
                data-testid="cursor-bridge-regenerate"
              >
                {isGenerating ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <KeyRound className="mr-2 h-4 w-4" />
                )}
                {t('cursorBridge.regenerate')}
              </Button>
            </div>

            <p className="text-xs text-[rgb(var(--muted))]">{t('cursorBridge.fallbackNote')}</p>
          </div>
        ) : (
          <div className="space-y-3">
            <p className="text-sm text-[rgb(var(--muted))]">{t('cursorBridge.generateHint')}</p>
            {error && (
              <p className="text-sm text-red-600 dark:text-red-400" data-testid="cursor-bridge-error">
                {error}
              </p>
            )}
            <Button
              variant="primary"
              size="md"
              onClick={() => void handleGenerate()}
              disabled={isGenerating || !gatewayRunning}
              data-testid="cursor-bridge-generate"
            >
              {isGenerating ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('cursorBridge.generating')}
                </>
              ) : (
                <>
                  <KeyRound className="mr-2 h-4 w-4" />
                  {t('cursorBridge.generate')}
                </>
              )}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
