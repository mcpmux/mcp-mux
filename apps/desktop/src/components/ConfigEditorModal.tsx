import { useState, useEffect, useCallback, useRef } from 'react';
import { X, Save, Loader2, AlertTriangle, Wand2, Plus } from 'lucide-react';
import { readSpaceConfig, saveSpaceConfig } from '@/lib/api/spaces';
import { refreshRegistry } from '@/lib/api/registry';
import Editor, { type Monaco } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { useToast, ToastContainer } from '@mcpmux/ui';
import USER_SPACE_CONFIG_SCHEMA from '../../../../schemas/user-space.schema.json';
import { RequestServerCTA } from './Contribute';

interface ConfigEditorModalProps {
  spaceId: string;
  spaceName: string;
  insertNewServer?: boolean;
  onClose: () => void;
  onSaved: () => void;
}

type SpaceConfigJson = {
  mcpServers?: Record<string, unknown>;
  [key: string]: unknown;
};

const CUSTOM_SERVER_BASE_KEY = 'custom-server';

function nextCustomServerKey(servers: Record<string, unknown>): string {
  let suffix = 1;

  while (true) {
    const key = suffix === 1 ? CUSTOM_SERVER_BASE_KEY : CUSTOM_SERVER_BASE_KEY + '-' + suffix;
    if (!(key in servers)) {
      return key;
    }
    suffix += 1;
  }
}

function addCustomServerDraft(config: SpaceConfigJson): SpaceConfigJson {
  const mcpServers = { ...(config.mcpServers ?? {}) };
  const key = nextCustomServerKey(mcpServers);
  const suffix =
    key === CUSTOM_SERVER_BASE_KEY ? '' : ' ' + key.replace(CUSTOM_SERVER_BASE_KEY + '-', '');

  mcpServers[key] = {
    name: 'New Custom Server' + suffix,
    command: '',
    args: [],
    env: {},
  };

  return {
    ...config,
    mcpServers,
  };
}

