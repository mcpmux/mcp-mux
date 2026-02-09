import { useState, useEffect, useCallback, useRef } from 'react';
import { X, Save, Loader2, AlertTriangle, Wand2 } from 'lucide-react';
import { readSpaceConfig, saveSpaceConfig } from '@/lib/api/spaces';
import { refreshRegistry } from '@/lib/api/registry';
import Editor, { type Monaco } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { useToast, ToastContainer } from '@mcpmux/ui';
import USER_SPACE_CONFIG_SCHEMA from '../../../../schemas/user-space.schema.json';

interface ConfigEditorModalProps {
  spaceId: string;
  spaceName: string;
  onClose: () => void;
  onSaved: () => void;
}

export function ConfigEditorModal({ spaceId, spaceName, onClose, onSaved }: ConfigEditorModalProps) {
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
  }, [spaceId]);

  const loadConfig = async () => {
    try {
      setIsLoading(true);
      setError(null);
      const data = await readSpaceConfig(spaceId);
      // Auto-format on load if valid JSON
      try {
        const parsed = JSON.parse(data);
        setContent(JSON.stringify(parsed, null, 2));
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
    const errors = markers.map(m => `Line ${m.startLineNumber}: ${m.message}`);
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
      <ToastContainer toasts={toasts} onClose={(id) => toasts.find(t => t.id === id)?.onClose(id)} />
      <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-[rgb(var(--surface))] w-full max-w-4xl h-[80vh] rounded-xl shadow-2xl flex flex-col border border-[rgb(var(--border))]">
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-[rgb(var(--border))]">
          <div>
            <h3 className="text-lg font-semibold flex items-center gap-2">
              Add Custom Server
            </h3>
            <p className="text-sm text-[rgb(var(--muted))]">
              Edit the JSON configuration for space: {spaceName}
            </p>
          </div>
          <button
            onClick={onClose}
            className="p-2 hover:bg-[rgb(var(--surface-hover))] rounded-lg transition-colors"
          >
            <X className="h-5 w-5 text-[rgb(var(--muted))]" />
          </button>
        </div>

        {/* Toolbar */}
        <div className="flex items-center gap-2 p-2 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-dim))]">
          <button
            onClick={handleSave}
            disabled={isSaving || isLoading || !isValidJson}
            className="flex items-center gap-2 px-3 py-1.5 text-sm font-medium bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))] rounded-md hover:bg-[rgb(var(--primary-hover))] disabled:opacity-50 transition-colors"
          >
            {isSaving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
            Save Changes
          </button>
          
          <div className="h-4 w-px bg-[rgb(var(--border))]" />

          <button
            onClick={handleFormat}
            disabled={isLoading || !isValidJson}
            className="flex items-center gap-2 px-3 py-1.5 text-sm font-medium text-[rgb(var(--foreground))] hover:bg-[rgb(var(--surface-hover))] rounded-md transition-colors disabled:opacity-50"
            title="Format JSON (Ctrl+Shift+F)"
          >
            <Wand2 className="h-4 w-4" />
            Format
          </button>

          <div className="flex-1" />
          
          {!isValidJson && (
            <span className="flex items-center gap-1.5 text-xs text-[rgb(var(--error))] px-2 font-medium">
              <AlertTriangle className="h-3 w-3" />
              {validationErrors.length > 0 ? 'Schema Error' : 'Invalid JSON'}
            </span>
          )}
        </div>

        {/* Editor Area */}
        <div className="flex-1 relative min-h-0 bg-[#1e1e1e]">
          {(isLoading || !editorReady) ? (
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
                <div className="flex items-center justify-center h-full bg-[#1e1e1e]">
                  <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
                </div>
              }
            />
          )}
        </div>

        {/* Footer / Status Bar */}
        {(error || validationErrors.length > 0) && (
          <div className="p-2 bg-[rgb(var(--error))]/10 border-t border-[rgb(var(--error))]/20 text-[rgb(var(--error))] text-xs px-4 max-h-20 overflow-auto">
            {error || validationErrors.slice(0, 3).join(' • ')}
            {validationErrors.length > 3 && ` (+${validationErrors.length - 3} more)`}
          </div>
        )}
      </div>
    </div>
    </>
  );
}
