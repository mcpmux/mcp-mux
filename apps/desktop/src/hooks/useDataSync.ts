import { useEffect } from 'react';
import { useAppStore } from '@/stores/appStore';
import { listSpaces, getActiveSpace } from '@/lib/api/spaces';
import { refreshOAuthTokensOnStartup } from '@/lib/api/gateway';

/**
 * Syncs data from Rust backend to Zustand store.
 * Run once at app startup.
 */
export function useDataSync() {
  const setSpaces = useAppStore((state) => state.setSpaces);
  const setLoading = useAppStore((state) => state.setLoading);
  const setActiveSpaceInStore = useAppStore((state) => state.setActiveSpace);

  useEffect(() => {
    async function syncData() {
      console.log('[useDataSync] Starting data sync...');
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

        // Fetch spaces and active space from backend
        console.log('[useDataSync] Calling listSpaces...');
        const spaces = await listSpaces();
        console.log('[useDataSync] listSpaces returned:', spaces.length, 'spaces', spaces);

        console.log('[useDataSync] Calling getActiveSpace...');
        const activeSpace = await getActiveSpace();
        console.log('[useDataSync] getActiveSpace returned:', activeSpace);
        
        console.log('[useDataSync] Setting spaces in store...');
        setSpaces(spaces);

        // Set active space from backend
        if (activeSpace) {
          console.log('[useDataSync] Setting active space:', activeSpace.id);
          setActiveSpaceInStore(activeSpace.id);
        } else if (spaces.length > 0) {
          // If no active space but we have spaces, set the first one
          console.log('[useDataSync] No active space, using first space:', spaces[0].id);
          setActiveSpaceInStore(spaces[0].id);
        }
      } catch (error) {
        console.error('[useDataSync] Failed to sync:', error);
      } finally {
        setLoading('spaces', false);
        console.log('[useDataSync] Data sync complete');
      }
    }

    syncData();
  }, [setSpaces, setLoading, setActiveSpaceInStore]);
}
