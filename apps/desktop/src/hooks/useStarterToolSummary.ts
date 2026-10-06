import { useEffect, useState } from 'react';
import { getStarterToolSummary, type StarterToolSummary } from '@/lib/api/featureSets';
import { useDomainEvents } from './useDomainEvents';

/**
 * What the Space's Starter gives connected apps (tool count, server count,
 * auto vs manual, over the size warning), kept live as servers connect,
 * discover tools, or the Starter is edited — from this window or by `@mux`.
 */
export function useStarterToolSummary(spaceId: string | null | undefined) {
  const [loaded, setLoaded] = useState<{
    spaceId: string;
    summary: StarterToolSummary | null;
  } | null>(null);
  // Bumped by domain events to trigger a re-fetch.
  const [version, setVersion] = useState(0);

  useEffect(() => {
    if (!spaceId) return;
    let cancelled = false;
    getStarterToolSummary(spaceId)
      .then((summary) => {
        if (!cancelled) setLoaded({ spaceId, summary });
      })
      .catch((err) => console.error('Failed to load Starter tool summary:', err));
    return () => {
      cancelled = true;
    };
  }, [spaceId, version]);

  const { subscribe } = useDomainEvents();
  useEffect(() => {
    const bump = () => setVersion((v) => v + 1);
    const unsubs = [
      subscribe('feature-set-changed', bump),
      subscribe('server-changed', bump),
      subscribe('server-status-changed', bump),
      subscribe('server-features-refreshed', bump),
    ];
    return () => unsubs.forEach((u) => u());
  }, [subscribe]);

  // Never show a previous Space's numbers while the new one loads.
  const summary = spaceId && loaded?.spaceId === spaceId ? loaded.summary : null;
  return { summary };
}
