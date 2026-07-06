/**
 * OAuth Consent Modal
 *
 * Single decision: allow or deny. No naming, no follow-up routing screen —
 * routing is a post-connection decision, surfaced by the workspace binding
 * sheet when the first session reports roots that have no binding yet.
 */

import { useEffect, useState } from 'react';
import { call as invoke } from '@/lib/transport';
import { listen } from '@tauri-apps/api/event';
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

interface OAuthDeepLinkPayload {
  requestId: string;
}

interface ConsentRequestDetails {
  requestId: string;
  clientId: string;
  clientName: string;
  redirectUri: string;
  scope: string;
  state: string | null;
  expiresAt: number;
  /** Cryptographic token shared only via Tauri IPC — must be sent back on approval. */
  consentToken: string;
}

interface ConsentError {
  code: 'NOT_FOUND' | 'EXPIRED' | 'ALREADY_PROCESSED' | 'GATEWAY_UNAVAILABLE';
  message: string;
}

interface ConsentApprovalResponse {
  success: boolean;
  redirect_url: string;
  error: string | null;
}

type ModalState =
  | { type: 'hidden' }
  | { type: 'loading'; requestId: string }
  | { type: 'error'; requestId: string; error: ConsentError }
  | { type: 'consent'; details: ConsentRequestDetails };

async function openRedirectUrl(url: string): Promise<void> {
  try {
    const { openUrl } = await import('@/lib/api/gateway');
    await openUrl(url);
  } catch (err) {
    console.error('[OAuth] openUrl failed:', err);
    try {
      const { openUrl: openUrlPlugin } = await import('@tauri-apps/plugin-opener');
      await openUrlPlugin(url);
    } catch (pluginErr) {
      console.error('[OAuth] Plugin opener also failed:', pluginErr);
      window.location.href = url;
    }
  }
}

function getErrorMessage(error: ConsentError): string {
  switch (error.code) {
    case 'NOT_FOUND':
      return 'This authorization request was not found. It may have expired or been processed already.';
    case 'EXPIRED':
      return 'This authorization request has expired. Please try again from your application.';
    case 'ALREADY_PROCESSED':
      return 'This authorization request has already been processed.';
    case 'GATEWAY_UNAVAILABLE':
      return 'The gateway service is not running. Please check that MCPMux is fully started.';
    default:
      return error.message;
  }
}

export function OAuthConsentModal() {
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
    const unlistenPromise = listen<OAuthDeepLinkPayload>(
      'oauth-consent-request',
      async (event) => {
        const requestId = event.payload.requestId;
        setModalState({ type: 'loading', requestId });
        setProcessError(null);

        try {
          const details = await invoke<ConsentRequestDetails>('get_pending_consent', {
            requestId,
          });
          setModalState({ type: 'consent', details });
        } catch (err) {
          console.error('[OAuth] Validation failed:', err);
          const error = err as ConsentError;
          setModalState({ type: 'error', requestId, error });
        }
      }
    );

    // Once the listener is subscribed, flush any cold-start URL buffered on
    // the Rust side (see PendingInitialDeepLink). Rust will re-fire
    // `oauth-consent-request` which the listener above then catches.
    unlistenPromise.then(() => {
      invoke('flush_pending_deep_link').catch((err) => {
        console.warn('[OAuth] flush_pending_deep_link failed:', err);
      });
    });

    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  const handleApprove = async () => {
    if (modalState.type !== 'consent') return;
    const { details } = modalState;

    setIsProcessing(true);
    setProcessError(null);

    try {
      const response = await invoke<ConsentApprovalResponse>('approve_oauth_consent', {
        request: {
          request_id: details.requestId,
          approved: true,
          consent_token: details.consentToken,
          client_alias: null,
        },
      });

      if (response.success && response.redirect_url) {
        await openRedirectUrl(response.redirect_url);
        setModalState({ type: 'hidden' });
      } else {
        setProcessError(response.error || 'Failed to approve connection');
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
      const response = await invoke<ConsentApprovalResponse>('approve_oauth_consent', {
        request: {
          request_id: details.requestId,
          approved: false,
          consent_token: details.consentToken,
          client_alias: null,
        },
      });

      if (response.success && response.redirect_url) {
        await openRedirectUrl(response.redirect_url);
        setModalState({ type: 'hidden' });
      } else {
        setProcessError(response.error || 'Failed to deny connection');
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
              Validating authorization request…
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }

  if (modalState.type === 'error') {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
        <Card className="animate-in fade-in zoom-in mx-4 w-full max-w-md shadow-xl duration-200">
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="rounded-full bg-red-500/10 p-2">
                <AlertCircle className="h-6 w-6 text-red-500" />
              </div>
              <div>
                <CardTitle>Authorization Failed</CardTitle>
                <CardDescription>
                  Could not process the authorization request
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <p className="text-sm text-[rgb(var(--muted))]">
              {getErrorMessage(modalState.error)}
            </p>
            <Button onClick={handleDismiss} className="w-full">
              Close
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
              Allow {details.clientName} to connect?
            </h2>
            <p className="text-sm text-[rgb(var(--muted))]">
              It will be able to call tools you enable for this folder.
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
              {approveReady ? 'Allow' : 'Allow (wait…)'}
            </Button>
            <Button
              variant="secondary"
              className="w-full"
              onClick={handleDeny}
              disabled={isProcessing}
            >
              <X className="mr-2 h-4 w-4" />
              Deny
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
