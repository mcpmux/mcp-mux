import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, CheckCircle2, SlidersHorizontal, XCircle } from 'lucide-react';
import { Button, Card, CardContent, CardHeader, CardTitle } from '@mcpmux/ui';
import { useBackendEventSubscription } from '@/lib/backend/events';
import { respondToMetaToolApproval } from '@/lib/api/metaTools';
import { useNavigate } from '@/hooks/use-navigate.hook';

/**
 * Incoming approval request emitted by the gateway's ApprovalBroker.
 * Shape mirrors `mcpmux_gateway::services::ApprovalRequest`.
 */
export interface ApprovalRequest {
  request_id: string;
  client_id: string;
  payload: {
    tool_name: string;
    summary: string;
    /** Target Space name for cross-Space write visibility. */
    space_name?: string | null;
    /**
     * Tool-list diff the dialog renders. Freeform by design — read defensively
     * via `toStringArray`; never assume a field exists.
     */
    diff: null | Record<string, unknown>;
    raw_args: unknown;
    affects_other_clients: boolean;
  };
  expires_at_unix_secs: number;
}

/** Coerce a freeform JSON value into a `string[]`, dropping non-strings. */
function toStringArray(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
}

type Decision = 'allow_once' | 'always_for_this_session_and_client' | 'deny';

/**
 * Global listener that renders an approval dialog whenever the gateway
 * asks for permission to run an `mcpmux_*` write tool. Place once, near the
 * root of the app.
 */
