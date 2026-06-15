import { CheckCircle2, Eye, ShieldAlert, XCircle } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@mcpmux/ui';
import {
  MAX_META_TOOL_ROWS as MAX_ROWS,
  useMetaToolActivityStore,
} from '@/stores/metaToolActivityStore';

/**
 * Audit log of every `mcpmux_*` invocation (read or write, success or failure).
 *
 * Rows live in a global store fed by an app-level `meta-tool-invoked` listener
 * (see metaToolActivityStore) so they persist across tab changes and capture
 * calls that fired before this panel was opened. The persistent audit stream
 * lives in the gateway's tracing logs.
 */
export function MetaToolAuditLog() {
  const rows = useMetaToolActivityStore((s) => s.rows);

  return (
    <Card data-testid="meta-tool-audit-log">
      <CardHeader>
        <CardTitle className="text-base flex items-center gap-2">
          <Eye className="h-4 w-4" />
          Recent meta-tool activity
        </CardTitle>
        <p className="text-xs text-[rgb(var(--muted))] mt-1">
          Every call to <code className="font-mono">mcpmux_*</code> made by a
          connected MCP client. Live — last {MAX_ROWS} entries.
        </p>
      </CardHeader>
      <CardContent>
        {rows.length === 0 ? (
          <p className="text-sm text-[rgb(var(--muted))] italic">
            No activity yet. Rows appear as MCP clients call meta tools.
          </p>
        ) : (
          <ul className="divide-y divide-[rgb(var(--border-subtle))] max-h-80 overflow-y-auto">
            {rows.map((r, i) => (
              <li
                key={`${r.timestamp}:${i}`}
                className="flex items-start gap-2 py-2 text-xs"
                data-testid={`meta-tool-audit-row-${r.tool_name}`}
              >
                <DecisionIcon decision={r.decision} />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <code className="font-mono font-medium truncate">
                      {r.tool_name}
                    </code>
                    <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-[rgb(var(--surface))] border border-[rgb(var(--border-subtle))]">
                      {r.decision}
                    </span>
                  </div>
                  <div className="text-[11px] text-[rgb(var(--muted))] mt-0.5 truncate">
                    client {r.client_id.slice(0, 8)}… •{' '}
                    {new Date(r.timestamp).toLocaleTimeString()}
                  </div>
                  {r.summary && (
                    <div className="text-[11px] text-[rgb(var(--muted))] mt-0.5 truncate">
                      {r.summary}
                    </div>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

function DecisionIcon({ decision }: { decision: string }) {
  const className = 'h-4 w-4 mt-0.5 flex-shrink-0';
  switch (decision) {
    case 'read':
      return <Eye className={`${className} text-[rgb(var(--muted))]`} />;
    case 'allow_once':
    case 'always_for_this_session_and_client':
      return <CheckCircle2 className={`${className} text-green-500`} />;
    case 'deny':
    case 'timeout':
    case 'rate_limited':
    case 'approval_required':
      return <XCircle className={`${className} text-red-500`} />;
    case 'invalid_args':
    case 'error':
    default:
      return <ShieldAlert className={`${className} text-amber-500`} />;
  }
}
