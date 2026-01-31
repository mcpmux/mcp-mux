import { useState, useEffect, useCallback } from 'react';
import {
  Space,
  listSpaces,
  createSpace,
  deleteSpace,
  setActiveSpace,
  getActiveSpace,
} from '@/lib/api/spaces';

/**
 * Hook for managing spaces (isolated environments).
 */
export function useSpaces() {
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [activeSpace, setActiveSpaceState] = useState<Space | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Refresh the list of spaces
  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);

      const [spacesList, active] = await Promise.all([listSpaces(), getActiveSpace()]);

      setSpaces(spacesList);
      setActiveSpaceState(active);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      console.error('Failed to load spaces:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  // Load spaces on mount
  useEffect(() => {
    refresh();
  }, [refresh]);

  // Create a new space
  const create = useCallback(
    async (name: string, icon?: string): Promise<Space> => {
      const space = await createSpace(name, icon);
      await refresh();
      return space;
    },
    [refresh]
  );

  // Delete a space
  const remove = useCallback(
    async (id: string): Promise<void> => {
      await deleteSpace(id);
      await refresh();
    },
    [refresh]
  );

  // Set the active space
  const setActive = useCallback(
    async (id: string): Promise<void> => {
      await setActiveSpace(id);
      await refresh();
    },
    [refresh]
  );

  return {
    spaces,
    activeSpace,
    loading,
    error,
    refresh,
    create,
    remove,
    setActive,
  };
}