export function MetaToolApprovalDialog() {
  const { t } = useTranslation('metatools');
  const navigate = useNavigate();
  const [queue, setQueue] = useState<ApprovalRequest[]>([]);
  const current = queue[0];

  const enqueueApproval = useCallback((payload: ApprovalRequest) => {
    setQueue((prev) => [...prev, payload]);
  }, []);

  const handleResolved = useCallback((payload: { request_id: string }) => {
    setQueue((prev) => prev.filter((r) => r.request_id !== payload.request_id));
  }, []);

  useBackendEventSubscription<ApprovalRequest>('meta-tool-approval-request', enqueueApproval);
  useBackendEventSubscription<{ request_id: string; decision: string }>(
    'meta-tool-approval-resolved',
    handleResolved
  );

  const respond = useCallback(
    async (decision: Decision) => {
      if (!current) return;
      try {
        await respondToMetaToolApproval(
          current.request_id,
          current.client_id,
          current.payload.tool_name,
          decision
        );
      } catch (e) {
        console.warn('respond_to_meta_tool_approval failed', e);
      } finally {
        setQueue((prev) => prev.slice(1));
      }
    },
    [current]
  );

  const manageApprovals = useCallback(() => {
    void respond('deny');
    navigate('builtin-servers');
  }, [respond, navigate]);

  const rawDiff = current?.payload.diff ?? null;
  const added = useMemo(
    () => [...toStringArray(rawDiff?.added), ...toStringArray(rawDiff?.added_tools)],
    [rawDiff]
  );
  const removed = useMemo(() => toStringArray(rawDiff?.removed), [rawDiff]);
  const hasBeforeAfter = rawDiff != null && ('before' in rawDiff || 'after' in rawDiff);
  const beforeCount = toStringArray(rawDiff?.before).length;
  const afterCount = hasBeforeAfter ? toStringArray(rawDiff?.after).length : added.length;
  const hasDiff = rawDiff != null && (added.length > 0 || removed.length > 0 || hasBeforeAfter);
  const deltaLabel = `+${added.length} / -${removed.length}`;

  if (!current) return null;

  return (
    <div
      className="fixed inset-0 z-[1000] bg-black/40 backdrop-blur-sm flex items-center justify-center p-4"
      data-testid="meta-tool-approval-dialog"
    >
      <Card className="w-full max-w-xl shadow-2xl">
        <CardHeader className="flex flex-row items-center gap-2">
          <AlertTriangle className="h-5 w-5 text-amber-500" />
          <CardTitle className="text-base">{t('approval.title')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="text-sm">
            <p className="font-medium">{current.payload.summary}</p>
            <div className="flex flex-wrap items-center gap-2 mt-1">
              {current.payload.space_name && (
                <span
                  className="inline-flex items-center gap-1 rounded-full border border-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))] px-2 py-0.5 text-xs"
                  data-testid="meta-tool-approval-space"
                >
                  {t('approval.spaceLabel')}&nbsp;
                  <span className="font-medium">{current.payload.space_name}</span>
                </span>
              )}
              <span className="text-xs text-[rgb(var(--muted))] font-mono">
                {t('approval.toolLabel')}&nbsp;{current.payload.tool_name}
              </span>
            </div>
          </div>

          {current.payload.affects_other_clients && (
            <div
              className="flex items-start gap-2 p-3 rounded border border-amber-400/40 bg-amber-50/40 dark:bg-amber-900/20 text-xs"
              data-testid="meta-tool-approval-cross-client-warning"
            >
              <AlertTriangle className="h-4 w-4 text-amber-600 mt-0.5 shrink-0" />
              <span>
                {t('approval.crossClientWarning.before')}
                <code>tools/list</code>
                {t('approval.crossClientWarning.after')}
              </span>
            </div>
          )}

          {hasDiff && (
            <div className="border border-[rgb(var(--border-subtle))] rounded text-xs">
              <div className="grid grid-cols-3 divide-x divide-[rgb(var(--border-subtle))] bg-[rgb(var(--surface))]">
                <Stat label={t('approval.diff.before')} value={hasBeforeAfter ? beforeCount : '—'} />
                <Stat label={t('approval.diff.after')} value={afterCount} emphasis />
                <Stat label={t('approval.diff.delta')} value={deltaLabel} />
              </div>
              {(added.length > 0 || removed.length > 0) && (
                <div className="max-h-40 overflow-y-auto p-2 space-y-0.5 font-mono">
                  {added.map((tool) => (
                    <div key={`+${tool}`} className="text-green-600 dark:text-green-400">
                      + {tool}
                    </div>
                  ))}
                  {removed.map((tool) => (
                    <div key={`-${tool}`} className="text-red-600 dark:text-red-400">
                      − {tool}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          <div className="flex items-center justify-end gap-2 pt-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => respond('deny')}
              data-testid="meta-tool-approval-deny"
            >
              <XCircle className="h-4 w-4 mr-1" /> {t('approval.deny')}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => respond('always_for_this_session_and_client')}
              title={t('approval.alwaysForSessionTitle')}
              data-testid="meta-tool-approval-always"
            >
              {t('approval.alwaysForSession')}
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={() => respond('allow_once')}
              data-testid="meta-tool-approval-allow-once"
            >
              <CheckCircle2 className="h-4 w-4 mr-1" /> {t('approval.allowOnce')}
            </Button>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[rgb(var(--border-subtle))] pt-3">
            <button
              type="button"
              onClick={manageApprovals}
              className="inline-flex items-center gap-1.5 rounded-md border border-[rgb(var(--border))] bg-[rgb(var(--surface))] px-2.5 py-1.5 text-xs font-medium text-[rgb(var(--foreground))] transition-colors hover:border-primary-400 hover:bg-primary-50 hover:text-primary-700 dark:hover:bg-primary-900/20 dark:hover:text-primary-300"
              title={t('approval.manageLinkTitle')}
              data-testid="meta-tool-approval-manage-link"
            >
              <SlidersHorizontal className="h-3.5 w-3.5" />
              {t('approval.manageLink')}
            </button>
            {queue.length > 1 && (
              <span className="text-[11px] text-[rgb(var(--muted))]">
                {t('approval.morePending', { count: queue.length - 1 })}
              </span>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

/**
 * Single cell in the approval diff summary grid.
 */
function Stat({
  label,
  value,
  emphasis,
}: {
  label: string;
  value: number | string;
  emphasis?: boolean;
}) {
  return (
    <div className="p-2 flex flex-col">
      <span className="text-[10px] uppercase tracking-wide text-[rgb(var(--muted))]">
        {label}
      </span>
      <span className={emphasis ? 'text-base font-semibold' : 'text-sm font-medium'}>
        {value}
      </span>
    </div>
  );
}
