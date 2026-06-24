/**
 * OAuth Consent Modal
 *
 * Single decision: allow or deny. No naming, no follow-up routing screen —
 * routing is a post-connection decision, surfaced by the workspace binding
 * sheet when the first session reports roots that have no binding yet.
 */

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { AlertCircle, Check, Loader2, X } from 'lucide-react';
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
  setProcessError: (error: string | null) => void
): Promise<void> {
  console.log('[OAuth] loadConsentRequest start:', requestId);
  setModalState({ type: 'loading', requestId });
  setProcessError(null);

  try {
    const details = await getPendingConsent(requestId);
    console.log('[OAuth] loadConsentRequest OK:', details.clientName, details.clientId);
    setModalState({ type: 'consent', details });
  } catch (err) {
    console.error('[OAuth] loadConsentRequest failed:', err);
    const error = err as ConsentError;
    setModalState({ type: 'error', requestId, error });
  }
}

type ModalState =
  | { type: 'hidden' }
  | { type: 'loading'; requestId: string }
  | { type: 'error'; requestId: string; error: ConsentError }
  | { type: 'consent'; details: ConsentRequestDetails };

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

export function OAuthConsentModal() {
  const { t } = useTranslation('clients');
  const [modalState, setModalState] = useState<ModalState>({ type: 'hidden' });
  const [isProcessing, setIsProcessing] = useState(false);
  const [processError, setProcessError] = useState<string | null>(null);
  /** 1.5-second cooldown before Allow becomes active — prevents accidental taps. */
  const [approveReady, setApproveReady] = useState(false);

  useEffect(() => {
    if (modalState.type === 'consent') {
      setApproveReady(false);
      const timer = setTimeout(() => setApproveReady(true), 1500);
      return () => clearTimeout(timer);
    }
    setApproveReady(false);
  }, [modalState.type]);

  useEffect(() => {
    console.log('[OAuth] OAuthConsentModal mounting — subscribing to consent events');
    return subscribeOAuthConsentEvents((payload) => {
      console.log('[OAuth] OAuthConsentModal handler fired:', payload);
      void loadConsentRequest(payload.requestId, setModalState, setProcessError);
    });
  }, []);

  const handleApprove = async () => {
    if (modalState.type !== 'consent') return;
    const { details } = modalState;

    setIsProcessing(true);
    setProcessError(null);

    try {
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
      } else {
        setProcessError(response.error || t('oauthConsent.approveFailed'));
      }
    } catch (err) {
      console.error('[OAuth] Failed to approve consent:', err);
      setProcessError(String(err));
    } finally {
      setIsProcessing(false);
    }
  };

  const handleDeny = async () => {
    if (modalState.type !== 'consent') return;
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
            <p className="text-[rgb(var(--muted))]">
              {t('oauthConsent.validating')}
            </p>
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
                <CardTitle data-testid="oauth-consent-error-title">{t('oauthConsent.errorTitle')}</CardTitle>
                <CardDescription>
                  {t('oauthConsent.errorDesc')}
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-[rgb(var(--muted))]" data-testid="oauth-consent-error-message">
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

  const { details } = modalState;
  const logoUrl = getClientLogo(details.clientName);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <Card className="animate-in fade-in zoom-in mx-4 w-full max-w-sm shadow-xl duration-200">
        <CardContent className="flex flex-col items-center gap-5 px-8 pb-6 pt-8 text-center">
          {logoUrl ? (
            <img
              src={logoUrl}
              alt={details.clientName}
              className="h-14 w-14 rounded-2xl shadow-sm"
            />
          ) : (
            <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-[rgb(var(--surface))] text-xl font-semibold text-[rgb(var(--foreground))]">
              {details.clientName.slice(0, 1).toUpperCase()}
            </div>
          )}

          <div className="space-y-1.5">
            <h2 className="text-lg font-semibold text-[rgb(var(--foreground))]">
              {t('oauthConsent.allowTitle', { clientName: details.clientName })}
            </h2>
            <p className="text-sm text-[rgb(var(--muted))]">
              {t('oauthConsent.allowDesc')}
            </p>
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
