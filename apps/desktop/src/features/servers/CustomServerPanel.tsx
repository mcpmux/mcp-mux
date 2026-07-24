import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { X, Plus, Loader2, Check, AlertTriangle, Settings, SlidersHorizontal } from 'lucide-react';
import { type Monaco } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import { Button, useToast } from '@mcpmux/ui';
import { readSpaceConfig, saveSpaceConfig } from '@/lib/api/spaces';
import { MonacoJsonEditor } from '@/components/monaco-json-editor.component';
import { CollapsibleSection } from '@/features/workspaces/WorkspacesPage';
import {
  buildServerEntryFromForm,
  createDefaultFormState,
  createDefaultStdioEntry,
  nextCustomServerKey,
  parseDefaultParamsJson,
  SINGLE_SERVER_ENTRY_SCHEMA,
  upsertServerEntry,
  type CustomServerFormState,
  type CustomServerTransportType,
  type InputDefFormRow,
  type KeyValueFormRow,
  type SpaceConfigJson,
} from './custom-server-entry.helpers';

const EDITOR_MOUNT_TIMEOUT_MS = 10_000;

type PanelMode = 'form' | 'json';

interface CustomServerPanelProps {
  spaceId: string;
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
 * Reusable key/value row editor for env vars and HTTP headers.
 */
function KeyValueEditor({
  rows,
  onChange,
  keyPlaceholder,
  valuePlaceholder,
  removeLabel,
  addLabel,
  testIdPrefix,
}: {
  rows: KeyValueFormRow[];
  onChange: (rows: KeyValueFormRow[]) => void;
  keyPlaceholder: string;
  valuePlaceholder: string;
  removeLabel: string;
  addLabel: string;
  testIdPrefix: string;
}) {
  return (
    <div className="space-y-2" data-testid={`${testIdPrefix}-rows`}>
      {rows.map((row, idx) => (
        <div key={idx} className="flex gap-2">
          <input
            type="text"
            value={row.key}
            onChange={(e) => {
              const next = [...rows];
              next[idx] = { ...row, key: e.target.value };
              onChange(next);
            }}
            placeholder={keyPlaceholder}
            className="input flex-1 font-mono text-sm"
            data-testid={`${testIdPrefix}-key-${idx}`}
          />
          <input
            type="text"
            value={row.value}
            onChange={(e) => {
              const next = [...rows];
              next[idx] = { ...row, value: e.target.value };
              onChange(next);
            }}
            placeholder={valuePlaceholder}
            className="input flex-1 font-mono text-sm"
            data-testid={`${testIdPrefix}-value-${idx}`}
          />
          <button
            type="button"
            onClick={() => onChange(rows.filter((_, i) => i !== idx))}
            className="px-2 py-1 text-sm text-[rgb(var(--muted))] hover:text-[rgb(var(--error))] transition-colors"
            title={removeLabel}
            data-testid={`${testIdPrefix}-remove-${idx}`}
          >
            ✕
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() => onChange([...rows, { key: '', value: '' }])}
        className="text-xs text-[rgb(var(--primary))] hover:underline"
        data-testid={`${testIdPrefix}-add`}
      >
        {addLabel}
      </button>
    </div>
  );
}

/**
 * Add/remove editor for metadata.inputs definition rows.
 */
function InputDefEditor({
  rows,
  onChange,
}: {
  rows: InputDefFormRow[];
  onChange: (rows: InputDefFormRow[]) => void;
}) {
  const { t } = useTranslation('servers');
  return (
    <div className="space-y-3" data-testid="custom-server-input-defs">
      {rows.map((row, idx) => (
        <div
          key={idx}
          className="rounded-lg border border-[rgb(var(--border-subtle))] p-3 space-y-2"
          data-testid={`custom-server-input-def-${idx}`}
        >
          <div className="flex gap-2">
            <input
              type="text"
              value={row.id}
              onChange={(e) => {
                const next = [...rows];
                next[idx] = { ...row, id: e.target.value };
                onChange(next);
              }}
              placeholder={t('customServerPanel.form.inputIdPlaceholder')}
              className="input flex-1 font-mono text-sm"
              data-testid={`custom-server-input-def-id-${idx}`}
            />
            <input
              type="text"
              value={row.label}
              onChange={(e) => {
                const next = [...rows];
                next[idx] = { ...row, label: e.target.value };
                onChange(next);
              }}
              placeholder={t('customServerPanel.form.inputLabelPlaceholder')}
              className="input flex-1 text-sm"
              data-testid={`custom-server-input-def-label-${idx}`}
            />
            <button
              type="button"
              onClick={() => onChange(rows.filter((_, i) => i !== idx))}
              className="px-2 py-1 text-sm text-[rgb(var(--muted))] hover:text-[rgb(var(--error))] transition-colors"
              title={t('customServerPanel.form.removeRow')}
            >
              ✕
            </button>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <select
              value={row.type}
              onChange={(e) => {
                const next = [...rows];
                next[idx] = { ...row, type: e.target.value as 'text' | 'password' };
                onChange(next);
              }}
              className="input text-sm"
              data-testid={`custom-server-input-def-type-${idx}`}
            >
              <option value="text">{t('customServerPanel.form.inputTypeText')}</option>
              <option value="password">{t('customServerPanel.form.inputTypePassword')}</option>
            </select>
            <label className="flex items-center gap-1.5 text-xs text-[rgb(var(--foreground))]">
              <input
                type="checkbox"
                checked={row.required}
                onChange={(e) => {
                  const next = [...rows];
                  next[idx] = { ...row, required: e.target.checked };
                  onChange(next);
                }}
                className="w-3.5 h-3.5 rounded border-[rgb(var(--border))]"
              />
              {t('customServerPanel.form.inputRequired')}
            </label>
            <label className="flex items-center gap-1.5 text-xs text-[rgb(var(--foreground))]">
              <input
                type="checkbox"
                checked={row.secret}
                onChange={(e) => {
                  const next = [...rows];
                  next[idx] = { ...row, secret: e.target.checked };
                  onChange(next);
                }}
                className="w-3.5 h-3.5 rounded border-[rgb(var(--border))]"
              />
              {t('customServerPanel.form.inputSecret')}
            </label>
          </div>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          onChange([
            ...rows,
            { id: '', label: '', type: 'text', required: false, secret: false },
          ])
        }
        className="text-xs text-[rgb(var(--primary))] hover:underline"
        data-testid="custom-server-input-def-add"
      >
        {t('customServerPanel.form.addInputDef')}
      </button>
    </div>
  );
}

/**
 * Guided form fields for custom server creation (Required + Optional sections).
 */
function CustomServerFormBody({
  form,
  onChange,
  formErrors,
}: {
  form: CustomServerFormState;
  onChange: (form: CustomServerFormState) => void;
  formErrors: string[];
}) {
  const { t } = useTranslation('servers');

  const setTransport = (transportType: CustomServerTransportType) => {
    onChange({ ...form, transportType });
  };

  return (
    <div className="space-y-4">
      {formErrors.length > 0 && (
        <div
          className="rounded-lg border border-[rgb(var(--error))]/30 bg-[rgb(var(--error))]/5 p-3 space-y-1"
          data-testid="custom-server-form-errors"
        >
          {formErrors.map((msg) => (
            <p key={msg} className="text-xs text-[rgb(var(--error))] flex items-center gap-1.5">
              <AlertTriangle className="h-3 w-3 flex-shrink-0" />
              {msg}
            </p>
          ))}
        </div>
      )}

      <CollapsibleSection
        icon={<Settings className="h-5 w-5" />}
        tone="primary"
        title={t('customServerPanel.form.requiredSection')}
        subtitle={t('customServerPanel.form.requiredSectionDesc')}
        defaultOpen
        testId="custom-server-required-section"
      >
        <div className="space-y-4">
          <div>
            <label
              htmlFor="custom-server-form-id"
              className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
            >
              {t('customServerPanel.serverId')}
              <span className="text-[rgb(var(--error))] ml-1">*</span>
            </label>
            <p className="text-xs text-[rgb(var(--muted))] mb-2">
              {t('customServerPanel.serverIdDesc')}
            </p>
            <input
              id="custom-server-form-id"
              type="text"
              value={form.serverId}
              onChange={(e) => onChange({ ...form, serverId: e.target.value })}
              placeholder={t('customServerPanel.serverIdPlaceholder')}
              className="input w-full font-mono"
              data-testid="custom-server-form-server-id"
            />
          </div>

          <div>
            <label
              htmlFor="custom-server-form-name"
              className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
            >
              {t('customServerPanel.form.displayName')}
              <span className="text-[rgb(var(--error))] ml-1">*</span>
            </label>
            <p className="text-xs text-[rgb(var(--muted))] mb-2">
              {t('customServerPanel.form.displayNameDesc')}
            </p>
            <input
              id="custom-server-form-name"
              type="text"
              value={form.displayName}
              onChange={(e) => onChange({ ...form, displayName: e.target.value })}
              placeholder={t('customServerPanel.form.displayNamePlaceholder')}
              className="input w-full"
              data-testid="custom-server-form-display-name"
            />
          </div>

          <div>
            <span className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
              {t('customServerPanel.form.transportType')}
              <span className="text-[rgb(var(--error))] ml-1">*</span>
            </span>
            <p className="text-xs text-[rgb(var(--muted))] mb-2">
              {t('customServerPanel.form.transportTypeDesc')}
            </p>
            <div className="inline-flex rounded-lg border border-[rgb(var(--border))] p-0.5 gap-0.5">
              {(['stdio', 'http'] as const).map((type) => (
                <button
                  key={type}
                  type="button"
                  onClick={() => setTransport(type)}
                  aria-pressed={form.transportType === type}
                  className={[
                    'px-3 py-1.5 text-xs font-medium rounded-md transition-all',
                    form.transportType === type
                      ? 'bg-[rgb(var(--primary))] text-[rgb(var(--primary-foreground))]'
                      : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))]',
                  ].join(' ')}
                  data-testid={`custom-server-form-transport-${type}`}
                >
                  {t(`customServerPanel.form.transport${type === 'stdio' ? 'Stdio' : 'Http'}`)}
                </button>
              ))}
            </div>
          </div>

          {form.transportType === 'stdio' ? (
            <div>
              <label
                htmlFor="custom-server-form-command"
                className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
              >
                {t('customServerPanel.form.command')}
                <span className="text-[rgb(var(--error))] ml-1">*</span>
              </label>
              <p className="text-xs text-[rgb(var(--muted))] mb-2">
                {t('customServerPanel.form.commandDesc')}
              </p>
              <input
                id="custom-server-form-command"
                type="text"
                value={form.command}
                onChange={(e) => onChange({ ...form, command: e.target.value })}
                placeholder={t('customServerPanel.form.commandPlaceholder')}
                className="input w-full font-mono"
                data-testid="custom-server-form-command"
              />
            </div>
          ) : (
            <div>
              <label
                htmlFor="custom-server-form-url"
                className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
              >
                {t('customServerPanel.form.url')}
                <span className="text-[rgb(var(--error))] ml-1">*</span>
              </label>
              <p className="text-xs text-[rgb(var(--muted))] mb-2">
                {t('customServerPanel.form.urlDesc')}
              </p>
              <input
                id="custom-server-form-url"
                type="url"
                value={form.url}
                onChange={(e) => onChange({ ...form, url: e.target.value })}
                placeholder={t('customServerPanel.form.urlPlaceholder')}
                className="input w-full font-mono"
                data-testid="custom-server-form-url"
              />
            </div>
          )}
        </div>
      </CollapsibleSection>

      <CollapsibleSection
        icon={<SlidersHorizontal className="h-5 w-5" />}
        tone="purple"
        title={t('customServerPanel.form.optionalSection')}
        subtitle={t('customServerPanel.form.optionalSectionDesc')}
        defaultOpen
        testId="custom-server-optional-section"
      >
        <div className="space-y-4">
          <div>
            <label
              htmlFor="custom-server-form-description"
              className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
            >
              {t('customServerPanel.form.description')}
            </label>
            <textarea
              id="custom-server-form-description"
              value={form.description}
              onChange={(e) => onChange({ ...form, description: e.target.value })}
              placeholder={t('customServerPanel.form.descriptionPlaceholder')}
              rows={2}
              className="input w-full resize-y"
              data-testid="custom-server-form-description"
            />
          </div>

          {form.transportType === 'stdio' && (
            <div>
              <label
                htmlFor="custom-server-form-args"
                className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
              >
                {t('customServerPanel.form.args')}
              </label>
              <p className="text-xs text-[rgb(var(--muted))] mb-2">
                {t('customServerPanel.form.argsDesc')}
              </p>
              <textarea
                id="custom-server-form-args"
                value={form.argsText}
                onChange={(e) => onChange({ ...form, argsText: e.target.value })}
                onBlur={(e) =>
                  onChange({
                    ...form,
                    argsText: e.target.value
                      .split('\n')
                      .filter((line) => line.trim().length > 0)
                      .join('\n'),
                  })
                }
                placeholder={t('customServerPanel.form.argsPlaceholder')}
                rows={3}
                className="input w-full font-mono text-sm resize-y"
                data-testid="custom-server-form-args"
              />
            </div>
          )}

          {form.transportType === 'stdio' && (
            <div>
              <span className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                {t('customServerPanel.form.envVars')}
              </span>
              <p className="text-xs text-[rgb(var(--muted))] mb-2">
                {t('customServerPanel.form.envVarsStdio')}
              </p>
              <KeyValueEditor
                rows={form.envRows}
                onChange={(envRows) => onChange({ ...form, envRows })}
                keyPlaceholder={t('customServerPanel.form.keyPlaceholder')}
                valuePlaceholder={t('customServerPanel.form.valuePlaceholder')}
                removeLabel={t('customServerPanel.form.removeRow')}
                addLabel={t('customServerPanel.form.addEnvVar')}
                testIdPrefix="custom-server-form-env"
              />
            </div>
          )}

          {form.transportType === 'http' && (
            <div>
              <span className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
                {t('customServerPanel.form.httpHeaders')}
              </span>
              <p className="text-xs text-[rgb(var(--muted))] mb-2">
                {t('customServerPanel.form.httpHeadersDesc')}
              </p>
              <KeyValueEditor
                rows={form.headerRows}
                onChange={(headerRows) => onChange({ ...form, headerRows })}
                keyPlaceholder={t('customServerPanel.form.headerNamePlaceholder')}
                valuePlaceholder={t('customServerPanel.form.valuePlaceholder')}
                removeLabel={t('customServerPanel.form.removeRow')}
                addLabel={t('customServerPanel.form.addHeader')}
                testIdPrefix="custom-server-form-header"
              />
            </div>
          )}

          <div>
            <span className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1">
              {t('customServerPanel.form.inputDefs')}
            </span>
            <p className="text-xs text-[rgb(var(--muted))] mb-2">
              {t('customServerPanel.form.inputDefsDesc')}
            </p>
            <InputDefEditor
              rows={form.inputDefs}
              onChange={(inputDefs) => onChange({ ...form, inputDefs })}
            />
          </div>

          <div>
            <label
              htmlFor="custom-server-form-default-params"
              className="block text-sm font-medium text-[rgb(var(--foreground))] mb-1"
            >
              {t('customServerPanel.form.defaultParams')}
            </label>
            <p className="text-xs text-[rgb(var(--muted))] mb-2">
              {t('customServerPanel.form.defaultParamsDesc')}{' '}
              <code className="font-mono">{`{"cloudId": "abc123"}`}</code>
            </p>
            <textarea
              id="custom-server-form-default-params"
              value={form.defaultParamsJson}
              onChange={(e) => onChange({ ...form, defaultParamsJson: e.target.value })}
              placeholder="{}"
              rows={3}
              className="input w-full font-mono text-sm resize-y"
              spellCheck={false}
              data-testid="custom-server-form-default-params"
            />
            <div className="flex items-center gap-2 mt-2">
              <label className="text-xs text-[rgb(var(--muted))]">
                {t('customServerPanel.form.onCollision')}
              </label>
              <select
                value={form.defaultParamsStrategy}
                onChange={(e) =>
                  onChange({
                    ...form,
                    defaultParamsStrategy: e.target.value as 'fill' | 'override',
                  })
                }
                className="text-xs border border-[rgb(var(--border))] rounded px-2 py-1 bg-[rgb(var(--surface))] text-[rgb(var(--foreground))]"
                data-testid="custom-server-form-default-params-strategy"
              >
                <option value="fill">{t('customServerPanel.form.callerWins')}</option>
                <option value="override">{t('customServerPanel.form.defaultsWin')}</option>
              </select>
            </div>
          </div>
        </div>
      </CollapsibleSection>
    </div>
  );
}

/**
 * Validate guided form required fields and default-params JSON before save.
 */
function collectFormValidationErrors(
  form: CustomServerFormState,
  t: TFunction<'servers'>,
): string[] {
  const errors: string[] = [];
  if (!form.serverId.trim()) {
    errors.push(t('customServerPanel.form.validation.serverIdRequired'));
  }
  if (!form.displayName.trim()) {
    errors.push(t('customServerPanel.form.validation.displayNameRequired'));
  }
  if (form.transportType === 'stdio' && !form.command.trim()) {
    errors.push(t('customServerPanel.form.validation.commandRequired'));
  }
  if (form.transportType === 'http' && !form.url.trim()) {
    errors.push(t('customServerPanel.form.validation.urlRequired'));
  }
  const defaultParamsTrimmed = form.defaultParamsJson.trim();
  if (defaultParamsTrimmed && defaultParamsTrimmed !== '{}') {
    if (!parseDefaultParamsJson(form.defaultParamsJson)) {
      errors.push(t('customServerPanel.form.validation.defaultParamsInvalid'));
    }
  }
  return errors;
}

/**
 * Slide-in panel for adding a custom server via guided form or JSON editor.
 */
export function CustomServerPanel({ spaceId, onClose, onSaved }: CustomServerPanelProps) {
  const { t } = useTranslation('servers');
  const { success, error: showError } = useToast();

  const [mode, setMode] = useState<PanelMode>('form');
  const [serverKey, setServerKey] = useState('');
  const [formState, setFormState] = useState<CustomServerFormState>(() =>
    createDefaultFormState(''),
  );
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

  const formValidationErrors = useMemo(
    () => (mode === 'form' ? collectFormValidationErrors(formState, t) : []),
    [mode, formState, t],
  );
  const isFormValid = formValidationErrors.length === 0;

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
      setFormState(createDefaultFormState(key));
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
    const trimmedKey =
      mode === 'form' ? formState.serverId.trim() : serverKey.trim();

    if (!trimmedKey) {
      showError(
        t('customServerPanel.toast.saveFailed'),
        t('customServerPanel.toast.serverIdRequired'),
      );
      return;
    }

    let entry: Record<string, unknown>;

    if (mode === 'form') {
      const errors = collectFormValidationErrors(formState, t);
      if (errors.length > 0) {
        showError(t('customServerPanel.toast.saveFailed'), errors[0]);
        return;
      }
      entry = buildServerEntryFromForm(formState);
    } else {
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
    mode,
    formState,
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

  const isSaveDisabled =
    isSaving ||
    isLoading ||
    !!loadError ||
    (mode === 'json' && !isValidJson) ||
    (mode === 'form' && !isFormValid);

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
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-3 flex-1 min-w-0">
              <div className="w-11 h-11 flex-shrink-0 flex items-center justify-center bg-[rgb(var(--background))] rounded-lg border border-[rgb(var(--border-subtle))]">
                <Plus className="h-5 w-5 text-[rgb(var(--primary))]" />
              </div>
              <h2 className="text-lg font-bold text-[rgb(var(--foreground))] truncate">
                {t('customServerPanel.title')}
              </h2>
            </div>
            <div className="flex items-center gap-2 flex-shrink-0">
              <ModeToggle
                mode={mode}
                onChange={setMode}
                formLabel={t('customServerPanel.modeForm')}
                jsonLabel={t('customServerPanel.modeJson')}
              />
              <button
                type="button"
                onClick={onClose}
                className="p-1.5 rounded-lg hover:bg-[rgb(var(--surface-hover))] transition-colors"
                aria-label={t('customServerPanel.closeAria')}
              >
                <X className="h-5 w-5" />
              </button>
            </div>
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
            <CustomServerFormBody
              form={formState}
              onChange={setFormState}
              formErrors={formValidationErrors}
            />
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
              disabled={isSaveDisabled}
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