export function ConfigEditorModal({
  spaceId,
  spaceName,
  insertNewServer = false,
  onClose,
  onSaved,
}: ConfigEditorModalProps) {
  const [content, setContent] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isValidJson, setIsValidJson] = useState(true);
  const [validationErrors, setValidationErrors] = useState<string[]>([]);
  const [editorReady, setEditorReady] = useState(false);
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const { toasts, success, error: showError } = useToast();

  // Delay editor mount to avoid glitch during modal open
  useEffect(() => {
    const timer = setTimeout(() => setEditorReady(true), 100);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    loadConfig();
  }, [spaceId, insertNewServer]);

  const loadConfig = async () => {
    try {
      setIsLoading(true);
      setError(null);
      const data = await readSpaceConfig(spaceId);
      // Auto-format on load if valid JSON. When opened from Add Custom Server,
      // insert a unique draft entry instead of replacing an existing server block.
      try {
        const parsed = JSON.parse(data) as SpaceConfigJson;
        const nextConfig = insertNewServer ? addCustomServerDraft(parsed) : parsed;
        setContent(JSON.stringify(nextConfig, null, 2));
      } catch {
        setContent(data);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const handleSave = async () => {
    try {
      // Validate JSON
      try {
        JSON.parse(content);
      } catch (e) {
        setIsValidJson(false);
        setError(`Invalid JSON: ${(e as Error).message}`);
        showError('Invalid JSON', (e as Error).message);
        return;
      }

      setIsSaving(true);
      setError(null);
      await saveSpaceConfig(spaceId, content);
      // Refresh server discovery to pick up new/changed servers
      await refreshRegistry();

      success('Configuration saved', 'Space configuration updated successfully');
      onSaved();
      onClose();
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      setError(errorMsg);
      showError('Failed to save configuration', errorMsg);
    } finally {
      setIsSaving(false);
    }
  };

  const handleFormat = useCallback(() => {
    if (editorRef.current) {
      // Use Monaco's built-in formatter
      editorRef.current.getAction('editor.action.formatDocument')?.run();
    }
  }, []);

  const handleInsertCustomServer = useCallback(() => {
    try {
      const parsed = JSON.parse(content || '{"mcpServers":{}}') as SpaceConfigJson;
      setContent(JSON.stringify(addCustomServerDraft(parsed), null, 2));
      setIsValidJson(true);
      setError(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setIsValidJson(false);
      setError('Invalid JSON: ' + message);
      showError('Invalid JSON', message);
    }
  }, [content, showError]);

  // Configure Monaco before mount to set up JSON schema validation
  const handleEditorBeforeMount = (monaco: Monaco) => {
    // Configure JSON language with schema validation
    monaco.languages.json.jsonDefaults.setDiagnosticsOptions({
      validate: true,
      schemas: [
        {
          uri: 'mcpmux://schemas/user-space-config.json',
          fileMatch: ['*'],
          schema: USER_SPACE_CONFIG_SCHEMA,
        },
      ],
      enableSchemaRequest: false,
      allowComments: false,
      trailingCommas: 'error',
    });
  };

  const handleEditorMount = (editor: editor.IStandaloneCodeEditor, monaco: Monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;

    // Focus editor on mount
    editor.focus();
  };

  const handleEditorValidation = (markers: editor.IMarker[]) => {
    const errors = markers.map((m) => `Line ${m.startLineNumber}: ${m.message}`);
    setValidationErrors(errors);
    setIsValidJson(markers.length === 0);
  };

  const handleContentChange = (newValue: string | undefined) => {
    if (newValue !== undefined) {
      setContent(newValue);
      // Clear any manual errors when content changes
      if (error && (error.startsWith('Invalid JSON') || error.startsWith('Cannot format'))) {
        setError(null);
      }
    }
  };

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+Shift+F to format
      if (e.ctrlKey && e.shiftKey && e.key === 'F') {
        e.preventDefault();
        handleFormat();
      }
      // Ctrl+S to save
      if (e.ctrlKey && e.key === 's') {
        e.preventDefault();
        handleSave();
      }
      // Escape to close
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleFormat, onClose]);

  return (
    <>
      <ToastContainer
        toasts={toasts}
        onClose={(id) => toasts.find((t) => t.id === id)?.onClose(id)}
      />
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
        <div className="flex h-[80vh] w-full max-w-4xl flex-col rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] shadow-2xl">
          {/* Header */}
          <div className="flex items-center justify-between border-b border-[rgb(var(--border))] p-4">
            <div className="flex items-center gap-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-lg border border-[rgb(var(--primary))]/20 bg-[rgb(var(--primary))]/10">
                <Save className="h-4 w-4 text-[rgb(var(--primary))]" />
              </div>
              <div>
                <h3 className="text-base font-semibold">Custom Server Configuration</h3>
                <p className="text-xs text-[rgb(var(--muted))]">{spaceName} &middot; JSON config</p>
              </div>
            </div>
            <button
              onClick={onClose}
              className="rounded-lg p-2 transition-colors hover:bg-[rgb(var(--surface-hover))]"
            >
              <X className="h-5 w-5 text-[rgb(var(--muted))]" />
            </button>
          </div>

          {/* Toolbar */}
          <div className="flex items-center gap-2 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-dim))] px-3 py-2">
            <button
              onClick={handleSave}
              disabled={isSaving || isLoading || !isValidJson}
              className="flex items-center gap-2 rounded-lg bg-[rgb(var(--primary))] px-3.5 py-1.5 text-sm font-medium text-[rgb(var(--primary-foreground))] shadow-sm transition-colors hover:bg-[rgb(var(--primary-hover))] disabled:opacity-50"
            >
              {isSaving ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Save className="h-4 w-4" />
              )}
              Save
            </button>

            <div className="h-5 w-px bg-[rgb(var(--border))]" />

            <button
              onClick={handleFormat}
              disabled={isLoading || !isValidJson}
              className="flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))] disabled:opacity-50"
              title="Format JSON (Ctrl+Shift+F)"
            >
              <Wand2 className="h-4 w-4" />
              Format
            </button>

            <button
              onClick={handleInsertCustomServer}
              disabled={isLoading || !isValidJson}
              className="flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium text-[rgb(var(--muted))] transition-colors hover:bg-[rgb(var(--surface-hover))] hover:text-[rgb(var(--foreground))] disabled:opacity-50"
              title="Insert another unique custom server entry"
            >
              <Plus className="h-4 w-4" />
              Insert Server
            </button>

            <div className="flex-1" />

            {!isValidJson && (
              <span className="flex items-center gap-1.5 px-2 text-xs font-medium text-[rgb(var(--error))]">
                <AlertTriangle className="h-3 w-3" />
                {validationErrors.length > 0 ? 'Schema Error' : 'Invalid JSON'}
              </span>
            )}

            <span className="text-xs text-[rgb(var(--muted))]">
              Ctrl+S save &middot; Ctrl+Shift+F format
            </span>
          </div>

          {/* Contribute / Request CTA — surfaces the registry templates so users
            don't have to hand-roll a definition if one already exists upstream. */}
          <div className="border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-dim))] px-4 py-3">
            <RequestServerCTA />
          </div>

          {/* Editor Area */}
          <div className="relative min-h-0 flex-1 bg-[#1e1e1e]">
            {isLoading || !editorReady ? (
              <div className="absolute inset-0 flex items-center justify-center">
                <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
              </div>
            ) : (
              <Editor
                height="100%"
                defaultLanguage="json"
                value={content}
                theme="vs-dark"
                onChange={handleContentChange}
                beforeMount={handleEditorBeforeMount}
                onMount={handleEditorMount}
                onValidate={handleEditorValidation}
                options={{
                  minimap: { enabled: false },
                  fontSize: 14,
                  fontFamily: "'Fira Code', 'Consolas', monospace",
                  lineNumbers: 'on',
                  scrollBeyondLastLine: false,
                  automaticLayout: true,
                  tabSize: 2,
                  wordWrap: 'on',
                  formatOnPaste: true,
                  formatOnType: true,
                  folding: true,
                  bracketPairColorization: { enabled: true },
                  guides: {
                    bracketPairs: true,
                    indentation: true,
                  },
                  padding: { top: 12, bottom: 12 },
                }}
                loading={
                  <div className="flex h-full items-center justify-center bg-[#1e1e1e]">
                    <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
                  </div>
                }
              />
            )}
          </div>

          {/* Footer / Status Bar */}
          {(error || validationErrors.length > 0) && (
            <div className="max-h-20 overflow-auto border-t border-[rgb(var(--error))]/20 bg-[rgb(var(--error))]/10 p-2 px-4 text-xs text-[rgb(var(--error))]">
              {error || validationErrors.slice(0, 3).join(' • ')}
              {validationErrors.length > 3 && ` (+${validationErrors.length - 3} more)`}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
