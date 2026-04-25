import { useState, useEffect, useCallback } from 'react';
import { Space, listSpaces, createSpace, deleteSpace } from '@/lib/api/spaces';

/**
 * Hook for managing spaces (isolated environments).
 *
 * Note: there's no longer an "active space" concept — gateway routing is
 * decided per reported workspace root via WorkspaceBinding, with the
 * `is_default` Space as the fallback. The desktop UI still tracks which
 * space the user is *viewing* via `viewSpaceId` in the Zustand store.
 */
export function useSpaces() {
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [defaultSpace, setDefaultSpace] = useState<Space | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);

      const spacesList = await listSpaces();
      setSpaces(spacesList);
      setDefaultSpace(spacesList.find((s) => s.is_default) ?? null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      console.error('Failed to load spaces:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const create = useCallback(
    async (name: string, icon?: string): Promise<Space> => {
      const space = await createSpace(name, icon);
      await refresh();
      return space;
    },
    [refresh]
  );

  const remove = useCallback(
    async (id: string): Promise<void> => {
      await deleteSpace(id);
      await refresh();
    },
    [refresh]
  );

  return {
    spaces,
    defaultSpace,
    loading,
    error,
    refresh,
    create,
    remove,
  };
}
