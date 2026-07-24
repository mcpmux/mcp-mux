/**
 * Register API-key client modal.
 *
 * Creates a pre-approved inbound client authenticated by a long-lived API key —
 * no browser/OAuth consent. Cursor tab builds a paste-ready ~/.cursor/mcp.json
 * snippet; Generic tab shows the raw key for headless/CI/remote clients.
 */

import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  KeyRound,
  Loader2,
  ShieldCheck,
  X,
} from 'lucide-react';
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, SearchableSelect } from '@mcpmux/ui';
import { registerApiKeyClient, type RegisteredApiKeyClient } from '@/lib/api/gateway';
import {
  createMachine,
  getHostname,
  listMachines,
  setClientMachineId,
  type Machine,
} from '@/lib/api/machines';
import {
  getMissingMachineProfileField,
  toMachineProfilePayload,
} from '@/lib/machine-profile.helpers';
import { EmojiPickerButton } from '@/components/emoji-picker-button.component';
import { useSpaces } from '@/stores';
import {
  buildCursorBridgeMcpJson,
  CURSOR_BRIDGE_CLIENT_NAME,
} from './cursor-bridge-config.helpers';

/** Sentinel value that reveals the shared "create new machine" sub-form. */
const NEW_MACHINE_OPTION = '__new__';

type ClientPresetTab = 'cursor' | 'generic';

interface RegisterApiKeyClientModalProps {
  onClose: () => void;
  /** Called once the client + key are created, so the page can refresh. */
  onRegistered: (client: RegisteredApiKeyClient) => void;
  gatewayUrl: string;
}

/**
 * Modal flow to register a preregistered client — Cursor preset (mcp.json snippet) or Generic (raw key).
 */
