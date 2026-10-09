import type { TransportConfig } from '@/types/registry';
import { describeLaunch } from '@/lib/serverLaunch';

/**
 * Shows the exact command or URL a server uses, so installing from the
 * registry is never a blind click. The text comes from the registry, so it
 * is rendered as plain text.
 */
export function TransportSummary({ transport }: { transport: TransportConfig }) {
  const label = transport.type === 'stdio' ? 'Runs on this computer' : 'Connects to';
  return (
    <div className="text-xs" data-testid="transport-summary">
      <div className="text-[rgb(var(--muted))] mb-1">{label}</div>
      <code className="block whitespace-pre-wrap break-all rounded bg-[rgb(var(--surface))] border border-[rgb(var(--border-subtle))] px-2 py-1.5 font-mono">
        {describeLaunch(transport)}
      </code>
    </div>
  );
}
