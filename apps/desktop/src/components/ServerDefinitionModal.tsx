import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Copy, Check, Loader2, Save } from 'lucide-react';
import type { ServerViewModel, ServerDefinition } from '../types/registry';
import { MonacoJsonEditor } from './monaco-json-editor.component';
import { updateServerInConfig } from '@/lib/api/spaces';

const EDITOR_MOUNT_TIMEOUT_MS = 10_000;

interface ServerDefinitionModalProps {
  server: ServerViewModel;
  onClose: () => void;
  /** Called after a successful save so the caller can reload the server list. */
  onSaved?: () => void;
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

/**
 * Build the standard MCP config format (the shape that lives under a
 * `mcpServers` key in a space JSON file) from a server's current view model.
 * This is the editable subset — no id/source/badges or other derived fields.
 */
function buildEditableEntry(server: ServerViewModel): Record<string, unknown> {
  const entry: Record<string, unknown> = {};

  if (server.transport.type === 'stdio') {
    entry.command = server.transport.command;
    entry.args = server.transport.args;
    entry.env = server.transport.env;
  } else {
    entry.url = server.transport.url;
    entry.headers = server.transport.headers;
  }

  entry.name = server.name;
  if (server.description) entry.description = server.description;
  if (server.icon) entry.icon = server.icon;
  if (server.alias) entry.alias = server.alias;
  if (server.auth && server.auth.type !== 'none') entry.auth = server.auth;
  if (server.transport.metadata.inputs.length > 0) {
    entry.metadata = { inputs: server.transport.metadata.inputs };
  }

  return entry;
}

export function ServerDefinitionModal({ server, onClose, onSaved }: ServerDefinitionModalProps) {
  const { t } = useTranslation('servers');
  const [copied, setCopied] = useState(false);
  const [editorReady, setEditorReady] = useState(false);
  const [editorMounted, setEditorMounted] = useState(false);
  const [editorLoadFailed, setEditorLoadFailed] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const isEditable = server.source.type === 'UserSpace';
  const [content, setContent] = useState(() =>
    JSON.stringify(isEditable ? buildEditableEntry(server) : extractDefinition(server), null, 2),
  );
  const json = content;

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

  const handleContentChange = (value: string | undefined) => {
    if (value !== undefined) {
      setContent(value);
      setSaveError(null);
    }
  };

  const handleSave = useCallback(async () => {
    let parsed: Record<string, unknown>;
    try {
      parsed = JSON.parse(content);
    } catch (e) {
      setSaveError(t('definitionModal.invalidJson', { message: (e as Error).message }));
      return;
    }

    if (server.source.type !== 'UserSpace') {
      return;
    }

    setIsSaving(true);
    setSaveError(null);
    try {
      await updateServerInConfig(server.source.space_id, server.id, parsed);
      onSaved?.();
      onClose();
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  }, [content, onClose, onSaved, server, t]);

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-[rgb(var(--surface))] w-full max-w-3xl h-[70vh] rounded-xl shadow-2xl flex flex-col border border-[rgb(var(--border))] animate-in fade-in scale-in duration-150">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-[rgb(var(--border))]">
          <div className="min-w-0">
            <h3 className="text-lg font-semibold truncate">{server.name}</h3>
            <p className="text-sm text-[rgb(var(--muted))]">
              {isEditable ? t('definitionModal.subtitleEditable') : t('definitionModal.subtitle')}
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
            {isEditable && (
              <button
                onClick={handleSave}
                disabled={isSaving}
                className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-lg bg-[rgb(var(--primary))] text-white hover:bg-[rgb(var(--primary))]/90 transition-colors disabled:opacity-50"
              >
                {isSaving ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Save className="h-4 w-4" />
                )}
                {t('definitionModal.save')}
              </button>
            )}
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
              readOnly={!isEditable}
              value={json}
              onChange={(e) => handleContentChange(e.target.value)}
              className="h-full w-full resize-none bg-[#1e1e1e] p-3 font-mono text-sm text-[#d4d4d4] focus:outline-none"
              spellCheck={false}
              aria-label={t('definitionModal.subtitle')}
            />
          ) : (
            <MonacoJsonEditor
              value={json}
              onChange={handleContentChange}
              readOnly={!isEditable}
              onMount={handleEditorMount}
              onMountFailed={handleEditorMountFailed}
              testId="server-definition-monaco"
            />
          )}
        </div>

        {saveError && (
          <div className="border-t border-[rgb(var(--error))]/20 bg-[rgb(var(--error))]/10 px-4 py-2 text-xs text-[rgb(var(--error))]">
            {saveError}
          </div>
        )}

        {editorLoadFailed && (
          <div className="border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-dim))] px-4 py-2 text-xs text-[rgb(var(--muted))]">
            {t('definitionModal.editorLoadFailed')}
          </div>
        )}
      </div>
    </div>
  );
}