export function RegisterApiKeyClientModal({
  onClose,
  onRegistered,
  gatewayUrl,
}: RegisterApiKeyClientModalProps) {
  const { t } = useTranslation('clients');
  const spaces = useSpaces();
  const [activeTab, setActiveTab] = useState<ClientPresetTab>('cursor');
  const [name, setName] = useState(CURSOR_BRIDGE_CLIENT_NAME);
  const [lockedSpaceId, setLockedSpaceId] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<RegisteredApiKeyClient | null>(null);
  const [resultTab, setResultTab] = useState<ClientPresetTab>('cursor');
  const [cursorSnippet, setCursorSnippet] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [snippetCopied, setSnippetCopied] = useState(false);
  const [comparisonOpen, setComparisonOpen] = useState(false);

  const [machines, setMachines] = useState<Machine[]>([]);
  const [selectedMachineId, setSelectedMachineId] = useState('');
  const [machineName, setMachineName] = useState('');
  const [machineIcon, setMachineIcon] = useState('');
  const [machineHostname, setMachineHostname] = useState('');
  const isCreatingMachine = selectedMachineId === NEW_MACHINE_OPTION;

  useEffect(() => {
    void listMachines()
      .then(setMachines)
      .catch(() => undefined);
  }, []);

  const spaceOptions = useMemo(
    () => [
      { value: '', label: t('registerModal.space.noLock') },
      ...spaces.map((space) => ({
        value: space.id,
        label: space.name,
        icon: space.icon ?? undefined,
      })),
    ],
    [spaces, t]
  );

  const machineOptions = useMemo(
    () => [
      { value: '', label: t('registerModal.machine.noMachine') },
      ...machines.map((machine) => ({
        value: machine.id,
        label: machine.name,
        icon: machine.icon ?? undefined,
      })),
    ],
    [machines, t]
  );

  /**
   * Switch preset tab and apply sensible default client name per tab.
   */
  const handleTabChange = (tab: ClientPresetTab) => {
    setActiveTab(tab);
    if (tab === 'cursor' && !name.trim()) {
      setName(CURSOR_BRIDGE_CLIENT_NAME);
    }
  };

  /**
   * Mint the API-key client, optionally tag a machine, and set result output per tab.
   */
  const mintClient = async (): Promise<RegisteredApiKeyClient | null> => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError('Give the client a name so you can recognise it later.');
      return null;
    }
    if (isCreatingMachine) {
      const missingField = getMissingMachineProfileField({
        name: machineName,
        icon: machineIcon,
        hostname: machineHostname,
      });
      if (missingField) {
        setError(`New machine ${missingField} is required, or switch back to "No machine".`);
        return null;
      }
    }

    setIsSubmitting(true);
    setError(null);
    try {
      const client = await registerApiKeyClient(
        trimmed,
        lockedSpaceId.trim() ? lockedSpaceId.trim() : null
      );

      let machineId: string | null = null;
      if (isCreatingMachine) {
        const created = await createMachine(
          toMachineProfilePayload({
            name: machineName,
            icon: machineIcon,
            hostname: machineHostname,
          })
        );
        machineId = created.id;
      } else if (selectedMachineId) {
        machineId = selectedMachineId;
      }
      if (machineId) {
        await setClientMachineId(client.clientId, machineId);
      }

      return client;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setIsSubmitting(false);
    }
  };

  /**
   * Submit the form for the active preset tab.
   */
  const handleGenerate = async () => {
    const client = await mintClient();
    if (!client) return;

    setResultTab(activeTab);
    setResult(client);
    if (activeTab === 'cursor') {
      setCursorSnippet(buildCursorBridgeMcpJson(client.apiKey, gatewayUrl));
    } else {
      setCursorSnippet(null);
    }
  };

  /**
   * Regenerate Cursor snippet by minting a new client (same as legacy CursorBridgeSection).
   */
  const handleRegenerateCursor = async () => {
    const client = await mintClient();
    if (!client) return;

    setResult(client);
    setCursorSnippet(buildCursorBridgeMcpJson(client.apiKey, gatewayUrl));
  };

  const handleCopyKey = async () => {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.apiKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard can be unavailable; the field is selectable as a fallback.
    }
  };

  const handleCopySnippet = async () => {
    if (!cursorSnippet) return;
    try {
      await navigator.clipboard.writeText(cursorSnippet);
      setSnippetCopied(true);
      setTimeout(() => setSnippetCopied(false), 2000);
    } catch {
      // Snippet is selectable as a fallback.
    }
  };

  const handleDone = () => {
    if (result) onRegistered(result);
    onClose();
  };

  const comparisonRows = [
    {
      path: t('registerModal.comparison.cursorBridge.path'),
      bestFor: t('registerModal.comparison.cursorBridge.bestFor'),
      setup: t('registerModal.comparison.cursorBridge.setup'),
    },
    {
      path: t('registerModal.comparison.genericKey.path'),
      bestFor: t('registerModal.comparison.genericKey.bestFor'),
      setup: t('registerModal.comparison.genericKey.setup'),
    },
    {
      path: t('registerModal.comparison.workspacesInstall.path'),
      bestFor: t('registerModal.comparison.workspacesInstall.bestFor'),
      setup: t('registerModal.comparison.workspacesInstall.setup'),
    },
    {
      path: t('registerModal.comparison.oauthDeepLink.path'),
      bestFor: t('registerModal.comparison.oauthDeepLink.bestFor'),
      setup: t('registerModal.comparison.oauthDeepLink.setup'),
    },
  ];

  const machineCreateSubForm = isCreatingMachine ? (
    <div className="mt-3 space-y-3 rounded-xl border border-[rgb(var(--border))] p-4">
      <div className="flex items-center gap-2">
        <EmojiPickerButton value={machineIcon} onChange={setMachineIcon} />
        <input
          type="text"
          value={machineName}
          onChange={(e) => setMachineName(e.target.value)}
          placeholder="e.g. Cursor Web"
          className="h-10 min-w-0 flex-1 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 text-sm"
        />
      </div>
      <input
        type="text"
        value={machineHostname}
        onChange={(e) => setMachineHostname(e.target.value)}
        onFocus={() => {
          if (!machineHostname) {
            void getHostname()
              .then(setMachineHostname)
              .catch(() => undefined);
          }
        }}
        placeholder="Hostname"
        className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 font-mono text-sm"
      />
    </div>
  ) : null;

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
            {result
              ? resultTab === 'cursor'
                ? t('registerModal.cursor.resultTitle')
                : 'API key created'
              : t('registerModal.title')}
          </CardTitle>
          <CardDescription>
            {result
              ? resultTab === 'cursor'
                ? t('cursorBridge.pasteInto')
                : 'Copy the key now — this is the only time it will be shown.'
              : activeTab === 'cursor'
                ? t('cursorBridge.generateHint')
                : 'A pre-authorised client that connects with an API key instead of browser approval. Use this for headless, CI, or remote clients reaching the gateway over the network.'}
          </CardDescription>
        </CardHeader>

        <CardContent className="space-y-5">
          {result ? (
            resultTab === 'cursor' && cursorSnippet ? (
              <>
                <div>
                  <p className="mb-2 text-xs font-medium uppercase tracking-wide text-[rgb(var(--muted))]">
                    {t('cursorBridge.pasteInto')}
                  </p>
                  <pre
                    data-testid="cursor-bridge-snippet"
                    className="max-h-72 overflow-auto rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-3 font-mono text-xs"
                  >
                    {cursorSnippet}
                  </pre>
                </div>

                <div className="flex items-start gap-3 rounded-xl border border-amber-300 bg-amber-50 p-3.5 dark:border-amber-700/60 dark:bg-amber-900/20">
                  <AlertTriangle className="mt-0.5 h-5 w-5 flex-shrink-0 text-amber-600 dark:text-amber-400" />
                  <p className="text-sm text-amber-800 dark:text-amber-200">
                    {t('cursorBridge.keyOnceWarning')}
                  </p>
                </div>

                <div className="flex flex-wrap justify-end gap-2">
                  <Button
                    variant="secondary"
                    size="md"
                    onClick={() => void handleRegenerateCursor()}
                    disabled={isSubmitting}
                    data-testid="cursor-bridge-regenerate"
                  >
                    {isSubmitting ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <KeyRound className="mr-2 h-4 w-4" />
                    )}
                    {t('cursorBridge.regenerate')}
                  </Button>
                  <Button
                    variant="primary"
                    size="md"
                    onClick={handleCopySnippet}
                    data-testid="cursor-bridge-copy"
                  >
                    {snippetCopied ? (
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
                  <Button variant="primary" size="md" onClick={handleDone}>
                    Done
                  </Button>
                </div>

                <p className="text-xs text-[rgb(var(--muted))]">{t('cursorBridge.fallbackNote')}</p>
              </>
            ) : (
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
                    <Button variant="secondary" size="md" onClick={handleCopyKey}>
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
            )
          ) : (
            <>
              <div
                className="flex rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] p-1"
                role="tablist"
                aria-label={t('registerModal.title')}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab === 'cursor'}
                  data-testid="register-api-key-tab-cursor"
                  className={`flex-1 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
                    activeTab === 'cursor'
                      ? 'bg-[rgb(var(--accent))]/10 text-[rgb(var(--accent))]'
                      : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--text))]'
                  }`}
                  onClick={() => handleTabChange('cursor')}
                >
                  {t('registerModal.tabs.cursor')}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab === 'generic'}
                  data-testid="register-api-key-tab-generic"
                  className={`flex-1 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
                    activeTab === 'generic'
                      ? 'bg-[rgb(var(--accent))]/10 text-[rgb(var(--accent))]'
                      : 'text-[rgb(var(--muted))] hover:text-[rgb(var(--text))]'
                  }`}
                  onClick={() => handleTabChange('generic')}
                >
                  {t('registerModal.tabs.generic')}
                </button>
              </div>

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
                  placeholder={
                    activeTab === 'cursor'
                      ? CURSOR_BRIDGE_CLIENT_NAME
                      : 'e.g. CI runner, my-laptop, prod-bot'
                  }
                  className="w-full rounded-xl border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-3.5 py-2.5 text-sm transition-all focus:border-[rgb(var(--accent))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--accent))]/40"
                />
              </div>

              <div>
                <label className="mb-1.5 block text-sm font-medium">
                  {t('registerModal.space.label')}
                </label>
                <SearchableSelect
                  value={lockedSpaceId}
                  onChange={setLockedSpaceId}
                  options={spaceOptions}
                  placeholder={t('registerModal.space.noLock')}
                  testId="register-api-key-locked-space"
                />
                <p className="mt-1.5 text-xs text-[rgb(var(--muted))]">
                  {t('registerModal.space.hint')}
                </p>
              </div>

              <div>
                <label className="mb-1.5 block text-sm font-medium">
                  {t('registerModal.machine.label')}
                </label>
                <SearchableSelect
                  value={isCreatingMachine ? '' : selectedMachineId}
                  onChange={setSelectedMachineId}
                  options={machineOptions}
                  placeholder={t('registerModal.machine.noMachine')}
                  onCreateNew={() => setSelectedMachineId(NEW_MACHINE_OPTION)}
                  testId="register-api-key-machine"
                />
                {machineCreateSubForm}
                <p className="mt-1.5 text-xs text-[rgb(var(--muted))]">
                  {t('registerModal.machine.hint')}
                </p>
              </div>

              {activeTab === 'generic' && (
                <div className="flex items-start gap-3 rounded-xl border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] p-3.5">
                  <ShieldCheck className="mt-0.5 h-5 w-5 flex-shrink-0 text-[rgb(var(--accent))]" />
                  <p className="text-xs text-[rgb(var(--muted))]">
                    The key is generated on this machine, shown once, and stored only as a SHA-256
                    hash. The client then sends it as a Bearer token — no approval prompt needed.
                  </p>
                </div>
              )}

              {error && (
                <p
                  className="text-sm text-red-600 dark:text-red-400"
                  data-testid="register-api-key-error"
                >
                  {error}
                </p>
              )}

              <div className="rounded-xl border border-[rgb(var(--border-subtle))]">
                <button
                  type="button"
                  className="flex w-full items-center justify-between gap-2 px-3.5 py-3 text-left text-sm font-medium text-[rgb(var(--text))] transition-colors hover:bg-[rgb(var(--surface-hover))]"
                  onClick={() => setComparisonOpen((open) => !open)}
                  aria-expanded={comparisonOpen}
                  data-testid="register-api-key-comparison-toggle"
                >
                  {t('registerModal.whichShouldIUse')}
                  {comparisonOpen ? (
                    <ChevronDown className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
                  ) : (
                    <ChevronRight className="h-4 w-4 flex-shrink-0 text-[rgb(var(--muted))]" />
                  )}
                </button>
                {comparisonOpen && (
                  <div className="border-t border-[rgb(var(--border-subtle))] px-3.5 pb-3.5 pt-2">
                    <div className="overflow-x-auto">
                      <table className="w-full min-w-[28rem] text-left text-xs">
                        <thead>
                          <tr className="text-[rgb(var(--muted))]">
                            <th className="pb-2 pr-3 font-medium">{t('registerModal.comparison.pathColumn')}</th>
                            <th className="pb-2 pr-3 font-medium">{t('registerModal.comparison.bestForColumn')}</th>
                            <th className="pb-2 font-medium">{t('registerModal.comparison.setupColumn')}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {comparisonRows.map((row) => (
                            <tr
                              key={row.path}
                              className="border-t border-[rgb(var(--border-subtle))] text-[rgb(var(--text))]"
                            >
                              <td className="py-2 pr-3 align-top font-medium">{row.path}</td>
                              <td className="py-2 pr-3 align-top text-[rgb(var(--muted))]">{row.bestFor}</td>
                              <td className="py-2 align-top text-[rgb(var(--muted))]">{row.setup}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                )}
              </div>

              <div className="flex justify-end gap-2">
                <Button variant="ghost" size="md" onClick={onClose} disabled={isSubmitting}>
                  Cancel
                </Button>
                <Button
                  variant="primary"
                  size="md"
                  onClick={() => void handleGenerate()}
                  disabled={isSubmitting}
                  data-testid="register-api-key-generate"
                >
                  {isSubmitting ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      {activeTab === 'cursor' ? t('cursorBridge.generating') : 'Generating…'}
                    </>
                  ) : (
                    <>
                      <KeyRound className="mr-2 h-4 w-4" />
                      {activeTab === 'cursor' ? t('cursorBridge.generate') : 'Generate key'}
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
