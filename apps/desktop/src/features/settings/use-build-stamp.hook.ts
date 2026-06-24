import { useEffect, useState } from 'react';
import { getBuildInfo, getVersion } from '@/lib/backend';
import {
  backendBuildInfoRows,
  buildStampDisplayRows,
  getSpaBuildStamp,
  type BuildStampRow,
} from '@/lib/build-info.helpers';

/** Result of loading version and build stamp metadata for Settings UI. */
export interface UseBuildStampResult {
  version: string;
  backendRows: BuildStampRow[];
  spaRows: BuildStampRow[];
  spaSha: string;
  backendSha: string;
  hasMismatch: boolean;
  loading: boolean;
  error: string | null;
}

/**
 * Load app version, SPA compile-time stamp, and backend build info for Settings display.
 */
export function useBuildStamp(): UseBuildStampResult {
  const spaStamp = getSpaBuildStamp();
  const [version, setVersion] = useState('');
  const [backendRows, setBackendRows] = useState<BuildStampRow[]>([]);
  const [backendSha, setBackendSha] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const spaRows = buildStampDisplayRows(spaStamp);
  const spaSha = spaStamp.gitSha;
  const hasMismatch = Boolean(spaSha && backendSha && spaSha !== backendSha);

  useEffect(() => {
    let cancelled = false;

    Promise.all([getVersion(), getBuildInfo()])
      .then(([nextVersion, buildInfo]) => {
        if (cancelled) return;
        setVersion(nextVersion);
        setBackendSha(buildInfo.git_sha);
        setBackendRows(backendBuildInfoRows(buildInfo));
        setError(null);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  return {
    version,
    backendRows,
    spaRows,
    spaSha,
    backendSha,
    hasMismatch,
    loading,
    error,
  };
}
