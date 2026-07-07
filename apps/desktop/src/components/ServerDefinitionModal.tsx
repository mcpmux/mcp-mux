import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Copy, Check, Loader2 } from 'lucide-react';
import type { ServerViewModel, ServerDefinition } from '../types/registry';
import { MonacoJsonEditor } from './monaco-json-editor.component';

const EDITOR_MOUNT_TIMEOUT_MS = 10_000;

interface ServerDefinitionModalProps {
  server: ServerViewModel;
  onClose: () => void;
}

const RUNTIME_SERVER_FIELDS = [
  'is_installed',
  'enabled',
  'oauth_connected',
  'input_values',
  'connection_status',
  'missing_required_inputs',
  'last_error',
  'created_at',
  'installation_source',
  'env_overrides',
  'args_append',
  'extra_headers',
  'default_params',
] as const;

/** Extract only ServerDefinition fields, stripping runtime state */
function extractDefinition(server: ServerViewModel): ServerDefinition {
  const copy = { ...server };
  for (const key of RUNTIME_SERVER_FIELDS) {
    delete (copy as Record<string, unknown>)[key];
  }
  return copy as ServerDefinition;
}

export function ServerDefinitionModal({ server, onClose }: ServerDefinitionModalProps) {
  const { t } = useTranslation('servers');
  const [copied, setCopied] = useState(false);
  const [editorReady, setEditorReady] = useState(false);
  const [editorMounted, setEditorMounted] = useState(false);
  const [editorLoadFailed, setEditorLoadFailed] = useState(false);

  const definition = extractDefinition(server);
  const json = JSON.stringify(definition, null, 2);

  useEffect(() => {
    const timer = setTimeout(() => setEditorReady(true), 100);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!editorReady || editorMounted || editorLoadFailed) {
      return;
    }

    const timer = setTimeout(() => {
      setEditorLoadFailed(true);
    }, EDITOR_MOUNT_TIMEOUT_MS);

    return () => clearTimeout(timer);
  }, [editorReady, editorMounted, editorLoadFailed]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback for environments where clipboard API is unavailable
    }
  }, [json]);

  /**
   * Mark Monaco mounted so the mount-timeout fallback does not fire.
   */
  const handleEditorMount = () => {
    setEditorMounted(true);
  };

  /**
   * Fall back to plain JSON when the editor container has no measurable height.
   */
  const handleEditorMountFailed = () => {
    setEditorLoadFailed(true);
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-[rgb(var(--surface))] w-full max-w-3xl h-[70vh] rounded-xl shadow-2xl flex flex-col border border-[rgb(var(--border))] animate-in fade-in scale-in duration-150">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-[rgb(var(--border))]">
          <div className="min-w-0">
            <h3 className="text-lg font-semibold truncate">
              {server.name}
            </h3>
            <p className="text-sm text-[rgb(var(--muted))]">
              {t('definitionModal.subtitle')}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handleCopy}
              className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-lg border border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
              title={t('definitionModal.copyTitle')}
            >
              {copied ? (
                <>
                  <Check className="h-4 w-4 text-[rgb(var(--success))]" />
                  {t('definitionModal.copied')}
                </>
              ) : (
                <>
                  <Copy className="h-4 w-4 text-[rgb(var(--muted))]" />
                  {t('definitionModal.copy')}
                </>
              )}
            </button>
            <button
              onClick={onClose}
              className="p-2 hover:bg-[rgb(var(--surface-hover))] rounded-lg transition-colors"
            >
              <X className="h-5 w-5 text-[rgb(var(--muted))]" />
            </button>
          </div>
        </div>

        {/* Editor Area */}
        <div className="flex-1 relative min-h-0 bg-[#1e1e1e]">
          {!editorReady ? (
            <div className="absolute inset-0 flex items-center justify-center">
              <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
            </div>
          ) : editorLoadFailed ? (
            <textarea
              readOnly
              value={json}
              className="h-full w-full resize-none bg-[#1e1e1e] p-3 font-mono text-sm text-[#d4d4d4] focus:outline-none"
              spellCheck={false}
              aria-label={t('definitionModal.subtitle')}
            />
          ) : (
            <MonacoJsonEditor
              value={json}
              readOnly
              onMount={handleEditorMount}
              onMountFailed={handleEditorMountFailed}
              testId="server-definition-monaco"
            />
          )}
        </div>

        {editorLoadFailed && (
          <div className="border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-dim))] px-4 py-2 text-xs text-[rgb(var(--muted))]">
            {t('definitionModal.editorLoadFailed')}
          </div>
        )}
      </div>
    </div>
  );
}
