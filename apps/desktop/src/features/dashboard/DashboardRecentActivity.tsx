import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckCircle2, Eye, ShieldAlert, XCircle } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@mcpmux/ui';
import type { MetaToolAuditEvent } from '@/lib/api/metaTools';
import { useMetaToolEventListener } from '@/hooks/useMetaToolEvents';

const MAX_ROWS = 5;

/**
 * Pick the icon shown beside a meta-tool audit row based on the gateway decision.
 */
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
    default:
      return <ShieldAlert className={`${className} text-amber-500`} />;
  }
}

/**
 * Compact live feed of recent `mcpmux_*` meta-tool invocations from connected clients.
 */
export function DashboardRecentActivity() {
  const { t } = useTranslation('dashboard');
  const [rows, setRows] = useState<MetaToolAuditEvent[]>([]);

  const appendRow = useCallback((event: MetaToolAuditEvent) => {
    setRows((prev) => {
      const next = [event, ...prev];
      return next.length > MAX_ROWS ? next.slice(0, MAX_ROWS) : next;
    });
  }, []);

  useMetaToolEventListener(appendRow);

  return (
    <Card data-testid="dashboard-recent-activity">
      <CardHeader>
        <CardTitle className="text-base flex items-center gap-2">
          <Eye className="h-4 w-4" />
          {t('activity.title')}
        </CardTitle>
        <CardDescription>{t('activity.description', { count: MAX_ROWS })}</CardDescription>
      </CardHeader>
      <CardContent>
        {rows.length === 0 ? (
          <p className="text-sm italic text-[rgb(var(--muted))]">
            {t('activity.empty.before')}
            <code className="font-mono">mcpmux_*</code>
            {t('activity.empty.after')}
          </p>
        ) : (
          <ul className="divide-y divide-[rgb(var(--border-subtle))]">
            {rows.map((row, index) => (
              <li
                key={`${row.timestamp}:${index}`}
                className="flex items-start gap-2 py-2.5 first:pt-0 last:pb-0"
                data-testid={`dashboard-activity-row-${row.tool_name}`}
              >
                <DecisionIcon decision={row.decision} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <code className="truncate font-mono text-xs font-medium">{row.tool_name}</code>
                    <span className="rounded border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-1.5 py-0.5 text-[10px] uppercase tracking-wide">
                      {row.decision}
                    </span>
                  </div>
                  <div className="mt-0.5 truncate text-[11px] text-[rgb(var(--muted))]">
                    {t('activity.clientPrefix', { id: row.client_id.slice(0, 8) })} •{' '}
                    {new Date(row.timestamp).toLocaleTimeString()}
                  </div>
                  {row.summary && (
                    <div className="mt-0.5 truncate text-[11px] text-[rgb(var(--muted))]">
                      {row.summary}
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
