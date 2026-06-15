import { useState, useEffect, useCallback } from 'react';
import { X, Copy, Check, Loader2 } from 'lucide-react';
import Editor from '@monaco-editor/react';
import type { ServerViewModel, ServerDefinition } from '../types/registry';

interface ServerDefinitionModalProps {
  server: ServerViewModel;
  onClose: () => void;
}

/** Extract only ServerDefinition fields, stripping runtime state */
function extractDefinition(server: ServerViewModel): ServerDefinition {
  const {
    is_installed: _a,
    enabled: _b,
    oauth_connected: _c,
    input_values: _d,
    connection_status: _e,
    missing_required_inputs: _f,
    last_error: _g,
    created_at: _h,
    installation_source: _i,
    env_overrides: _j,
    args_append: _k,
    extra_headers: _l,
    ...definition
  } = server;
  return definition;
}

export function ServerDefinitionModal({ server, onClose }: ServerDefinitionModalProps) {
  const [copied, setCopied] = useState(false);
  const [editorReady, setEditorReady] = useState(false);

  const definition = extractDefinition(server);
  const json = JSON.stringify(definition, null, 2);

  useEffect(() => {
    const timer = setTimeout(() => setEditorReady(true), 100);
    return () => clearTimeout(timer);
  }, []);

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
              Server Definition
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handleCopy}
              className="flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-lg border border-[rgb(var(--border))] hover:bg-[rgb(var(--surface-hover))] transition-colors"
              title="Copy to clipboard"
            >
              {copied ? (
                <>
                  <Check className="h-4 w-4 text-[rgb(var(--success))]" />
                  Copied
                </>
              ) : (
                <>
                  <Copy className="h-4 w-4 text-[rgb(var(--muted))]" />
                  Copy
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
          ) : (
            <Editor
              height="100%"
              defaultLanguage="json"
              value={json}
              theme="vs-dark"
              options={{
                readOnly: true,
                minimap: { enabled: false },
                fontSize: 14,
                fontFamily: "'Fira Code', 'Consolas', monospace",
                lineNumbers: 'on',
                scrollBeyondLastLine: false,
                automaticLayout: true,
                tabSize: 2,
                wordWrap: 'on',
                folding: true,
                bracketPairColorization: { enabled: true },
                guides: {
                  bracketPairs: true,
                  indentation: true,
                },
                padding: { top: 12, bottom: 12 },
                domReadOnly: true,
              }}
              loading={
                <div className="flex items-center justify-center h-full bg-[#1e1e1e]">
                  <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
                </div>
              }
            />
          )}
        </div>
      </div>
    </div>
  );
}
