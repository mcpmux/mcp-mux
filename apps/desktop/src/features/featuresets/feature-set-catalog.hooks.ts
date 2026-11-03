import { useEffect, useState } from 'react';
import { listInstalledServers } from '@/lib/api/registry';
import type { ServerFeature } from '@/lib/api/serverFeatures';
import { listServerFeatures } from '@/lib/api/serverFeatures';
import type { InstalledServerState } from '@/types/registry';

/**
 * Every feature in a Space (including ones from offline servers) plus the installed
 * servers used to name and icon their groups.
 */
export function useFeatureSetCatalog(spaceId: string, featureSetId: string) {
  const [features, setFeatures] = useState<ServerFeature[]>([]);
  const [installed, setInstalled] = useState<InstalledServerState[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let isCurrent = true;
    const load = async () => {
      setIsLoading(true);
      setLoadError(null);
      try {
        const [loadedFeatures, loadedInstalled] = await Promise.all([
          listServerFeatures(spaceId, true),
          listInstalledServers(spaceId),
        ]);
        if (!isCurrent) {
          return;
        }
        setFeatures(loadedFeatures);
        setInstalled(loadedInstalled);
      } catch (e) {
        if (isCurrent) {
          setLoadError(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (isCurrent) {
          setIsLoading(false);
        }
      }
    };
    void load();
    return () => {
      isCurrent = false;
    };
  }, [spaceId, featureSetId]);

  return { features, installed, isLoading, loadError };
}
