import { useEffect, useState } from 'react';
import { getGatewayAuthDisabled } from '@/lib/api/workspaceInstall';
import { useGatewayEvents } from './useDomainEvents';

/**
 * Whether connecting apps need an access key (and so an approval step).
 * `null` until loaded. Re-reads when the gateway restarts, which is when a
 * network-access or public-URL change can flip the default.
 */
export function useGatewayAuthRequired(): boolean | null {
  const [authRequired, setAuthRequired] = useState<boolean | null>(null);

  const load = () => {
    getGatewayAuthDisabled()
      .then((disabled) => setAuthRequired(!disabled))
      .catch((err) => console.error('Failed to load auth setting:', err));
  };

  useEffect(load, []);
  useGatewayEvents((payload) => {
    if (payload.action === 'started') load();
  });

  return authRequired;
}

/** One sentence for the end of a "connect your app" instruction. */
export function connectFinishNote(authRequired: boolean | null): string {
  return authRequired === false
    ? 'It connects right away — no approval step.'
    : 'Approve it on this page when it connects.';
}
