import { useEffect } from 'react';
import { getGatewayStatus, isTauri, takePendingPortConflict } from '@/lib/backend';
import { useIsLoading } from '@/stores';
import { useGatewayControl } from './useGatewayControl';

/**
 * Polling schedule (ms after mount). Covers the realistic window for the
 * Rust auto-start task to complete its port probe. Short early polls catch
 * the common case; longer tails catch cold-start machines / slow disks.
 * Total max wait: ~4.75s before giving up silently.
 */
const POLL_SCHEDULE_MS = [0, 150, 300, 600, 1200, 2400];

/**
 * Mounts at the app root and resolves any auto-start port conflict the
 * backend deferred during launch.
 *
 * ## Why polling, not events
 *
 * Tauri events aren't buffered — if the Rust auto-start task emits
 * `gateway-autostart-port-conflict` before `listen()` has attached the
 * frontend listener, the event is dropped. Combined with React
 * StrictMode's double-mount in dev, the probability of this race is
 * noticeable.
 *
 * Polling `take_pending_port_conflict` (atomic read-and-clear on the
 * backend) plus `get_gateway_status` together covers all three
 * launch-time outcomes:
 *
 * 1. **Silent success** — port free, gateway auto-started. `getGatewayStatus`
 *    returns `running: true` → we exit.
 * 2. **Port conflict** — backend set `pending_port_conflict`. The take
 *    consumes it; we show the prompt.
 * 3. **Auto-start disabled** — neither a conflict nor a running gateway.
 *    We exhaust the poll schedule and exit quietly; user can start
 *    manually from the Dashboard.
 *
 * The backend `take` is atomic so the StrictMode double-mount never
 * produces duplicate prompts.
 */
export function AutoStartConflictResolver() {
  const gatewayControl = useGatewayControl();
  const isLoadingSpaces = useIsLoading('spaces');

  useEffect(() => {
    if (!isTauri() && isLoadingSpaces) {
      return;
    }

    let cancelled = false;

    (async () => {
      for (let i = 0; i < POLL_SCHEDULE_MS.length; i++) {
        if (cancelled) return;
        const delay = POLL_SCHEDULE_MS[i];
        if (delay > 0) {
          await new Promise((resolve) => setTimeout(resolve, delay));
        }
        if (cancelled) return;

        try {
          // If the gateway auto-started silently (port was free), we're
          // done — no need to keep probing.
          const status = await getGatewayStatus();
          if (cancelled) return;
          if (status.running) {
            console.log(
              `[AutoStart] attempt ${i + 1}: gateway already running (${status.url}) — nothing to resolve`
            );
            return;
          }

          const conflict = await takePendingPortConflict();
          if (cancelled) return;
          console.log(
            `[AutoStart] attempt ${i + 1}: takePendingPortConflict →`,
            conflict
          );

          if (conflict) {
            const outcome = await gatewayControl.start();
            console.log('[AutoStart] prompt outcome:', outcome);
            return;
          }
          // Otherwise keep polling — backend auto-start task may not have
          // run yet. Last iteration just bails (user can start manually).
        } catch (err) {
          console.error(
            `[AutoStart] attempt ${i + 1} failed — will retry:`,
            err
          );
        }
      }

      console.log(
        '[AutoStart] poll schedule exhausted — no conflict, no running gateway (likely auto-start disabled)'
      );
    })();

    return () => {
      cancelled = true;
    };
  }, [gatewayControl, isLoadingSpaces]);

  return <>{gatewayControl.ConfirmDialogElement}</>;
}
