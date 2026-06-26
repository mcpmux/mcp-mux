/**
 * OAuth Consent Modal
 *
 * Allow or deny inbound OAuth clients. First-time clients must name (or pick)
 * a machine before approval; returning clients get the simple allow/deny card.
 */

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { AlertCircle, Check, Loader2, Plus, X } from 'lucide-react';
import {
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@mcpmux/ui';
import { resolveKnownClientKey } from '@/lib/clientIcons';
import {
  approveOAuthConsent,
  getPendingConsent,
  type ConsentError,
  type ConsentRequestDetails,
} from '@/lib/api/oauth';
import {
  createMachine,
  getHostname,
  listMachines,
  setClientMachineId,
  type Machine,
} from '@/lib/api/machines';
import { subscribeOAuthConsentEvents } from '@/lib/backend/shell';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';
import jetbrainsIcon from '@/assets/client-icons/jetbrains.svg';
import androidStudioIcon from '@/assets/client-icons/android-studio.svg';

const CLIENT_ICON_ASSETS: Record<string, string> = {
  cursor: cursorIcon,
  vscode: vscodeIcon,
  claude: claudeIcon,
  windsurf: windsurfIcon,
  jetbrains: jetbrainsIcon,
  'android-studio': androidStudioIcon,
};

type ModalState =
  | { type: 'hidden' }
  | { type: 'loading'; requestId: string }
  | { type: 'error'; requestId: string; error: ConsentError }
  | { type: 'name-machine'; details: ConsentRequestDetails; machines: Machine[] }
  | { type: 'consent'; details: ConsentRequestDetails };

/**
 * Resolve a known client logo asset URL from the client display name.
 */
function getClientLogo(clientName: string): string | null {
  const key = resolveKnownClientKey(clientName);
  return key ? (CLIENT_ICON_ASSETS[key] ?? null) : null;
}

/**
 * Load consent details for a request id and transition modal state.
 */
async function loadConsentRequest(
  requestId: string,
  setModalState: (state: ModalState) => void,
  setProcessError: (error: string | null) => void,
  onNewClient: (hostname: string) => void
): Promise<void> {
  console.log('[OAuth] loadConsentRequest start:', requestId);
  setModalState({ type: 'loading', requestId });
  setProcessError(null);

  try {
    const details = await getPendingConsent(requestId);
    console.log('[OAuth] loadConsentRequest OK:', details.clientName, details.clientId);

    if (details.isNewClient) {
      const [machines, hostname] = await Promise.all([listMachines(), getHostname()]);
      onNewClient(hostname);
      setModalState({ type: 'name-machine', details, machines });
      return;
    }

    setModalState({ type: 'consent', details });
  } catch (err) {
    console.error('[OAuth] loadConsentRequest failed:', err);
    const error = err as ConsentError;
    setModalState({ type: 'error', requestId, error });
  }
}

/**
 * Deliver the OAuth redirect without opening a dead browser tab for loopback URIs.
 */
async function openRedirectUrl(url: string): Promise<void> {
  const { openUrl } = await import('@/lib/backend/shell');
  await openUrl(url);
}

/**
 * Map gateway consent errors to user-facing copy.
 */
function getErrorMessage(t: TFunction<'clients'>, error: ConsentError): string {
  switch (error.code) {
    case 'NOT_FOUND':
      return t('oauthConsent.errors.notFound');
    case 'EXPIRED':
      return t('oauthConsent.errors.expired');
    case 'ALREADY_PROCESSED':
      return t('oauthConsent.errors.alreadyProcessed');
    case 'GATEWAY_UNAVAILABLE':
      return t('oauthConsent.errors.gatewayUnavailable');
    default:
      return error.message;
  }
}

/**
 * Radio-style selectable row for machine picker.
 */
function ChoiceRow({
  selected,
  onSelect,
  title,
  subtitle,
}: {
  selected: boolean;
  onSelect: () => void;
  title: string;
  subtitle?: string;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className={[
        'group flex w-full items-start gap-3 rounded-xl border px-4 py-3 text-left transition-all',
        selected
          ? 'border-primary-500 bg-primary-50 shadow-sm dark:bg-primary-900/20 dark:border-primary-400'
          : 'border-[rgb(var(--border))] bg-[rgb(var(--background))] hover:border-[rgb(var(--border-strong,var(--border)))] hover:bg-[rgb(var(--surface-hover,var(--surface)))]',
      ].join(' ')}
    >
      <div
        className={[
          'mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full border-2 transition-all',
          selected
            ? 'border-primary-500 bg-primary-500 dark:border-primary-400 dark:bg-primary-400'
            : 'border-[rgb(var(--border))] bg-[rgb(var(--background))] group-hover:border-[rgb(var(--muted))]',
        ].join(' ')}
      >
        {selected && <Check className="h-3 w-3 text-white" strokeWidth={3.5} />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-[rgb(var(--foreground))]">{title}</div>
        {subtitle && (
          <div className="mt-0.5 text-xs text-[rgb(var(--muted))]">{subtitle}</div>
        )}
      </div>
    </button>
  );
}

/**
 * Render client logo or first-letter fallback.
 */
function ClientLogo({ clientName }: { clientName: string }) {
  const logoUrl = getClientLogo(clientName);
  if (logoUrl) {
    return (
      <img
        src={logoUrl}
        alt={clientName}
        className="h-14 w-14 rounded-2xl shadow-sm"
      />
    );
  }
  return (
    <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-[rgb(var(--surface))] text-xl font-semibold text-[rgb(var(--foreground))]">
      {clientName.slice(0, 1).toUpperCase()}
    </div>
  );
}

export function OAuthConsentModal() {
  const { t } = useTranslation('clients');
  const [modalState, setModalState] = useState<ModalState>({ type: 'hidden' });
  const [isProcessing, setIsProcessing] = useState(false);
  const [processError, setProcessError] = useState<string | null>(null);
  /** 1.5-second cooldown before Allow becomes active — prevents accidental taps. */
  const [approveReady, setApproveReady] = useState(false);
  const [machineName, setMachineName] = useState('');
  const [selectedMachineId, setSelectedMachineId] = useState('');
  const [creatingMachine, setCreatingMachine] = useState(false);
  const activeRequestIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (modalState.type === 'consent' || modalState.type === 'name-machine') {
      setApproveReady(false);
      const timer = setTimeout(() => setApproveReady(true), 1500);
      return () => clearTimeout(timer);
    }
    setApproveReady(false);
  }, [modalState]);

  useEffect(() => {
    console.log('[OAuth] OAuthConsentModal mounting — subscribing to consent events');
    return subscribeOAuthConsentEvents((payload) => {
      console.log('[OAuth] OAuthConsentModal handler fired:', payload);
      if (payload.requestId === activeRequestIdRef.current) {
        console.log('[OAuth] Duplicate event for same requestId — ignoring:', payload.requestId);
        return;
      }
      activeRequestIdRef.current = payload.requestId;
      void loadConsentRequest(payload.requestId, setModalState, setProcessError, (hostname) => {
        setMachineName(hostname);
        setSelectedMachineId('');
        setCreatingMachine(false);
      });
    });
  }, []);

  /**
   * Complete OAuth approval after persisting the redirect URL response.
   */
  const finishApproval = async (details: ConsentRequestDetails) => {
    const response = await approveOAuthConsent({
      request_id: details.requestId,
      approved: true,
      consent_token: details.consentToken,
      client_alias: null,
    });

    if (response.success) {
      if (response.redirect_url) {
        try {
          await openRedirectUrl(response.redirect_url);
        } catch (redirectErr) {
          console.warn('[OAuth] Redirect delivery failed after approve:', redirectErr);
        }
      }
      setModalState({ type: 'hidden' });
      return;
    }

    setProcessError(response.error || t('oauthConsent.approveFailed'));
  };

  const handleApprove = async () => {
    if (modalState.type !== 'consent') return;
    const { details } = modalState;

    setIsProcessing(true);
    setProcessError(null);

    try {
      await finishApproval(details);
    } catch (err) {
      console.error('[OAuth] Failed to approve consent:', err);
      setProcessError(String(err));
    } finally {
      setIsProcessing(false);
    }
  };

  /**
   * Create or pick a machine, link it to the client, then approve OAuth consent.
   */
  const handleNameAndAllow = async () => {
    if (modalState.type !== 'name-machine') return;
    const { details } = modalState;

    let machineId = selectedMachineId;
    if (creatingMachine) {
      const name = machineName.trim();
      if (!name) {
        setProcessError(t('oauthConsent.nameMachine.nameRequired'));
        return;
      }
    } else if (!machineId) {
      setProcessError(t('oauthConsent.nameMachine.nameRequired'));
      return;
    }

    setIsProcessing(true);
    setProcessError(null);

    try {
      if (creatingMachine) {
        const created = await createMachine({ name: machineName.trim() });
        machineId = created.id;
      }

      await setClientMachineId(details.clientId, machineId);
      await finishApproval(details);
    } catch (err) {
      console.error('[OAuth] Failed to name machine and approve:', err);
      setProcessError(String(err));
    } finally {
      setIsProcessing(false);
    }
  };

  const handleDeny = async () => {
    if (modalState.type !== 'consent' && modalState.type !== 'name-machine') return;
    const { details } = modalState;

    setIsProcessing(true);
    setProcessError(null);

    try {
      const response = await approveOAuthConsent({
        request_id: details.requestId,
        approved: false,
        consent_token: details.consentToken,
        client_alias: null,
      });

      if (response.success) {
        if (response.redirect_url) {
          try {
            await openRedirectUrl(response.redirect_url);
          } catch (redirectErr) {
            console.warn('[OAuth] Redirect delivery failed after deny:', redirectErr);
          }
        }
        setModalState({ type: 'hidden' });
      } else {
        setProcessError(response.error || t('oauthConsent.denyFailed'));
      }
    } catch (err) {
      console.error('[OAuth] Failed to deny consent:', err);
      setProcessError(String(err));
    } finally {
      setIsProcessing(false);
    }
  };

  const handleDismiss = () => {
    activeRequestIdRef.current = null;
    setModalState({ type: 'hidden' });
    setProcessError(null);
  };

  if (modalState.type === 'hidden') return null;

  if (modalState.type === 'loading') {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
        <Card className="animate-in fade-in zoom-in mx-4 w-full max-w-md shadow-xl duration-200">
          <CardContent className="flex flex-col items-center gap-4 py-8">
            <Loader2 className="text-primary-500 h-8 w-8 animate-spin" />
            <p className="text-[rgb(var(--muted))]">{t('oauthConsent.validating')}</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  if (modalState.type === 'error') {
    return (
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
        data-testid="oauth-consent-error-modal"
      >
        <Card className="animate-in fade-in zoom-in mx-4 w-full max-w-md shadow-xl duration-200">
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="rounded-full bg-red-500/10 p-2">
                <AlertCircle className="h-6 w-6 text-red-500" />
              </div>
              <div>
                <CardTitle data-testid="oauth-consent-error-title">
                  {t('oauthConsent.errorTitle')}
                </CardTitle>
                <CardDescription>{t('oauthConsent.errorDesc')}</CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <p
              className="text-sm text-[rgb(var(--muted))]"
              data-testid="oauth-consent-error-message"
            >
              {getErrorMessage(t, modalState.error)}
            </p>
            <Button onClick={handleDismiss} className="w-full">
              {t('oauthConsent.close')}
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  if (modalState.type === 'name-machine') {
    const { details, machines } = modalState;
    const canSubmit =
      creatingMachine ? machineName.trim().length > 0 : selectedMachineId.length > 0;

    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
        <Card className="animate-in fade-in zoom-in mx-4 w-full max-w-md shadow-xl duration-200">
          <CardContent className="flex flex-col gap-5 px-6 pb-6 pt-8">
            <div className="flex flex-col items-center gap-4 text-center">
              <ClientLogo clientName={details.clientName} />
              <div className="space-y-1.5">
                <h2 className="text-lg font-semibold text-[rgb(var(--foreground))]">
                  {t('oauthConsent.nameMachine.title')}
                </h2>
                <p className="text-sm text-[rgb(var(--muted))]">
                  {t('oauthConsent.nameMachine.desc', { clientName: details.clientName })}
                </p>
              </div>
            </div>

            <div>
              <div className="mb-3 text-xs font-medium uppercase tracking-wider text-[rgb(var(--muted))]">
                {t('oauthConsent.nameMachine.sectionLabel')}
              </div>
              <div className="max-h-48 space-y-1.5 overflow-y-auto">
                {machines.map((machine) => (
                  <ChoiceRow
                    key={machine.id}
                    selected={!creatingMachine && selectedMachineId === machine.id}
                    onSelect={() => {
                      setCreatingMachine(false);
                      setSelectedMachineId(machine.id);
                    }}
                    title={machine.icon ? `${machine.icon}  ${machine.name}` : machine.name}
                    subtitle={machine.hostname ?? undefined}
                  />
                ))}
                {!creatingMachine ? (
                  <button
                    type="button"
                    onClick={() => {
                      setCreatingMachine(true);
                      setSelectedMachineId('');
                    }}
                    className="flex w-full items-center gap-2 rounded-xl border border-dashed border-[rgb(var(--border))] px-4 py-3 text-sm text-[rgb(var(--muted))] transition-colors hover:border-[rgb(var(--accent))] hover:text-[rgb(var(--foreground))]"
                  >
                    <Plus className="h-4 w-4" />
                    {t('oauthConsent.nameMachine.newMachine')}
                  </button>
                ) : (
                  <div className="rounded-xl border border-[rgb(var(--border))] p-4">
                    <input
                      type="text"
                      value={machineName}
                      onChange={(e) => setMachineName(e.target.value)}
                      placeholder={t('oauthConsent.nameMachine.namePlaceholder')}
                      className="w-full rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 text-sm"
                      autoFocus
                    />
                  </div>
                )}
              </div>
            </div>

            {processError && (
              <div className="flex w-full items-start gap-2 rounded-lg bg-red-500/10 p-3 text-left text-sm text-red-500">
                <AlertCircle className="h-4 w-4 flex-shrink-0 translate-y-0.5" />
                <span>{processError}</span>
              </div>
            )}

            <div className="flex w-full flex-col gap-2">
              <Button
                variant="primary"
                className="w-full"
                onClick={handleNameAndAllow}
                disabled={isProcessing || !approveReady || !canSubmit}
              >
                {isProcessing ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Check className="mr-2 h-4 w-4" />
                )}
                {approveReady
                  ? t('oauthConsent.nameMachine.allowBtn')
                  : t('oauthConsent.nameMachine.allowWait')}
              </Button>
              <Button
                variant="secondary"
                className="w-full"
                onClick={handleDeny}
                disabled={isProcessing}
              >
                <X className="mr-2 h-4 w-4" />
                {t('oauthConsent.deny')}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    );
  }

  const { details } = modalState;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <Card className="animate-in fade-in zoom-in mx-4 w-full max-w-sm shadow-xl duration-200">
        <CardContent className="flex flex-col items-center gap-5 px-8 pb-6 pt-8 text-center">
          <ClientLogo clientName={details.clientName} />

          <div className="space-y-1.5">
            <h2 className="text-lg font-semibold text-[rgb(var(--foreground))]">
              {t('oauthConsent.allowTitle', { clientName: details.clientName })}
            </h2>
            <p className="text-sm text-[rgb(var(--muted))]">{t('oauthConsent.allowDesc')}</p>
          </div>

          {processError && (
            <div className="flex w-full items-start gap-2 rounded-lg bg-red-500/10 p-3 text-left text-sm text-red-500">
              <AlertCircle className="h-4 w-4 flex-shrink-0 translate-y-0.5" />
              <span>{processError}</span>
            </div>
          )}

          <div className="flex w-full flex-col gap-2">
            <Button
              variant="primary"
              className="w-full"
              onClick={handleApprove}
              disabled={isProcessing || !approveReady}
            >
              {isProcessing ? (
                <div className="mr-2 h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
              ) : (
                <Check className="mr-2 h-4 w-4" />
              )}
              {approveReady ? t('oauthConsent.allow') : t('oauthConsent.allowWait')}
            </Button>
            <Button
              variant="secondary"
              className="w-full"
              onClick={handleDeny}
              disabled={isProcessing}
            >
              <X className="mr-2 h-4 w-4" />
              {t('oauthConsent.deny')}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
