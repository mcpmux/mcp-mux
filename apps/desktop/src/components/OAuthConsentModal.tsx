/**
 * OAuth Consent Modal
 *
 * Displays when an MCP client requests authorization via deep link.
 * 
 * ## Flow
 * 1. Deep link received with request_id only
 * 2. Call get_pending_consent to validate and get full details from backend
 * 3. Only show modal if validation succeeds
 * 4. On approve/deny, call approve_oauth_consent
 */

import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Check, X, AlertCircle, Loader2, Globe, Lock } from 'lucide-react';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@mcpmux/ui';
import { listSpaces, type Space } from '@/lib/api/spaces';
import { resolveKnownClientKey } from '@/lib/clientIcons';
import cursorIcon from '@/assets/client-icons/cursor.svg';
import vscodeIcon from '@/assets/client-icons/vscode.png';
import claudeIcon from '@/assets/client-icons/claude.svg';
import windsurfIcon from '@/assets/client-icons/windsurf.svg';

/** Bundled icon assets for known clients */
const CLIENT_ICON_ASSETS: Record<string, string> = {
  cursor: cursorIcon,
  vscode: vscodeIcon,
  claude: claudeIcon,
  windsurf: windsurfIcon,
};

/** Look up a bundled logo for a known client by name */
function getClientLogo(clientName: string): string | null {
  const key = resolveKnownClientKey(clientName);
  return key ? CLIENT_ICON_ASSETS[key] ?? null : null;
}

/** Minimal deep link payload - only request_id */
interface OAuthDeepLinkPayload {
  requestId: string;
}

/** Full consent details from backend (camelCase from Rust serde) */
interface ConsentRequestDetails {
  requestId: string;
  clientId: string;
  clientName: string;
  redirectUri: string;
  scope: string;
  state: string | null;
  expiresAt: number;
}

/** Error from get_pending_consent */
interface ConsentError {
  code: 'NOT_FOUND' | 'EXPIRED' | 'ALREADY_PROCESSED' | 'GATEWAY_UNAVAILABLE';
  message: string;
}

/** Response from approve_oauth_consent command */
interface ConsentApprovalResponse {
  success: boolean;
  redirect_url: string;
  error: string | null;
}

/** Current modal state */
type ModalState = 
  | { type: 'hidden' }
  | { type: 'loading'; requestId: string }
  | { type: 'error'; requestId: string; error: ConsentError }
  | { type: 'consent'; details: ConsentRequestDetails };

/** Open a URL using the backend open command (handles custom protocols like cursor://) */
async function openRedirectUrl(url: string): Promise<void> {
  try {
    // Use the backend open_url command which uses the 'open' crate
    // This works for custom protocols that the webview opener plugin can't handle
    const { openUrl } = await import('@/lib/api/gateway');
    await openUrl(url);
  } catch (err) {
    console.error('[OAuth] openUrl failed:', err);
    // Fallback: try the webview opener plugin
    try {
      const { openUrl: openUrlPlugin } = await import('@tauri-apps/plugin-opener');
      await openUrlPlugin(url);
    } catch (pluginErr) {
      console.error('[OAuth] Plugin opener also failed:', pluginErr);
      // Last resort - redirect directly (won't work for custom protocols)
      window.location.href = url;
    }
  }
}

