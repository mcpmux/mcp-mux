import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Plus, Loader2, Check, AlertTriangle } from 'lucide-react';
import { type Monaco } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { Button, useToast } from '@mcpmux/ui';
import { readSpaceConfig, saveSpaceConfig } from '@/lib/api/spaces';
import { MonacoJsonEditor } from '@/components/monaco-json-editor.component';
import {
  createDefaultStdioEntry,
  nextCustomServerKey,
  SINGLE_SERVER_ENTRY_SCHEMA,
  upsertServerEntry,
  type SpaceConfigJson,
} from './custom-server-entry.helpers';

const EDITOR_MOUNT_TIMEOUT_MS = 10_000;

type PanelMode = 'form' | 'json';

interface CustomServerPanelProps {
  spaceId: string;
  spaceName: string;
  onClose: () => void;
  onSaved: () => void;
}

/**
 * Segmented Form / JSON toggle matching WorkspacesPage filter styling.
 */
function ModeToggle({
  mode,
  onChange,
  formLabel,
  jsonLabel,
}: {
  mode: PanelMode;
  onChange: (mode: PanelMode) => void;
  formLabel: string;
  jsonLabel: string;
}) {
  const options: Array<{ value: PanelMode; label: string }> = [
    { value: 'form', label: formLabel },
    { value: 'json', label: jsonLabel },
  ];

  return (
    <div
      className="inline-flex rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-0.5 gap-0.5"
      data-testid="custom-server-panel-mode-toggle"
    >
      {options.map((o) => {
        const active = o.value === mode;
        return (
          <button
            key={o.value}
            type="button"
            onClick={() => onChange(o.value)}
            aria-pressed={active}
            data-testid={`custom-server-panel-mode-${o.value}`}
            className={[
              'inline-flex items-center px-3 py-1.5 text-xs font-medium rounded-lg transition-all',
              active
                ? 'bg-[rgb(var(--background))] text-[rgb(var(--foreground))] shadow-sm'
                : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]',
            ].join(' ')}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * Slide-in panel for adding a custom server via JSON (Form mode stub until Phase 3).
 */
export function CustomServerPanel({ spaceId, spaceName, onClose, onSaved }: CustomServerPanelProps) {
  const { t } = useTranslation('servers');
  const { success, error: showError } = useToast();

  const [mode, setMode] = useState<PanelMode>('json');
  const [serverKey, setServerKey] = useState('');
  const [jsonContent, setJsonContent] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [isValidJson, setIsValidJson] = useState(true);
  const [validationErrors, setValidationErrors] = useState<string[]>([]);
  const [editorReady, setEditorReady] = useState(false);
  const [editorMounted, setEditorMounted] = useState(false);
  const [editorLoadFailed, setEditorLoadFailed] = useState(false);
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => setEditorReady(true), 100);
    return () => clearTimeout(timer);
  }, []);

  /**
   * Load space config and seed default server key + stdio template.
   */
  const initializeFromConfig = useCallback(async () => {
    try {
      setIsLoading(true);
      setLoadError(null);
      const raw = await readSpaceConfig(spaceId);
      const parsed = JSON.parse(raw) as SpaceConfigJson;
      const servers = parsed.mcpServers ?? {};
      const key = nextCustomServerKey(servers);
      setServerKey(key);
      setJsonContent(JSON.stringify(createDefaultStdioEntry(key), null, 2));
      setIsValidJson(true);
      setValidationErrors([]);
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
      setEditorMounted(false);
      setEditorLoadFailed(false);
    }
  }, [spaceId]);

  useEffect(() => {
    void initializeFromConfig();
  }, [initializeFromConfig]);

  useEffect(() => {
    if (isLoading || !editorReady || editorMounted || editorLoadFailed) {
      return;
    }
    const timer = setTimeout(() => setEditorLoadFailed(true), EDITOR_MOUNT_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [isLoading, editorReady, editorMounted, editorLoadFailed]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  /**
   * Register single-entry JSON schema with Monaco before the editor mounts.
   */
  const handleEditorBeforeMount = (monaco: Monaco) => {
    monaco.languages.json.jsonDefaults.setDiagnosticsOptions({
      validate: true,
      schemas: [
        {
          uri: 'mcpmux://schemas/single-server-entry.json',
          fileMatch: ['*'],
          schema: SINGLE_SERVER_ENTRY_SCHEMA,
        },
      ],
      enableSchemaRequest: false,
      allowComments: false,
      trailingCommas: 'error',
    });
  };

  /** Sync Monaco validation markers into panel save state. */
  const handleEditorValidation = (markers: editor.IMarker[]) => {
    const errors = markers.map((m) =>
      t('customServerPanel.validation.line', { line: m.startLineNumber, message: m.message }),
    );
    setValidationErrors(errors);
    setIsValidJson(markers.length === 0);
  };

  /** Persist the edited entry into the space config file. */
  const handleSave = useCallback(async () => {
    const trimmedKey = serverKey.trim();
    if (!trimmedKey) {
      showError(
        t('customServerPanel.toast.saveFailed'),
        t('customServerPanel.toast.serverIdRequired'),
      );
      return;
    }

    let entry: Record<string, unknown>;
    try {
      entry = JSON.parse(jsonContent) as Record<string, unknown>;
    } catch (e) {
      const message = (e as Error).message;
      setSaveError(t('customServerPanel.validation.invalidJson', { message }));
      showError(t('customServerPanel.toast.invalidJsonTitle'), message);
      return;
    }

    if (!isValidJson) {
      return;
    }

    setIsSaving(true);
    setSaveError(null);
    try {
      const raw = await readSpaceConfig(spaceId);
      const parsed = JSON.parse(raw) as SpaceConfigJson;
      const updated = upsertServerEntry(parsed, trimmedKey, entry);
      await saveSpaceConfig(spaceId, JSON.stringify(updated, null, 2));
      success(t('customServerPanel.toast.saved'), t('customServerPanel.toast.savedBody'));
      onSaved();
      onClose();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setSaveError(message);
      showError(t('customServerPanel.toast.saveFailed'), message);
    } finally {
      setIsSaving(false);
    }
  }, [
    serverKey,
    jsonContent,
    isValidJson,
    spaceId,
    success,
    showError,
    onSaved,
    onClose,
    t,
  ]);

  const panelWidthClass =
    mode === 'json'
      ? 'w-full max-w-[720px] min-w-[600px]'
      : 'w-full max-w-[480px] min-w-[420px]';

  return (
    <>
      <div
        className="fixed inset-0 bg-black/20 backdrop-blur-[2px] z-[55] animate-in fade-in duration-200"
        onClick={onClose}
        data-testid="custom-server-panel-backdrop"
      />
      <div
        className={`fixed right-0 top-0 bottom-0 bg-[rgb(var(--surface))] border-l border-[rgb(var(--border))] shadow-2xl flex flex-col animate-in slide-in-from-right duration-300 z-[60] ${panelWidthClass}`}
        data-testid="custom-server-panel"
      >
        <div className="flex-shrink-0 p-4 border-b border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
          <div className="flex items-start justify-between gap-2">
            <div className="flex items-start gap-3 flex-1 min-w-0">
              <div className="w-11 h-11 flex-shrink-0 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg border border-[rgb(var(--border-subtle))]">
                <Plus className="h-5 w-5 text-[rgb(var(--primary))]" />
              </div>
              <div className="min-w-0">
                <h2 className="text-lg font-bold text-[rgb(var(--foreground))]">
                  {t('customServerPanel.title')}
                </h2>
                <p className="text-xs text-[rgb(var(--muted))] mt-0.5">
                  {t('customServerPanel.subtitle', { spaceName })}
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors flex-shrink-0"
              aria-label={t('customServerPanel.closeAria')}
            >
              <X className="h-5 w-5" />
            </button>
          </div>
          <div className="mt-4">
            <ModeToggle
              mode={mode}
              onChange={setMode}
              formLabel={t('customServerPanel.modeForm')}
              jsonLabel={t('customServerPanel.modeJson')}
            />
          </div>
        </div>

        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            </div>
          ) : loadError ? (
            <p className="text-sm text-[rgb(var(--error))]">{loadError}</p>
          ) : mode === 'form' ? (
            <p className="text-sm text-[rgb(var(--muted))]">{t('customServerPanel.formComingSoon')}</p>
          ) : (
            <>
              <div>
                <label
                  htmlFor="custom-server-id"
                  className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
                >
                  {t('customServerPanel.serverId')}
                </label>
                <p className="text-xs text-[rgb(var(--muted))] mb-2">
                  {t('customServerPanel.serverIdDesc')}
                </p>
                <input
                  id="custom-server-id"
                  type="text"
                  value={serverKey}
                  onChange={(e) => setServerKey(e.target.value)}
                  placeholder={t('customServerPanel.serverIdPlaceholder')}
                  className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary-500"
                  data-testid="custom-server-panel-server-id"
                />
              </div>
              <div className="flex flex-col min-h-[280px] rounded-lg border border-[rgb(var(--border))] overflow-hidden bg-[#1e1e1e]">
                {!editorReady ? (
                  <div className="flex flex-1 items-center justify-center py-12">
                    <Loader2 className="h-8 w-8 animate-spin text-[rgb(var(--muted))]" />
                  </div>
                ) : editorLoadFailed ? (
                  <textarea
                    value={jsonContent}
                    onChange={(e) => setJsonContent(e.target.value)}
                    className="flex-1 min-h-[280px] w-full resize-none bg-[#1e1e1e] p-3 font-mono text-sm text-[#d4d4d4] focus:outline-none"
                    spellCheck={false}
                  />
                ) : (
                  <MonacoJsonEditor
                    value={jsonContent}
                    onChange={(v) => v !== undefined && setJsonContent(v)}
                    beforeMount={handleEditorBeforeMount}
                    onMount={(mounted) => {
                      editorRef.current = mounted;
                      setEditorMounted(true);
                    }}
                    onMountFailed={() => setEditorLoadFailed(true)}
                    onValidate={handleEditorValidation}
                    testId="custom-server-panel-monaco"
                  />
                )}
              </div>
              {!isValidJson && (
                <span className="flex items-center gap-1.5 text-xs font-medium text-[rgb(var(--error))]">
                  <AlertTriangle className="h-3 w-3" />
                  {validationErrors.length > 0
                    ? t('customServerPanel.schemaError')
                    : t('customServerPanel.invalidJson')}
                </span>
              )}
            </>
          )}
        </div>

        <div className="flex-shrink-0 p-4 border-t border-[rgb(var(--border))] bg-[rgb(var(--surface-elevated))]">
          {saveError && (
            <p className="text-xs text-[rgb(var(--error))] mb-2">{saveError}</p>
          )}
          <div className="flex items-center gap-2">
            <Button
              variant="primary"
              size="md"
              onClick={() => void handleSave()}
              disabled={isSaving || isLoading || !!loadError || (mode === 'json' && !isValidJson)}
              className="flex-1"
              data-testid="custom-server-panel-save"
            >
              {isSaving ? (
                <Loader2 className="h-4 w-4 animate-spin mr-1.5" />
              ) : (
                <Check className="h-4 w-4 mr-1.5" />
              )}
              {isSaving ? t('customServerPanel.saving') : t('customServerPanel.save')}
            </Button>
            <Button variant="secondary" size="md" onClick={onClose} disabled={isSaving}>
              {t('customServerPanel.cancel')}
            </Button>
          </div>
        </div>
      </div>
    </>
  );
}
