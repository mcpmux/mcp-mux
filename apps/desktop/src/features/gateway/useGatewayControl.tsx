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

function sourceLabel(source: 'override' | 'configured' | 'default'): string {
  switch (source) {
    case 'configured':
      return 'your configured gateway port';
    case 'default':
      return 'the default gateway port';
    case 'override':
      return 'the requested gateway port';
  }
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
      title: 'Gateway port is in use',
      message:
        `${capitalize(sourceLabel(probe.source))} (:${probe.preferredPort}) is already ` +
        `taken by another process. Start the gateway on a different port that the system ` +
        `picks automatically? Your IDE configs will need to be updated to point at the new ` +
        `port.`,
      confirmLabel: 'Use another port',
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
        (allowFallback) =>
          startGateway({ port: opts?.port, allowDynamicFallback: allowFallback }),
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
    try {
      return await runStart(
        (allowFallback) =>
          restartGateway({ port: opts?.port, allowDynamicFallback: allowFallback }),
        opts?.port
      );
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
      title: 'Gateway port is in use',
      message:
        `${capitalize(sourceLabel(pie.source))} (:${pie.port}) is already in use. ` +
        `Start on a different port?`,
      confirmLabel: 'Use another port',
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

function parsePortFromUrl(url: string): number | null {
  const match = /:(\d+)(?:\/|$)/.exec(url);
  return match ? Number(match[1]) : null;
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