/** Get user-friendly error message */
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
  const [clientAlias, setClientAlias] = useState('');
  const [connectionMode, setConnectionMode] = useState<'follow_active' | 'locked'>('follow_active');
  const [lockedSpaceId, setLockedSpaceId] = useState<string | null>(null);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [isProcessing, setIsProcessing] = useState(false);
  const [processError, setProcessError] = useState<string | null>(null);

  // Load spaces when modal opens
  useEffect(() => {
    if (modalState.type === 'consent') {
      listSpaces().then(setSpaces).catch(console.error);
    }
  }, [modalState.type]);

  useEffect(() => {
    // Listen for OAuth consent requests from the backend (deep link)
    const unlisten = listen<OAuthDeepLinkPayload>('oauth-consent-request', async (event) => {
      console.log('[OAuth] Received deep link, validating request:', event.payload.requestId);
      
      const requestId = event.payload.requestId;
      setModalState({ type: 'loading', requestId });
      setClientAlias('');
      setConnectionMode('follow_active');
      setLockedSpaceId(null);
      setProcessError(null);
      
      try {
        // Validate and get full details from backend
        const details = await invoke<ConsentRequestDetails>('get_pending_consent', {
          requestId,
        });
        
        console.log('[OAuth] Consent validated:', details);
        setModalState({ type: 'consent', details });
        setClientAlias(details.clientName);
      } catch (err) {
        console.error('[OAuth] Validation failed:', err);
        // err is the ConsentError from backend
        const error = err as ConsentError;
        setModalState({ type: 'error', requestId, error });
      }
    });

    return () => {
      unlisten.then(fn => fn());
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
          client_alias: clientAlias || null,
          connection_mode: connectionMode,
          locked_space_id: connectionMode === 'locked' ? lockedSpaceId : null,
        },
      });

      if (response.success && response.redirect_url) {
        console.log('[OAuth] Approved, redirecting to:', response.redirect_url);
        await openRedirectUrl(response.redirect_url);
        setModalState({ type: 'hidden' });
      } else {
        setProcessError(response.error || 'Failed to approve consent');
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
          client_alias: null,
        },
      });

      if (response.success && response.redirect_url) {
        console.log('[OAuth] Denied, redirecting to:', response.redirect_url);
        await openRedirectUrl(response.redirect_url);
        setModalState({ type: 'hidden' });
      } else {
        setProcessError(response.error || 'Failed to deny consent');
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

  // Hidden state - render nothing
  if (modalState.type === 'hidden') return null;

  // Loading state - show spinner
  if (modalState.type === 'loading') {
    return (
      <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50">
        <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
          <CardContent className="py-8 flex flex-col items-center gap-4">
            <Loader2 className="h-8 w-8 animate-spin text-primary-500" />
            <p className="text-[rgb(var(--muted))]">Validating authorization request...</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Error state - show error with dismiss button
  if (modalState.type === 'error') {
    return (
      <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50">
        <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
          <CardHeader>
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-full bg-red-500/10">
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

  // Consent state - show approval modal
  const { details } = modalState;
  const scopes = details.scope?.split(' ').filter(Boolean) || ['mcp'];
  const logoUrl = getClientLogo(details.clientName);

  return (
    <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50">
      <Card className="w-full max-w-md mx-4 shadow-xl animate-in fade-in zoom-in duration-200">
        <CardHeader>
          <div className="flex items-center gap-3">
            <img
              src="/mcpmux.svg"
              alt="McpMux"
              className="h-10 w-10 rounded-lg"
            />
            <div>
              <CardTitle>Authorization Request</CardTitle>
              <CardDescription>
                {details.clientName} wants to connect
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Client Info */}
          <div className="flex items-center gap-3 p-4 rounded-lg bg-surface-hover border border-[rgb(var(--border))]">
            {logoUrl && (
              <img
                src={logoUrl}
                alt={details.clientName}
                className="h-8 w-8 rounded-lg"
              />
            )}
            <div>
              <div className="font-medium text-lg">{details.clientName}</div>
              <div className="text-sm text-[rgb(var(--muted))] mt-0.5 break-all">
                {details.clientId.length > 50
                  ? `${details.clientId.substring(0, 50)}...`
                  : details.clientId}
              </div>
            </div>
          </div>

          {/* Scopes */}
          <div>
            <div className="text-sm font-medium mb-2">Requested permissions:</div>
            <div className="flex flex-wrap gap-2">
              {scopes.map((scope, i) => (
                <span
                  key={i}
                  className="px-2 py-1 text-xs rounded-full bg-primary-500/10 text-primary-500 border border-primary-500/20"
                >
                  {scope}
                </span>
              ))}
            </div>
          </div>

          {/* Alias Input */}
          <div>
            <label className="text-sm font-medium">
              Display name (optional)
            </label>
            <input
              type="text"
              value={clientAlias}
              onChange={(e) => setClientAlias(e.target.value)}
              placeholder="e.g., Work Cursor, Personal Claude"
              className="mt-1 w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] text-[rgb(var(--foreground))] placeholder:text-[rgb(var(--muted))] focus:outline-none focus:ring-2 focus:ring-primary-500/20"
            />
            <p className="text-xs text-[rgb(var(--muted))] mt-1">
              Give this client a friendly name to identify it later
            </p>
          </div>

          {/* Space Mode Selection */}
          <div>
            <label className="text-sm font-medium mb-2 block">
              Space connection mode
            </label>
            <div className="space-y-2">
              {/* Follow Active Option */}
              <label
                className={`flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors ${
                  connectionMode === 'follow_active'
                    ? 'border-primary-500 bg-primary-500/5'
                    : 'border-[rgb(var(--border))] hover:border-[rgb(var(--border-hover))]'
                }`}
              >
                <input
                  type="radio"
                  name="connectionMode"
                  value="follow_active"
                  checked={connectionMode === 'follow_active'}
                  onChange={() => setConnectionMode('follow_active')}
                  className="sr-only"
                />
                <Globe className={`h-5 w-5 ${connectionMode === 'follow_active' ? 'text-primary-500' : 'text-[rgb(var(--muted))]'}`} />
                <div className="flex-1">
                  <div className={`font-medium text-sm ${connectionMode === 'follow_active' ? 'text-primary-500' : ''}`}>
                    Follow Active Space
                  </div>
                  <div className="text-xs text-[rgb(var(--muted))]">
                    Client sees servers from whichever space is active
                  </div>
                </div>
                {connectionMode === 'follow_active' && (
                  <Check className="h-4 w-4 text-primary-500" />
                )}
              </label>

              {/* Lock to Space Option */}
              <label
                className={`flex items-center gap-3 p-3 rounded-lg border cursor-pointer transition-colors ${
                  connectionMode === 'locked'
                    ? 'border-primary-500 bg-primary-500/5'
                    : 'border-[rgb(var(--border))] hover:border-[rgb(var(--border-hover))]'
                }`}
              >
                <input
                  type="radio"
                  name="connectionMode"
                  value="locked"
                  checked={connectionMode === 'locked'}
                  onChange={() => setConnectionMode('locked')}
                  className="sr-only"
                />
                <Lock className={`h-5 w-5 ${connectionMode === 'locked' ? 'text-primary-500' : 'text-[rgb(var(--muted))]'}`} />
                <div className="flex-1">
                  <div className={`font-medium text-sm ${connectionMode === 'locked' ? 'text-primary-500' : ''}`}>
                    Lock to Space
                  </div>
                  <div className="text-xs text-[rgb(var(--muted))]">
                    Client always sees servers from a specific space
                  </div>
                </div>
                {connectionMode === 'locked' && (
                  <Check className="h-4 w-4 text-primary-500" />
                )}
              </label>
            </div>

            {/* Space Selector (only when locked) */}
            {connectionMode === 'locked' && spaces.length > 0 && (
              <div className="mt-3">
                <select
                  value={lockedSpaceId || ''}
                  onChange={(e) => setLockedSpaceId(e.target.value || null)}
                  className="w-full px-3 py-2 rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--surface))] text-[rgb(var(--foreground))] focus:outline-none focus:ring-2 focus:ring-primary-500/20"
                >
                  <option value="">Select a space to lock to...</option>
                  {spaces.map((space) => (
                    <option key={space.id} value={space.id}>
                      {space.icon ? `${space.icon} ${space.name}` : space.name}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </div>

          {/* Error Message */}
          {processError && (
            <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 text-red-500 text-sm">
              <AlertCircle className="h-4 w-4 flex-shrink-0" />
              <span>{processError}</span>
            </div>
          )}

          {/* Action Buttons */}
          <div className="flex gap-3 pt-2">
            <Button
              variant="secondary"
              className="flex-1"
              onClick={handleDeny}
              disabled={isProcessing}
            >
              <X className="h-4 w-4 mr-2" />
              Deny
            </Button>
            <Button
              variant="primary"
              className="flex-1"
              onClick={handleApprove}
              disabled={isProcessing}
            >
              {isProcessing ? (
                <div className="h-4 w-4 mr-2 animate-spin rounded-full border-2 border-current border-t-transparent" />
              ) : (
                <Check className="h-4 w-4 mr-2" />
              )}
              Approve
            </Button>
          </div>

          {/* Dismiss Link */}
          <div className="text-center">
            <button
              onClick={handleDismiss}
              className="text-xs text-[rgb(var(--muted))] hover:text-[rgb(var(--foreground))] transition-colors"
            >
              Dismiss (client will wait)
            </button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
