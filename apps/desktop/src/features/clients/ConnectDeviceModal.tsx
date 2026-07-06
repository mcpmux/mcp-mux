import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import QRCode from 'qrcode';
import { Loader2, RefreshCw, Smartphone, X } from 'lucide-react';
import { Button, Card, CardContent } from '@mcpmux/ui';

/**
 * Result of the `mint_pairing_token` Tauri command.
 */
interface PairingTokenInfo {
  token: string;
  claimUrl: string;
  lanBaseUrl: string;
  expiresInSecs: number;
}

/**
 * "Connect another device" — mints a short-lived pairing token and renders it
 * as a QR code + link. Scanning it on a phone/laptop opens the gateway's claim
 * page, which issues that device its own API key (no IP typing, no manual
 * config). The token is single-use and expires; a visible countdown plus a
 * one-click "new code" keeps it honest.
 *
 * Requires the gateway to be running and network access to be on (otherwise the
 * LAN URL only resolves on this machine) — the caller gates on that and passes
 * `networkAccess` so we can warn.
 */
export function ConnectDeviceModal({
  networkAccess,
  onClose,
}: {
  networkAccess: boolean;
  onClose: () => void;
}) {
  const [info, setInfo] = useState<PairingTokenInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [remaining, setRemaining] = useState(0);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  const mint = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<PairingTokenInfo>('mint_pairing_token');
      setInfo(result);
      setRemaining(result.expiresInSecs);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setInfo(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void mint();
  }, [mint]);

  // Render the QR whenever a fresh token arrives.
  useEffect(() => {
    if (info && canvasRef.current) {
      void QRCode.toCanvas(canvasRef.current, info.claimUrl, {
        width: 220,
        margin: 1,
        errorCorrectionLevel: 'M',
      }).catch(() => {
        /* canvas draw failure is non-fatal — the link below still works */
      });
    }
  }, [info]);

  // Countdown; auto-refresh isn't done (the user may still be scanning), we
  // just reflect expiry and offer a manual refresh.
  useEffect(() => {
    if (remaining <= 0) return;
    const id = setInterval(() => setRemaining((r) => Math.max(0, r - 1)), 1000);
    return () => clearInterval(id);
  }, [remaining]);

  const expired = info !== null && remaining <= 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <Card className="animate-in fade-in zoom-in-95 w-full max-w-md duration-200">
        <CardContent className="p-6">
          <div className="mb-4 flex items-start justify-between">
            <div className="flex items-center gap-2">
              <Smartphone className="h-5 w-5" />
              <h2 className="text-lg font-semibold">Connect another device</h2>
            </div>
            <button
              onClick={onClose}
              className="rounded p-1 hover:bg-[rgb(var(--surface-hover))]"
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          {!networkAccess && (
            <div
              className="mb-4 rounded-lg border border-amber-300 bg-amber-50 p-3 text-xs text-amber-800 dark:border-amber-700/60 dark:bg-amber-900/20 dark:text-amber-300"
              data-testid="connect-device-network-warning"
            >
              Network access is off, so this link only works on this computer. Turn on{' '}
              <strong>Allow access from other devices</strong> in Settings → Gateway to pair a phone
              or another machine.
            </div>
          )}

          <p className="mb-4 text-sm text-[rgb(var(--muted))]">
            Scan this code on the other device. It opens a page that connects the device and gives
            it its own private key — no IP addresses or config to type.
          </p>

          {loading ? (
            <div className="flex h-56 items-center justify-center">
              <Loader2 className="h-6 w-6 animate-spin text-[rgb(var(--muted))]" />
            </div>
          ) : error ? (
            <div
              className="rounded-lg border border-red-300 bg-red-50 p-3 text-sm text-red-700 dark:border-red-700/60 dark:bg-red-900/20 dark:text-red-300"
              data-testid="connect-device-error"
            >
              {error}
            </div>
          ) : info ? (
            <div className="flex flex-col items-center gap-3">
              <div
                className={`rounded-xl bg-white p-3 ${expired ? 'opacity-30' : ''}`}
                data-testid="connect-device-qr"
              >
                <canvas ref={canvasRef} />
              </div>
              <div className="w-full break-all rounded-lg border border-[rgb(var(--border))] bg-[rgb(var(--background))] px-3 py-2 text-center font-mono text-xs">
                {info.claimUrl}
              </div>
              {expired ? (
                <p className="text-xs font-medium text-amber-600 dark:text-amber-400">
                  This code expired. Generate a new one.
                </p>
              ) : (
                <p className="text-xs text-[rgb(var(--muted))]">
                  Expires in {Math.floor(remaining / 60)}:
                  {String(remaining % 60).padStart(2, '0')}
                </p>
              )}
              <Button
                variant={expired ? 'primary' : 'secondary'}
                size="sm"
                onClick={() => void mint()}
                data-testid="connect-device-refresh"
              >
                <RefreshCw className="mr-2 h-4 w-4" />
                New code
              </Button>
            </div>
          ) : null}
        </CardContent>
      </Card>
    </div>
  );
}
