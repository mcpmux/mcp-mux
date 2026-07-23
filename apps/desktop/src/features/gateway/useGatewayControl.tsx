import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { useConfirm } from '@mcpmux/ui';
import {
  probeGatewayStart,
  startGateway,
  restartGateway,
  parsePortInUseError,
} from '@/lib/api/gateway';

/**
 * Shape of the outcome returned by start/restart helpers.
 *
 * `cancelled` signals the user dismissed the port-in-use prompt — callers
 * should treat it as a non-error (no toast, just stop).
 */
export type GatewayStartOutcome =
  | { status: 'started'; url: string; fellBackToDynamic: boolean; port: number }
  | { status: 'cancelled' };

type PortSource = 'override' | 'configured' | 'default';

/**
 * Returns the localized label for which gateway port source is in conflict.
 */
function sourceLabel(t: TFunction<'clients'>, source: PortSource): string {
  return t(`gatewayConfirm.source.${source}`);
}

/**
 * Hook that handles the probe → confirm → start flow uniformly across the
 * Dashboard, Servers page, and Settings page. Render `ConfirmDialogElement`
 * once inside the consuming component.
 *
 * When the preferred port is taken, the user is shown a dialog asking
 * whether to let the gateway bind to a different (OS-assigned) port. If
 * they cancel, the returned outcome is `{ status: 'cancelled' }` and no
 * error is thrown — the caller can exit silently.
 */
export function useGatewayControl() {
  const { t } = useTranslation('clients');
  const { t: tCommon } = useTranslation('common');
  const { confirm, ConfirmDialogElement } = useConfirm();

  const runStart = async (
    invoker: (allowFallback: boolean) => Promise<string>,
    probePort?: number
  ): Promise<GatewayStartOutcome> => {
    console.log('[Gateway] probeGatewayStart({port:', probePort, '})');
    const probe = await probeGatewayStart(probePort);
    console.log('[Gateway] probe result:', probe);

    if (probe.preferredAvailable) {
      console.log('[Gateway] preferred port free → strict start');
      const url = await invoker(false);
      const port = parsePortFromUrl(url) ?? probe.preferredPort;
      console.log('[Gateway] strict start ok →', url);
      return { status: 'started', url, port, fellBackToDynamic: false };
    }

    console.log('[Gateway] preferred port taken → prompting user');
    const ok = await confirm({
      title: t('gatewayConfirm.title'),
      message: t('gatewayConfirm.portTakenMessage', {
        sourceLabel: sourceLabel(t, probe.source),
        port: probe.preferredPort,
      }),
      confirmLabel: t('gatewayConfirm.confirmLabel'),
      cancelLabel: tCommon('actions.cancel'),
      variant: 'default',
    });

    if (!ok) {
      console.log('[Gateway] user cancelled — gateway stays stopped');
      return { status: 'cancelled' };
    }

    console.log('[Gateway] user confirmed → fallback start with dynamic port');
    const url = await invoker(true);
    const port = parsePortFromUrl(url) ?? probe.preferredPort;
    console.log('[Gateway] fallback start ok →', url);
    return {
      status: 'started',
      url,
      port,
      fellBackToDynamic: true,
    };
  };

  const start = async (opts?: { port?: number }): Promise<GatewayStartOutcome> => {
    try {
      return await runStart(
        (allowFallback) => startGateway({ port: opts?.port, allowDynamicFallback: allowFallback }),
        opts?.port
      );
    } catch (err) {
      // If we hit a race (probe said free, bind failed) or any other bind
      // error, surface it with the structured prompt flow.
      return await handleBindFailure(err, opts?.port, (allowFallback) =>
        startGateway({ port: opts?.port, allowDynamicFallback: allowFallback })
      );
    }
  };

  const restart = async (opts?: { port?: number }): Promise<GatewayStartOutcome> => {
    // Unlike start(), restart never pre-probes the port: the gateway we're
    // about to replace is still holding it at probe time, so
    // probeGatewayStart() would see our own listener and spuriously report
    // the port as taken. Restart owns the port by definition — go straight
    // to restartGateway() and only surface the confirm dialog on a genuine
    // bind failure (handleBindFailure), same as a real conflict from start().
    console.log('[Gateway] restart → skipping pre-probe, going straight to restartGateway');
    try {
      const url = await restartGateway({ port: opts?.port, allowDynamicFallback: false });
      const port = parsePortFromUrl(url) ?? opts?.port ?? 0;
      console.log('[Gateway] restart ok →', url);
      return { status: 'started', url, port, fellBackToDynamic: false };
    } catch (err) {
      return await handleBindFailure(err, opts?.port, (allowFallback) =>
        restartGateway({ port: opts?.port, allowDynamicFallback: allowFallback })
      );
    }
  };

  const handleBindFailure = async (
    err: unknown,
    port: number | undefined,
    invoker: (allowFallback: boolean) => Promise<string>
  ): Promise<GatewayStartOutcome> => {
    const pie = parsePortInUseError(err);
    if (!pie) throw err;
    const ok = await confirm({
      title: t('gatewayConfirm.title'),
      message: t('gatewayConfirm.portTakenShortMessage', {
        sourceLabel: sourceLabel(t, pie.source),
        port: pie.port,
      }),
      confirmLabel: t('gatewayConfirm.confirmLabel'),
      cancelLabel: tCommon('actions.cancel'),
    });
    if (!ok) return { status: 'cancelled' };
    const url = await invoker(true);
    return {
      status: 'started',
      url,
      port: parsePortFromUrl(url) ?? pie.port,
      fellBackToDynamic: true,
    };
  };

  return { start, restart, ConfirmDialogElement };
}

/**
 * Extracts the TCP port from a gateway URL string.
 */
function parsePortFromUrl(url: string): number | null {
  const match = /:(\d+)(?:\/|$)/.exec(url);
  return match ? Number(match[1]) : null;
}
