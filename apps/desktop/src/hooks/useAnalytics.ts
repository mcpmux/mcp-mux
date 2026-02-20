/**
 * Hook that subscribes to domain events and forwards core metrics to PostHog.
 *
 * Tracked: server_installed, server_uninstalled
 * (app_opened, registry_search, version, OS, location are handled elsewhere)
 */

import { useEffect } from 'react';
import { capture } from '@/lib/analytics';
import { useDomainEvents } from './useDomainEvents';
import type { ServerChangedPayload } from './useDomainEvents';

export function useAnalytics() {
  const { subscribe } = useDomainEvents();

  useEffect(() => {
    return subscribe('server-changed', (payload: ServerChangedPayload) => {
      if (payload.action === 'installed') {
        capture('server_installed', {
          server_id: payload.server_id,
          server_name: payload.server_name,
        });
      } else if (payload.action === 'uninstalled') {
        capture('server_uninstalled', {
          server_id: payload.server_id,
        });
      }
    });
  }, [subscribe]);
}
