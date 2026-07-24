import { useEffect } from 'react';
import { useAppStore } from '@/stores/appStore';
import { listSpaces } from '@/lib/api/spaces';
import { refreshOAuthTokensOnStartup } from '@/lib/api/gateway';
import { isTauri } from '@/lib/backend/data/transport';
import { enableAdminSse } from '@/lib/backend/events/admin-sse-hub';

/**
 * Syncs data from Rust backend to Zustand store.
 * Run once at app startup.
 */
export function useDataSync() {
  const setSpaces = useAppStore((state) => state.setSpaces);
  const setLoading = useAppStore((state) => state.setLoading);

  useEffect(() => {
    async function syncData() {
      console.log('[useDataSync] Starting data sync...');
      if (!isTauri()) {
        enableAdminSse();
      }
      setLoading('spaces', true);
      try {
        // Refresh OAuth tokens first (before connecting servers)
        console.log('[useDataSync] Refreshing OAuth tokens...');
        try {
          const refreshResult = await refreshOAuthTokensOnStartup();
          console.log('[useDataSync] OAuth token refresh result:', refreshResult);
        } catch (error) {
          console.error('[useDataSync] OAuth token refresh failed (non-fatal):', error);
        }

        console.log('[useDataSync] Calling listSpaces...');
        const spaces = await listSpaces();
        console.log('[useDataSync] listSpaces returned:', spaces.length, 'spaces');

        // setSpaces handles validating viewSpaceId and falling back to the
        // is_default space when the persisted view space doesn't exist.
        setSpaces(spaces);
      } catch (error) {
        console.error('[useDataSync] Failed to sync:', error);
      } finally {
        setLoading('spaces', false);
        console.log('[useDataSync] Data sync complete');
      }
    }

    syncData();
  }, [setSpaces, setLoading]);
}
